use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use thiserror::Error;

use crate::api::{ConnectorKind, Inventory, InventoryError, State, StateError, StateStage};
use crate::connectors::{
    ConnectionManager, ConnectionManagerError, Connector, LocalConnector, SshConnectionOptions, SshConnector,
    SshOptionsError,
};

use super::{ExecutionStrategy, PreparedDeploy, Scheduler, SchedulerError};

/// Connectors and lifecycle coordination for executing prepared Rust deploys.
pub struct DeployRuntime {
    connectors: IndexMap<String, Arc<dyn Connector>>,
}

impl DeployRuntime {
    #[must_use]
    pub fn new(connectors: IndexMap<String, Arc<dyn Connector>>) -> Self {
        Self { connectors }
    }

    /// Creates the built-in local and SSH connectors selected by an inventory.
    pub fn from_inventory(inventory: &Inventory, connect_timeout: Duration) -> Result<Self, DeployRuntimeError> {
        let mut connectors = IndexMap::<String, Arc<dyn Connector>>::new();
        for host in inventory.hosts() {
            let connector: Arc<dyn Connector> = match host.connector() {
                ConnectorKind::Local => Arc::new(LocalConnector),
                ConnectorKind::Ssh => {
                    let data = inventory.resolved_host_data(host.name())?;
                    let options = SshConnectionOptions::from_data(host.name(), &data, connect_timeout)?;
                    Arc::new(SshConnector::new(options))
                }
                ConnectorKind::External(connector) => {
                    return Err(DeployRuntimeError::UnsupportedConnector(connector.clone()));
                }
            };
            connectors.insert(host.name().to_owned(), connector);
        }
        Ok(Self::new(connectors))
    }

    /// Connects, executes the prepared plan, and disconnects every connected host.
    pub async fn execute(
        &self,
        state: &mut State,
        deploy: &PreparedDeploy,
        strategy: ExecutionStrategy,
    ) -> Result<(), DeployRuntimeError> {
        let manager = self.connection_manager();
        let result = async {
            state.set_stage(StateStage::Connect)?;
            manager.connect_all(state, None).await?;
            state.set_stage(StateStage::Prepare)?;
            let runner = Arc::new(deploy.runner(&self.connectors)?);
            state.set_stage(StateStage::Execute)?;
            let execution = Scheduler::new(runner.clone()).execute(state, deploy.plan(), strategy).await;
            for (host, results) in runner.results().await {
                state.set_host_results(host, results);
            }
            execution?;
            Ok(())
        }
        .await;

        state.set_stage(StateStage::Disconnect)?;
        let disconnect = manager.disconnect_all(state).await;
        match (result, disconnect) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error.into()),
            (Err(error), Err(disconnect)) => {
                Err(DeployRuntimeError::RunAndDisconnect { run: error.to_string(), disconnect: Box::new(disconnect) })
            }
        }
    }

    fn connection_manager(&self) -> ConnectionManager {
        let mut manager = ConnectionManager::default();
        for (host, connector) in &self.connectors {
            manager.insert(host, Arc::clone(connector));
        }
        manager
    }
}

/// Failure in a deploy lifecycle, including cleanup failures.
#[derive(Debug, Error)]
pub enum DeployRuntimeError {
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Connection(Box<ConnectionManagerError>),
    #[error(transparent)]
    Deploy(#[from] super::DeployError),
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
    #[error(transparent)]
    Inventory(#[from] InventoryError),
    #[error(transparent)]
    SshOptions(#[from] SshOptionsError),
    #[error("no runtime implementation is registered for connector {0}")]
    UnsupportedConnector(String),
    #[error("deploy failed ({run}) and disconnect also failed: {disconnect}")]
    RunAndDisconnect { run: String, disconnect: Box<ConnectionManagerError> },
}

impl From<ConnectionManagerError> for DeployRuntimeError {
    fn from(error: ConnectionManagerError) -> Self {
        Self::Connection(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use indexmap::IndexMap;

    use super::{DeployRuntime, Duration};
    use crate::api::{Config, ConnectorKind, Data, HostSpec, Inventory, InventorySpec, State, StateStage};
    use crate::connectors::{Connector, LocalConnector};
    use crate::operations::files::{File, FileInput};
    use crate::operations::server::{Shell, ShellCommands};
    use crate::operations::{DeployRuntimeError, ExecutionStrategy, OperationOptions, PreparedDeploy};

    #[test]
    fn runtime_builds_builtin_connectors_and_rejects_unimplemented_external_connectors() {
        let local =
            Inventory::from_spec(InventorySpec { hosts: vec![HostSpec::local()], ..InventorySpec::default() }).unwrap();
        let external = Inventory::from_spec(InventorySpec {
            hosts: vec![HostSpec {
                name: "container".to_owned(),
                data: Data::new(),
                connector: ConnectorKind::External("docker".to_owned()),
            }],
            ..InventorySpec::default()
        })
        .unwrap();

        assert!(DeployRuntime::from_inventory(&local, Duration::from_secs(1)).is_ok());
        assert!(matches!(
            DeployRuntime::from_inventory(&external, Duration::from_secs(1)),
            Err(DeployRuntimeError::UnsupportedConnector(connector)) if connector == "docker"
        ));
    }

    #[tokio::test]
    async fn runtime_executes_host_specific_commands_through_the_full_lifecycle() {
        let inventory = Inventory::from_spec(InventorySpec {
            hosts: vec![HostSpec::ssh("one"), HostSpec::ssh("two")],
            ..InventorySpec::default()
        })
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let output_path = directory.path().join("deploy-output");
        let deploy = PreparedDeploy::prepare(&inventory, |host| {
            host.add(Shell, ShellCommands::from(format!("echo {} >> {}", host.host().name(), output_path.display())))?;
            Ok(())
        })
        .unwrap();
        let connector: Arc<dyn Connector> = Arc::new(LocalConnector);
        let connectors =
            IndexMap::from([("one".to_owned(), Arc::clone(&connector)), ("two".to_owned(), Arc::clone(&connector))]);
        let mut state = State::new(inventory, Config::default());

        DeployRuntime::new(connectors).execute(&mut state, &deploy, ExecutionStrategy::Lockstep).await.unwrap();

        let mut output = fs::read_to_string(output_path).unwrap().lines().map(str::to_owned).collect::<Vec<_>>();
        output.sort();
        assert_eq!(output, ["one", "two"]);
        assert_eq!(state.stage(), StateStage::Disconnect);
        assert_eq!(state.activated_hosts().len(), 2);
        assert_eq!(state.host_results("one").map(|results| results.successful_operations), Some(1));
        assert_eq!(state.host_results("two").map(|results| results.operations), Some(1));
    }

    #[tokio::test]
    async fn runtime_executes_stateful_operations_through_the_public_deploy_boundary() {
        let inventory =
            Inventory::from_spec(InventorySpec { hosts: vec![HostSpec::local()], ..InventorySpec::default() }).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("stateful-file");
        let deploy = PreparedDeploy::prepare(&inventory, |host| {
            host.add_stateful(File, FileInput::new(path.to_string_lossy()), OperationOptions::default())?;
            Ok(())
        })
        .unwrap();
        let mut state = State::new(inventory.clone(), Config::default());
        let runtime = DeployRuntime::from_inventory(&inventory, Duration::from_secs(1)).unwrap();

        runtime.execute(&mut state, &deploy, ExecutionStrategy::Lockstep).await.unwrap();

        assert!(path.is_file());
    }
}
