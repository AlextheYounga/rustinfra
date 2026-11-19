use std::sync::Arc;

use indexmap::IndexMap;
use thiserror::Error;

use crate::api::{State, StateError, StateStage};
use crate::connectors::{ConnectionManager, ConnectionManagerError, Connector};

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
            Scheduler::new(runner).execute(state, deploy.plan(), strategy).await?;
            Ok(())
        }
        .await;

        state.set_stage(StateStage::Disconnect)?;
        let disconnect = manager.disconnect_all(state).await;
        match (result, disconnect) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(DeployRuntimeError::Connection(error)),
            (Err(error), Err(disconnect)) => {
                Err(DeployRuntimeError::RunAndDisconnect { run: error.to_string(), disconnect })
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
    Connection(#[from] ConnectionManagerError),
    #[error(transparent)]
    Deploy(#[from] super::DeployError),
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
    #[error("deploy failed ({run}) and disconnect also failed: {disconnect}")]
    RunAndDisconnect { run: String, disconnect: ConnectionManagerError },
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use indexmap::IndexMap;

    use super::DeployRuntime;
    use crate::api::{Config, HostSpec, Inventory, InventorySpec, State, StateStage};
    use crate::connectors::{Connector, LocalConnector};
    use crate::operations::server::{Shell, ShellCommands};
    use crate::operations::{ExecutionStrategy, PreparedDeploy};

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
    }
}
