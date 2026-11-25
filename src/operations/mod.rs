//! Operation metadata, ordering, and execution coordination.

use std::error::Error;
use std::future::Future;
use std::pin::Pin;

pub mod apk;
pub mod apt;
pub mod brew;
pub mod bsdinit;
pub mod cargo;
pub mod choco;
mod command_executor;
pub mod crontab;
mod database;
mod deploy;
pub mod dnf;
pub mod docker;
pub mod files;
pub mod flatpak;
pub mod gem;
pub mod git;
pub mod go;
pub mod gpg;
pub mod iptables;
pub mod launchd;
pub mod lxd;
pub mod mysql;
pub mod npm;
pub mod openrc;
pub mod opkg;
mod order;
mod packages;
pub mod pacman;
pub mod pip;
pub mod pipx;
pub mod pkg;
pub mod pkgin;
pub mod pnpm;
pub mod postgres;
pub mod postgresql;
mod prepared_runner;
pub mod puppet;
pub mod python;
mod report;
mod retry;
mod rpm_manager;
pub mod runit;
mod runtime;
mod scheduler;
pub mod selinux;
pub mod server;
mod service;
pub mod snap;
pub mod ssh;
pub mod supervisorctl;
pub mod systemd;
pub mod sysvinit;
pub mod upstart;
pub mod uv;
pub mod vzctl;
pub mod xbps;
pub mod yum;
pub mod zfs;
pub mod zypper;

pub use command_executor::{CommandAttempt, CommandExecutionError, CommandExecutor};
pub use deploy::{DeployError, DeployHost, DeployPreviewError, OperationOptions, PreparedDeploy, PreparedOperation};
pub use order::{CycleEdge, OperationGraph, OperationId, OperationMeta, OperationOrderError};
pub(crate) use prepared_runner::PreparedOperationRunner;
pub use report::{
    ComposeReporter, DeployPhase, DeployPreview, DeployReporter, DeploySummary, HostOperationReport,
    HostOperationStatus, NullReporter, OperationPreview, OperationSummary, TextReporter,
};
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
