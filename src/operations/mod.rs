//! Operation metadata, ordering, and execution coordination.

use std::error::Error;
use std::future::Future;
use std::pin::Pin;

pub mod apt;
mod command_executor;
pub mod crontab;
mod deploy;
pub mod dnf;
pub mod files;
pub mod git;
mod order;
mod prepared_runner;
mod retry;
mod runtime;
mod scheduler;
pub mod server;
pub mod systemd;

pub use command_executor::{CommandAttempt, CommandExecutionError, CommandExecutor};
pub use deploy::{DeployError, DeployHost, OperationOptions, PreparedDeploy, PreparedOperation};
pub use order::{CycleEdge, OperationGraph, OperationId, OperationMeta, OperationOrderError};
pub(crate) use prepared_runner::PreparedOperationRunner;
pub use retry::{
    OperationExecution, OperationExecutionOptions, OperationExecutionResult, OperationExecutor, RetryContext,
    RetryPredicate,
};
pub use runtime::{DeployRuntime, DeployRuntimeError};
pub use scheduler::{
    ExecutionPlan, ExecutionStrategy, OperationRunSpec, OperationRunner, OperationTaskError, Scheduler, SchedulerError,
};

use crate::api::Command;
use crate::connectors::Connector;
use crate::facts::FactOptions;

/// A typed desired-state action that produces connector commands.
pub trait Operation {
    type Input;

    fn name(&self) -> &'static str;
    fn is_idempotent(&self) -> bool;
    fn commands(&self, input: &Self::Input) -> Vec<Command>;
}

/// Asynchronous command generation for an operation that reads current host state.
pub type StatefulOperationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Command>, Box<dyn Error + Send + Sync>>> + Send + 'a>>;

/// A desired-state operation that re-reads facts whenever its commands are generated.
pub trait StatefulOperation: Send + Sync {
    type Input: Send + Sync;

    fn name(&self) -> &'static str;
    fn is_idempotent(&self) -> bool;
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a>;
}
