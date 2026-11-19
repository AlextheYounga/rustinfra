use std::collections::HashMap;
use std::panic::Location;
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use thiserror::Error;

use crate::api::{Host, Inventory};
use crate::connectors::Connector;
use crate::facts::{FactOptions, Phase};

use super::{
    ExecutionPlan, Operation, OperationExecution, OperationExecutionOptions, OperationGraph, OperationId,
    OperationMeta, OperationOrderError, OperationRunSpec, PreparedOperationRunner, StatefulOperation,
};

/// Execution and display controls attached to one operation call.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub name: Option<String>,
    pub serial: bool,
    pub parallel: usize,
    pub run_once: bool,
    pub execution: OperationExecutionOptions,
}

#[derive(Clone)]
struct SharedOperation {
    operation_name: &'static str,
    display_name: String,
    idempotent: bool,
    serial: bool,
    parallel: usize,
    hosts: IndexSet<String>,
}

/// A deploy after its Rust definition has been evaluated for every host.
pub struct PreparedDeploy {
    plan: ExecutionPlan,
    operations: IndexMap<OperationId, SharedOperation>,
    executions: IndexMap<(String, OperationId), OperationExecution>,
}

impl PreparedDeploy {
    /// Evaluates a Rust deploy function once per inventory host and solves its global operation order.
    pub fn prepare(
        inventory: &Inventory,
        mut define: impl FnMut(&mut DeployHost<'_>) -> Result<(), DeployError>,
    ) -> Result<Self, DeployError> {
        let mut builder = DeployBuilder::default();
        for host in inventory.hosts() {
            let mut deploy_host = DeployHost::new(host, &mut builder);
            define(&mut deploy_host)?;
            deploy_host.finish();
        }
        builder.finish(inventory)
    }

    #[must_use]
    pub fn plan(&self) -> &ExecutionPlan {
        &self.plan
    }

    #[must_use]
    pub fn operation<'a>(&'a self, id: &'a OperationId) -> Option<PreparedOperation<'a>> {
        self.operations.get(id).map(|operation| PreparedOperation { id, operation })
    }

    pub(crate) fn runner(
        &self,
        connectors: &IndexMap<String, Arc<dyn Connector>>,
    ) -> Result<PreparedOperationRunner, DeployError> {
        for host in &self.plan.hosts {
            if !connectors.contains_key(host) {
                return Err(DeployError::MissingConnector(host.clone()));
            }
        }
        Ok(PreparedOperationRunner::new(connectors.clone(), self.executions.clone()))
    }
}

/// Read-only metadata for a prepared operation.
#[derive(Clone, Copy)]
pub struct PreparedOperation<'a> {
    id: &'a OperationId,
    operation: &'a SharedOperation,
}

impl PreparedOperation<'_> {
    #[must_use]
    pub fn id(&self) -> &OperationId {
        self.id
    }

    #[must_use]
    pub fn operation_name(&self) -> &'static str {
        self.operation.operation_name
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.operation.display_name
    }

    #[must_use]
    pub fn is_idempotent(&self) -> bool {
        self.operation.idempotent
    }
}

/// Host-local context used while evaluating a Rust deploy definition.
pub struct DeployHost<'a> {
    host: &'a Host,
    builder: &'a mut DeployBuilder,
    path: Vec<OperationId>,
    location_counts: HashMap<String, usize>,
}

impl<'a> DeployHost<'a> {
    fn new(host: &'a Host, builder: &'a mut DeployBuilder) -> Self {
        Self { host, builder, path: Vec::new(), location_counts: HashMap::new() }
    }

    #[must_use]
    pub fn host(&self) -> &Host {
        self.host
    }

    /// Adds one typed operation for this host.
    #[track_caller]
    pub fn add<O>(&mut self, operation: O, input: O::Input) -> Result<OperationId, DeployError>
    where
        O: Operation + Send + Sync + 'static,
        O::Input: Send + Sync + 'static,
    {
        self.add_with_options(operation, input, OperationOptions::default())
    }

