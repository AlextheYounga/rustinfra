use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use thiserror::Error;

use crate::api::{ConnectorKind, Inventory, InventoryError, State, StateError, StateStage};
use crate::connectors::{
    ConnectionManager, ConnectionManagerError, Connector, LocalConnector, SshConnectionOptions, SshConnector,
    SshOptionsError,
};

use super::{
    ComposeReporter, DeployPhase, DeployPreviewError, DeployReporter, DeploySummary, ExecutionStrategy, PreparedDeploy,
    Scheduler, SchedulerError,
};

/// Connectors and lifecycle coordination for executing prepared Rust deploys.
pub struct DeployRuntime {
    connectors: IndexMap<String, Arc<dyn Connector>>,
    reporter: Arc<dyn DeployReporter>,
}

impl DeployRuntime {
    #[must_use]
    pub fn new(connectors: IndexMap<String, Arc<dyn Connector>>) -> Self {
        Self { connectors, reporter: Arc::new(ComposeReporter::auto()) }
    }

    #[must_use]
    pub fn with_reporter(mut self, reporter: Arc<dyn DeployReporter>) -> Self {
        self.reporter = reporter;
        self
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
        let mut summary = None;
        let result = async {
            state.set_stage(StateStage::Connect)?;
            let host_count = state.inventory().len();
            self.reporter.phase_started(DeployPhase::Connect, host_count);
            let connection = manager.connect_all(state, None).await;
            if let Ok(report) = &connection {
                self.reporter.phase_progress(
                    DeployPhase::Connect,
                    host_count,
                    &format!("{} connected, {} failed", state.active_hosts().len(), report.failures.len()),
                );
            }
            self.reporter.phase_finished(DeployPhase::Connect);
            connection?;

            state.set_stage(StateStage::Prepare)?;
            let planned_hosts = deploy.plan().operations.iter().map(|operation| operation.hosts.len()).sum();
            self.reporter.phase_started(DeployPhase::Plan, planned_hosts);
            let preview =
                deploy.preview_reported(&self.connectors, state.config().parallel, self.reporter.as_ref()).await;
            self.reporter.phase_finished(DeployPhase::Plan);
            if let Ok(preview) = &preview {
                self.reporter.proposed_changes(preview);
            }
            let preview = preview?;

            let runner = Arc::new(deploy.runner(&self.connectors, Arc::clone(&self.reporter))?);
            state.set_stage(StateStage::Execute)?;
            self.reporter.phase_started(DeployPhase::Apply, planned_hosts);
            let execution = Scheduler::new(runner.clone()).execute(state, deploy.plan(), strategy).await;
            for (host, results) in runner.results().await {
                state.set_host_results(host, results);
            }
            let reports = runner.operation_reports().await;
            summary = Some(DeploySummary::from_reports(&preview, &reports));
            self.reporter.phase_finished(DeployPhase::Apply);
            execution?;
            Ok(())
        }
        .await;

        state.set_stage(StateStage::Disconnect)?;
        self.reporter.phase_started(DeployPhase::Disconnect, state.activated_hosts().len());
        let disconnect = manager.disconnect_all(state).await;
        self.reporter.phase_finished(DeployPhase::Disconnect);
        if let Some(summary) = &summary {
            self.reporter.deploy_finished(summary);
        }
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
    Preview(#[from] DeployPreviewError),
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
    use std::io::{self, Write};
    use std::sync::Arc;
    use std::sync::Mutex;

    use indexmap::IndexMap;

    use super::{DeployRuntime, Duration};
    use crate::api::{Config, ConnectorKind, Data, HostSpec, Inventory, InventorySpec, State, StateStage};
    use crate::connectors::{Connector, LocalConnector};
    use crate::operations::files::{File, FileInput};
    use crate::operations::server::{Shell, ShellCommands};
    use crate::operations::{
        DeployRuntimeError, ExecutionStrategy, NullReporter, OperationOptions, PreparedDeploy, TextReporter,
    };

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl SharedWriter {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().map_err(|_| io::Error::other("output lock poisoned"))?.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

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

        DeployRuntime::new(connectors)
            .with_reporter(Arc::new(NullReporter))
            .execute(&mut state, &deploy, ExecutionStrategy::Lockstep)
            .await
            .unwrap();

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
        let runtime = DeployRuntime::from_inventory(&inventory, Duration::from_secs(1))
            .unwrap()
            .with_reporter(Arc::new(NullReporter));

        runtime.execute(&mut state, &deploy, ExecutionStrategy::Lockstep).await.unwrap();

        assert!(path.is_file());
    }

    #[tokio::test]
    async fn runtime_reports_preview_live_output_per_host_results_and_final_summary() -> Result<(), DeployRuntimeError>
    {
        let inventory =
            Inventory::from_spec(InventorySpec { hosts: vec![HostSpec::local()], ..InventorySpec::default() }).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let existing = directory.path().join("existing");
        let missing = directory.path().join("missing");
        fs::write(&existing, "present").unwrap();
        let deploy = PreparedDeploy::prepare(&inventory, |host| {
            host.add_stateful(
                File,
                FileInput::new(existing.to_string_lossy()),
                OperationOptions { name: Some("keep existing".to_owned()), ..OperationOptions::default() },
            )?;
            host.add_stateful(
                File,
                FileInput::new(missing.to_string_lossy()),
                OperationOptions { name: Some("create missing".to_owned()), ..OperationOptions::default() },
            )?;
            host.add_with_options(
                Shell,
                ShellCommands::from("printf 'live output\\n'"),
                OperationOptions { name: Some("show output".to_owned()), ..OperationOptions::default() },
            )?;
            host.add_with_options(
                Shell,
                ShellCommands::from("printf 'broken\\n' >&2; exit 2"),
                OperationOptions { name: Some("fail visibly".to_owned()), ..OperationOptions::default() },
            )?;
            Ok(())
        })
        .unwrap();
        let writer = SharedWriter::default();
        let reporter = Arc::new(TextReporter::new(writer.clone()));
        let runtime = DeployRuntime::from_inventory(&inventory, Duration::from_secs(1))?.with_reporter(reporter);
        let mut state = State::new(inventory, Config::default());

        let result = runtime.execute(&mut state, &deploy, ExecutionStrategy::Lockstep).await;

        assert!(result.is_err());
        assert!(missing.is_file());
        let output = writer.text();
        assert!(output.contains("rustinfra: proposed changes"));
        assert!(output.contains("keep existing                    no      @local"));
        assert!(output.contains("create missing                   yes     @local"));
        assert!(output.contains("[@local][keep existing] No changes"));
        assert!(output.contains("[@local][create missing] Success"));
        assert!(output.contains("[@local][show output][out] live output"));
        assert!(output.contains("[@local][show output] Success"));
        assert!(output.contains("[@local][fail visibly][err] broken"));
        assert!(output.contains("[@local][fail visibly] Failed: shell command exited with Some(2): broken"));
        assert!(output.contains("rustinfra: operation summary"));
        Ok::<(), DeployRuntimeError>(())
    }
}
