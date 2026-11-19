//! Operation metadata, ordering, and execution coordination.

mod command_executor;
mod order;
mod scheduler;
pub mod server;

pub use command_executor::{CommandExecutionError, CommandExecutor};
pub use order::{CycleEdge, OperationGraph, OperationId, OperationMeta, OperationOrderError};
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
