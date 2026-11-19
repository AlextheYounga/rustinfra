//! Operation metadata, ordering, and execution coordination.

mod command_executor;
mod deploy;
mod order;
mod retry;
mod runtime;
mod scheduler;
pub mod server;

pub use command_executor::{CommandAttempt, CommandExecutionError, CommandExecutor};
pub use deploy::{DeployError, DeployHost, OperationOptions, PreparedDeploy, PreparedOperation};
pub use order::{CycleEdge, OperationGraph, OperationId, OperationMeta, OperationOrderError};
pub use retry::{
    OperationExecution, OperationExecutionOptions, OperationExecutionResult, OperationExecutor, RetryContext,
    RetryPredicate,
};
pub use runtime::{DeployRuntime, DeployRuntimeError};
pub use scheduler::{
    ExecutionPlan, ExecutionStrategy, OperationRunSpec, OperationRunner, OperationTaskError, Scheduler, SchedulerError,
};

use crate::api::Command;

/// A typed desired-state action that produces connector commands.
pub trait Operation {
    type Input;

    fn name(&self) -> &'static str;
    fn is_idempotent(&self) -> bool;
    fn commands(&self, input: &Self::Input) -> Vec<Command>;
}