    /// Adds one typed operation with pyinfra-style global execution controls.
    #[track_caller]
    pub fn add_with_options<O>(
        &mut self,
        operation: O,
        input: O::Input,
        options: OperationOptions,
    ) -> Result<OperationId, DeployError>
    where
        O: Operation + Send + Sync + 'static,
        O::Input: Send + Sync + 'static,
    {
        let location = Location::caller();
        let location_key = format!("{}:{}:{}", location.file(), location.line(), location.column());
        let occurrence = self.location_counts.entry(location_key.clone()).or_default();
        let id = OperationId::new(format!("{location_key}:{occurrence}"));
        *occurrence += 1;

        let display_name = options.name.clone().unwrap_or_else(|| operation.name().to_owned());
        let shared = SharedOperation {
            operation_name: operation.name(),
            display_name,
            idempotent: operation.is_idempotent(),
            serial: options.serial,
            parallel: options.parallel,
            hosts: IndexSet::new(),
        };
        if options.run_once && self.builder.run_once.contains(&id) {
            return Ok(id);
        }
        self.builder.register(id.clone(), shared)?;
        if options.run_once {
            self.builder.run_once.insert(id.clone());
        }

        let operation = Arc::new(operation);
        let execution = OperationExecution::new(
            self.host.name(),
            operation.name(),
            move || operation.commands(&input),
            options.execution,
        );
        self.builder.executions.insert((self.host.name().to_owned(), id.clone()), execution);
        if let Some(shared) = self.builder.operations.get_mut(&id) {
            shared.hosts.insert(self.host.name().to_owned());
        }
        self.path.push(id.clone());
        Ok(id)
    }

    /// Adds a desired-state operation that gathers fresh facts on every execution attempt.
    #[track_caller]
    pub fn add_stateful<O>(
        &mut self,
        operation: O,
        input: O::Input,
        options: OperationOptions,
    ) -> Result<OperationId, DeployError>
    where
        O: StatefulOperation + 'static,
        O::Input: 'static,
    {
        let location = Location::caller();
        let location_key = format!("{}:{}:{}", location.file(), location.line(), location.column());
        let occurrence = self.location_counts.entry(location_key.clone()).or_default();
        let id = OperationId::new(format!("{location_key}:{occurrence}"));
        *occurrence += 1;

        let display_name = options.name.clone().unwrap_or_else(|| operation.name().to_owned());
        let shared = SharedOperation {
            operation_name: operation.name(),
            display_name,
            idempotent: operation.is_idempotent(),
            serial: options.serial,
            parallel: options.parallel,
            hosts: IndexSet::new(),
        };
        if options.run_once && self.builder.run_once.contains(&id) {
            return Ok(id);
        }
        self.builder.register(id.clone(), shared)?;
        if options.run_once {
            self.builder.run_once.insert(id.clone());
        }

        let operation = Arc::new(operation);
        let input = Arc::new(input);
        let fact_options = FactOptions {
            run: options.execution.run.clone(),
            ignore_errors: options.execution.ignore_errors,
            phase: Phase::Execute,
        };
        let execution = OperationExecution::new_async(
            self.host.name(),
            operation.name(),
            move |connector| {
                let operation = Arc::clone(&operation);
                let input = Arc::clone(&input);
                let fact_options = fact_options.clone();
                Box::pin(async move {
                    operation
                        .commands(input.as_ref(), connector, &fact_options)
                        .await
                        .map_err(|error| super::CommandExecutionError::Generation { message: error.to_string() })
                })
            },
            options.execution,
        );
        self.builder.executions.insert((self.host.name().to_owned(), id.clone()), execution);
        if let Some(shared) = self.builder.operations.get_mut(&id) {
            shared.hosts.insert(self.host.name().to_owned());
        }
        self.path.push(id.clone());
        Ok(id)
    }

