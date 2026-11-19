use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::time;

use crate::api::Command;
use crate::connectors::{CommandOutput, Connector, RunOptions};

use super::command_executor::AttemptOptions;
use super::{CommandExecutionError, CommandExecutor};

/// Data supplied to an operation's `_retry_until` equivalent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryContext {
    pub stdout_lines: Vec<String>,
    pub stderr_lines: Vec<String>,
    pub commands: Vec<String>,
    pub executed_commands: usize,
    pub host: String,
    pub operation: String,
}

/// A retry condition. Returning an error stops conditional retries without failing the operation.
pub type RetryPredicate = Arc<dyn Fn(&RetryContext) -> Result<bool, String> + Send + Sync>;

/// Controls retries and error handling for one host operation.
#[derive(Clone)]
pub struct OperationExecutionOptions {
    pub run: RunOptions,
    pub ignore_errors: bool,
    pub continue_on_error: bool,
    pub retries: usize,
    pub retry_delay: Duration,
    pub retry_until: Option<RetryPredicate>,
}

impl Default for OperationExecutionOptions {
    fn default() -> Self {
        Self {
            run: RunOptions::default(),
            ignore_errors: false,
            continue_on_error: false,
            retries: 0,
            retry_delay: Duration::from_secs(5),
            retry_until: None,
        }
    }
}

/// A host-specific operation whose commands are regenerated for every attempt.
#[derive(Clone)]
pub struct OperationExecution {
    host: String,
    operation: String,
    generate_commands: Arc<CommandGenerator>,
    options: OperationExecutionOptions,
}

type CommandGenerationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Command>, CommandExecutionError>> + Send + 'a>>;
type CommandGenerator = dyn for<'a> Fn(&'a dyn Connector) -> CommandGenerationFuture<'a> + Send + Sync;

impl OperationExecution {
    #[must_use]
    pub fn new(
        host: impl Into<String>,
        operation: impl Into<String>,
        generate_commands: impl Fn() -> Vec<Command> + Send + Sync + 'static,
        options: OperationExecutionOptions,
    ) -> Self {
        Self::new_async(
            host,
            operation,
            move |_| {
                let commands = generate_commands();
                Box::pin(async move { Ok(commands) })
            },
            options,
        )
    }

    #[must_use]
    pub fn new_async(
        host: impl Into<String>,
        operation: impl Into<String>,
        generate_commands: impl for<'a> Fn(&'a dyn Connector) -> CommandGenerationFuture<'a> + Send + Sync + 'static,
        options: OperationExecutionOptions,
    ) -> Self {
        Self { host: host.into(), operation: operation.into(), generate_commands: Arc::new(generate_commands), options }
    }
}

/// Final-attempt result and pyinfra-compatible retry metadata.
#[derive(Debug)]
pub struct OperationExecutionResult {
    pub success: bool,
    pub return_status: bool,
    pub ignored_error: bool,
    pub partial: bool,
    pub retry_attempts: usize,
    pub max_retries: usize,
    pub executed_commands: usize,
    pub commands: Vec<Command>,
    pub output: CommandOutput,
    pub error: Option<CommandExecutionError>,
}

impl OperationExecutionResult {
    #[must_use]
    pub fn was_retried(&self) -> bool {
        self.retry_attempts > 0
    }

    #[must_use]
    pub fn retry_succeeded(&self) -> bool {
        self.was_retried() && self.success
    }
}

/// Executes a host operation with pyinfra's retry loop.
#[derive(Clone, Copy, Debug, Default)]
pub struct OperationExecutor;

impl OperationExecutor {
    pub async fn execute(&self, connector: &dyn Connector, execution: &OperationExecution) -> OperationExecutionResult {
        let mut retry_attempts = 0;

        loop {
            let commands = match (execution.generate_commands)(connector).await {
                Ok(commands) => commands,
                Err(error) => {
                    return OperationExecutionResult {
                        success: false,
                        return_status: execution.options.ignore_errors,
                        ignored_error: execution.options.ignore_errors,
                        partial: false,
                        retry_attempts,
                        max_retries: execution.options.retries,
                        executed_commands: 0,
                        commands: Vec::new(),
                        output: CommandOutput::default(),
                        error: Some(error),
                    };
                }
            };
            let attempt = CommandExecutor
                .execute_attempt(
                    connector,
                    &commands,
                    AttemptOptions {
                        run: &execution.options.run,
                        continue_on_error: execution.options.continue_on_error,
                    },
                )
                .await;
            let should_retry =
                retry_attempts < execution.options.retries && should_retry(&attempt, &commands, execution);

            if should_retry {
                retry_attempts += 1;
                time::sleep(execution.options.retry_delay).await;
                continue;
            }

            let success = !attempt.did_error;
            return OperationExecutionResult {
                success,
                return_status: success || execution.options.ignore_errors,
                ignored_error: attempt.did_error && execution.options.ignore_errors,
                partial: attempt.did_error && attempt.executed_commands > 0,
                retry_attempts,
                max_retries: execution.options.retries,
                executed_commands: attempt.executed_commands,
                commands,
                output: attempt.output,
                error: attempt.error,
            };
        }
    }
}

fn should_retry(attempt: &super::CommandAttempt, commands: &[Command], execution: &OperationExecution) -> bool {
    if attempt.did_error {
        return true;
    }
    let Some(predicate) = &execution.options.retry_until else {
        return false;
    };
    let context = RetryContext {
        stdout_lines: attempt.output.stdout_lines().into_iter().map(str::to_owned).collect(),
        stderr_lines: attempt.output.stderr_lines().into_iter().map(str::to_owned).collect(),
        commands: commands.iter().map(command_description).collect(),
        executed_commands: attempt.executed_commands,
        host: execution.host.clone(),
        operation: execution.operation.clone(),
    };
    predicate(&context).unwrap_or(false)
}

fn command_description(command: &Command) -> String {
    match command {
        Command::Shell(command) => command.masked_value(),
        Command::Upload(command) => format!("FileUploadCommand({}, {})", command.source.display(), command.destination),
        Command::Download(command) => {
            format!("FileDownloadCommand({}, {})", command.source, command.destination.display())
        }
        Command::Rsync(command) => format!("RsyncCommand({}, {})", command.source.display(), command.destination),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{OperationExecution, OperationExecutionOptions, OperationExecutor, RetryContext};
    use crate::api::{Command, StringCommand};
    use crate::connectors::LocalConnector;

    #[tokio::test]
    async fn failed_attempt_regenerates_commands_and_successful_retry_is_reported() {
        let generation = Arc::new(AtomicUsize::new(0));
        let generator_count = Arc::clone(&generation);
        let options = OperationExecutionOptions {
            retries: 2,
            retry_delay: Duration::ZERO,
            ..OperationExecutionOptions::default()
        };
        let execution = OperationExecution::new(
            "host",
            "server.shell",
            move || {
                let attempt = generator_count.fetch_add(1, Ordering::SeqCst);
                let command = if attempt == 0 { "exit 1" } else { "printf success" };
                vec![Command::Shell(StringCommand::new([command]))]
            },
            options,
        );

        let result = OperationExecutor.execute(&LocalConnector, &execution).await;

        assert!(result.success);
        assert!(result.return_status);
        assert_eq!(result.retry_attempts, 1);
        assert_eq!(result.max_retries, 2);
        assert!(result.was_retried());
        assert!(result.retry_succeeded());
        assert_eq!(generation.load(Ordering::SeqCst), 2);
        assert_eq!(result.output.stdout(), "success");
    }

    #[tokio::test]
    async fn retry_until_receives_each_successful_attempt_and_stops_on_ready_output() {
        let generation = Arc::new(AtomicUsize::new(0));
        let generator_count = Arc::clone(&generation);
        let contexts = Arc::new(Mutex::new(Vec::<RetryContext>::new()));
        let predicate_contexts = Arc::clone(&contexts);
        let options = OperationExecutionOptions {
            retries: 3,
            retry_delay: Duration::ZERO,
            retry_until: Some(Arc::new(move |context| {
                predicate_contexts.lock().unwrap().push(context.clone());
                Ok(!context.stdout_lines.iter().any(|line| line.contains("READY")))
            })),
            ..OperationExecutionOptions::default()
        };
        let execution = OperationExecution::new(
            "host",
            "server.shell",
            move || {
                let attempt = generator_count.fetch_add(1, Ordering::SeqCst);
                let command = if attempt < 2 { "printf starting" } else { "printf READY" };
                vec![Command::Shell(StringCommand::new([command]))]
            },
            options,
        );

        let result = OperationExecutor.execute(&LocalConnector, &execution).await;

        assert!(result.success);
        assert_eq!(result.retry_attempts, 2);
        assert_eq!(contexts.lock().unwrap().len(), 3);
        assert_eq!(result.output.stdout(), "READY");
    }

    #[tokio::test]
    async fn continue_and_ignore_errors_run_later_commands_but_retain_failure() {
        let options = OperationExecutionOptions {
            ignore_errors: true,
            continue_on_error: true,
            ..OperationExecutionOptions::default()
        };
        let execution = OperationExecution::new(
            "host",
            "server.shell",
            || {
                vec![
                    Command::Shell(StringCommand::new(["printf failed >&2; exit 1"])),
                    Command::Shell(StringCommand::new(["printf continued"])),
                ]
            },
            options,
        );

        let result = OperationExecutor.execute(&LocalConnector, &execution).await;

        assert!(!result.success);
        assert!(result.return_status);
        assert!(result.ignored_error);
        assert!(result.partial);
        assert_eq!(result.executed_commands, 1);
        assert_eq!(result.output.stdout(), "continued");
        assert_eq!(result.output.stderr(), "failed");
    }

    #[tokio::test]
    async fn retry_predicate_error_does_not_fail_or_retry_the_operation() {
        let options = OperationExecutionOptions {
            retries: 2,
            retry_delay: Duration::ZERO,
            retry_until: Some(Arc::new(|_| Err("predicate failed".to_owned()))),
            ..OperationExecutionOptions::default()
        };
        let execution = OperationExecution::new(
            "host",
            "server.shell",
            || vec![Command::Shell(StringCommand::new(["printf success"]))],
            options,
        );

        let result = OperationExecutor.execute(&LocalConnector, &execution).await;

        assert!(result.success);
        assert_eq!(result.retry_attempts, 0);
        assert_eq!(result.output.stdout(), "success");
    }
}