    fn finish(self) {
        self.builder.graph.set_host_path(self.host.name(), self.path);
    }
}

#[derive(Default)]
struct DeployBuilder {
    graph: OperationGraph,
    operations: IndexMap<OperationId, SharedOperation>,
    executions: IndexMap<(String, OperationId), OperationExecution>,
    run_once: IndexSet<OperationId>,
}

impl DeployBuilder {
    fn register(&mut self, id: OperationId, operation: SharedOperation) -> Result<(), DeployError> {
        if let Some(existing) = self.operations.get(&id) {
            if existing.operation_name != operation.operation_name
                || existing.display_name != operation.display_name
                || existing.serial != operation.serial
                || existing.parallel != operation.parallel
            {
                return Err(DeployError::InconsistentOperation { id });
            }
            return Ok(());
        }

        let mut metadata = OperationMeta::new([self.operations.len()]);
        metadata.names.insert(operation.display_name.clone());
        self.graph.insert(id.clone(), metadata);
        self.operations.insert(id, operation);
        Ok(())
    }

    fn finish(self, inventory: &Inventory) -> Result<PreparedDeploy, DeployError> {
        let ordered = self.graph.order()?;
        let operations = ordered
            .iter()
            .filter_map(|id| {
                self.operations.get(id).map(|operation| OperationRunSpec {
                    id: id.clone(),
                    hosts: operation.hosts.clone(),
                    serial: operation.serial,
                    parallel: operation.parallel,
                })
            })
            .collect();
        let hosts = inventory.hosts().map(|host| host.name().to_owned()).collect();
        Ok(PreparedDeploy {
            plan: ExecutionPlan { hosts, operations },
            operations: self.operations,
            executions: self.executions,
        })
    }
}

/// Invalid or inconsistent Rust deploy definition.
#[derive(Debug, Error)]
pub enum DeployError {
    #[error(transparent)]
    OperationOrder(#[from] OperationOrderError),
    #[error("operation {id:?} is configured differently across hosts")]
    InconsistentOperation { id: OperationId },
    #[error("host {0} has no connector")]
    MissingConnector(String),
}

#[cfg(test)]
mod tests {
    use super::{OperationOptions, PreparedDeploy};
    use crate::api::{HostSpec, Inventory, InventorySpec};
    use crate::operations::server::{Shell, ShellCommands};

    fn inventory() -> Inventory {
        Inventory::from_spec(InventorySpec {
            hosts: vec![HostSpec::ssh("one"), HostSpec::ssh("two")],
            ..InventorySpec::default()
        })
        .unwrap()
    }

    #[test]
    fn preparation_merges_matching_calls_and_keeps_host_specific_paths() {
        let deploy = PreparedDeploy::prepare(&inventory(), |host| {
            let command = format!("echo {}", host.host().name());
            host.add(Shell, ShellCommands::from(command))?;
            if host.host().name() == "two" {
                host.add(Shell, ShellCommands::from("echo only-two"))?;
            }
            Ok(())
        })
        .unwrap();

        assert_eq!(deploy.plan.operations.len(), 2);
        assert_eq!(deploy.plan.operations[0].hosts.len(), 2);
        assert_eq!(deploy.plan.operations[1].hosts.iter().map(String::as_str).collect::<Vec<_>>(), vec!["two"]);
    }

    #[test]
    fn run_once_is_attached_only_to_the_first_host_that_reaches_the_call() {
        let deploy = PreparedDeploy::prepare(&inventory(), |host| {
            host.add_with_options(
                Shell,
                ShellCommands::from("echo once"),
                OperationOptions { run_once: true, ..OperationOptions::default() },
            )?;
            Ok(())
        })
        .unwrap();

        assert_eq!(deploy.plan.operations.len(), 1);
        assert_eq!(deploy.plan.operations[0].hosts.iter().map(String::as_str).collect::<Vec<_>>(), vec!["one"]);
    }
}
