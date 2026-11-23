use std::io::Error;
use std::time::Duration;

use thiserror::Error;
use tokio::{fs, time};

use crate::api::{Command, ContentUploadCommand, ReconnectCommand};
use crate::connectors::{CommandOutput, Connector, ConnectorError, FileTransferOptions, RunOptions};
use crate::facts::server::{KernelInput, Uptime};
use crate::facts::{FactExecutor, FactOptions, Phase};

/// Failure while executing one command in an operation's ordered command list.
#[derive(Debug, Error)]
pub enum CommandExecutionError {
    #[error("could not generate operation commands: {message}")]
    Generation { message: String },
    #[error(transparent)]
    Connector(#[from] ConnectorError),
    #[error("could not prepare generated upload content: {0}")]
    LocalIo(#[from] Error),
    #[error("shell command exited with {exit_code:?}: {stderr}")]
    ShellFailed { exit_code: Option<i32>, stderr: String },
    #[error("local function failed: {message}")]
    FunctionFailed { message: String },
    #[error("server did not reboot in time (reboot_timeout={timeout_seconds}s)")]
    RebootTimeout { timeout_seconds: u64 },
    #[error("reboot reconnect interval must be greater than zero")]
    InvalidReconnectInterval,
}

/// Result of one pass through an operation's generated commands.
#[derive(Debug)]
pub struct CommandAttempt {
    pub did_error: bool,
    pub executed_commands: usize,
    pub output: CommandOutput,
    pub error: Option<CommandExecutionError>,
}

#[derive(Clone, Copy)]
pub(crate) struct AttemptOptions<'a> {
    pub run: &'a RunOptions,
    pub continue_on_error: bool,
}

/// Executes each command yielded by one operation in order.
#[derive(Clone, Copy, Debug, Default)]
pub struct CommandExecutor;

impl CommandExecutor {
    pub async fn execute(
        &self,
        connector: &dyn Connector,
        commands: &[Command],
        options: &RunOptions,
    ) -> Result<CommandOutput, CommandExecutionError> {
        let attempt =
            self.execute_attempt(connector, commands, AttemptOptions { run: options, continue_on_error: false }).await;
        match attempt.error {
            Some(error) => Err(error),
            None => Ok(attempt.output),
        }
    }

    /// Executes one attempt, optionally continuing after failed commands.
    pub(crate) async fn execute_attempt(
        &self,
        connector: &dyn Connector,
        commands: &[Command],
        options: AttemptOptions<'_>,
    ) -> CommandAttempt {
        let mut attempt =
            CommandAttempt { did_error: false, executed_commands: 0, output: CommandOutput::default(), error: None };
        for command in commands {
            let result = match command {
                Command::Shell(command) => match connector.run_shell_command(command.clone(), options.run).await {
                    Ok(result) if result.success => {
                        attempt.output.extend(result.output);
                        Ok(())
                    }
                    Ok(result) => {
                        let error = CommandExecutionError::ShellFailed {
                            exit_code: result.exit_code,
                            stderr: result.output.stderr(),
                        };
                        attempt.output.extend(result.output);
                        Err(error)
                    }
                    Err(error) => Err(error.into()),
                },
                Command::Upload(command) => connector
                    .put_file(
                        &command.source,
                        &command.destination,
                        FileTransferOptions {
                            run: options.run,
                            remote_temporary_filename: command.remote_temporary_filename.as_deref(),
                        },
                    )
                    .await
                    .map_err(CommandExecutionError::from),
                Command::UploadContent(command) => self.upload_content(connector, command, options.run).await,
                Command::Download(command) => connector
                    .get_file(
                        &command.source,
                        &command.destination,
                        FileTransferOptions {
                            run: options.run,
                            remote_temporary_filename: command.remote_temporary_filename.as_deref(),
                        },
                    )
                    .await
                    .map_err(CommandExecutionError::from),
                Command::Rsync(command) => {
                    connector.rsync(command, options.run).await.map_err(CommandExecutionError::from)
                }
                Command::Function(command) => {
                    command.execute().map_err(|message| CommandExecutionError::FunctionFailed { message })
                }
                Command::Reconnect(command) => self.reconnect(connector, command, options.run).await,
            };

            match result {
                Ok(()) => attempt.executed_commands += 1,
                Err(error) => {
                    attempt.did_error = true;
                    attempt.error = Some(error);
                    if !options.continue_on_error {
                        break;
                    }
                }
            }
        }
        attempt
    }

    async fn reconnect(
        &self,
        connector: &dyn Connector,
        command: &ReconnectCommand,
        options: &RunOptions,
    ) -> Result<(), CommandExecutionError> {
        if command.interval_seconds == 0 {
            return Err(CommandExecutionError::InvalidReconnectInterval);
        }
        time::sleep(Duration::from_secs(command.delay_seconds)).await;
        let _ = connector.disconnect().await;
        let interval = Duration::from_secs(command.interval_seconds);
        let max_retries = rounded_division(command.timeout_seconds, command.interval_seconds);
        let fact_options = FactOptions { run: options.clone(), ignore_errors: false, phase: Phase::Execute };
        let uptime_input = KernelInput { kernel: command.kernel.clone() };
        for retry in 0..=max_retries.saturating_add(1) {
            if connector.connect().await.is_ok()
                && let Ok(post_uptime) = FactExecutor::new(connector, &fact_options).get(&Uptime, &uptime_input).await
                && reboot_confirmed(command.pre_reboot_uptime, post_uptime, command.delay_seconds)
            {
                return Ok(());
            }
            if retry <= max_retries {
                time::sleep(interval).await;
            }
        }
        Err(CommandExecutionError::RebootTimeout { timeout_seconds: command.timeout_seconds })
    }

    async fn upload_content(
        &self,
        connector: &dyn Connector,
        command: &ContentUploadCommand,
        options: &RunOptions,
    ) -> Result<(), CommandExecutionError> {
        let temporary = tempfile::NamedTempFile::new()?;
        fs::write(temporary.path(), &command.content).await?;
        connector
            .put_file(
                temporary.path(),
                &command.destination,
                FileTransferOptions {
                    run: options,
                    remote_temporary_filename: command.remote_temporary_filename.as_deref(),
                },
            )
            .await
            .map_err(CommandExecutionError::from)
    }
}

fn reboot_confirmed(pre_uptime: u64, post_uptime: u64, delay_seconds: u64) -> bool {
    post_uptime < pre_uptime.saturating_add(delay_seconds)
}

fn rounded_division(numerator: u64, denominator: u64) -> u64 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let complement = denominator - remainder;
    if remainder > complement || remainder == complement && quotient % 2 == 1 {
        quotient.saturating_add(1)
    } else {
        quotient
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandExecutionError, CommandExecutor};
    use super::{reboot_confirmed, rounded_division};
    use crate::api::{Command, StringCommand};
    use crate::connectors::{LocalConnector, RunOptions};

    #[tokio::test]
    async fn shell_commands_execute_in_order_and_combine_output() {
        let commands = [
            Command::Shell(StringCommand::new(["printf 'first\\n'"])),
            Command::Shell(StringCommand::new(["printf 'second\\n'"])),
        ];

        let output = CommandExecutor.execute(&LocalConnector, &commands, &RunOptions::default()).await.unwrap();

        assert_eq!(output.stdout(), "first\nsecond");
    }

    #[tokio::test]
    async fn shell_failure_stops_later_commands() {
        let commands = [
            Command::Shell(StringCommand::new(["printf failed >&2; exit 4"])),
            Command::Shell(StringCommand::new(["printf should-not-run"])),
        ];

        let error = CommandExecutor.execute(&LocalConnector, &commands, &RunOptions::default()).await.unwrap_err();

        assert!(matches!(error, CommandExecutionError::ShellFailed { exit_code: Some(4), .. }));
    }

    #[test]
    fn reboot_confirmation_requires_uptime_to_reset() {
        assert!(reboot_confirmed(5_000, 8, 10));
        assert!(!reboot_confirmed(5_000, 5_010, 10));
    }

    #[test]
    fn reconnect_retry_count_uses_python_style_rounding() {
        assert_eq!(rounded_division(5, 2), 2);
        assert_eq!(rounded_division(7, 2), 4);
        assert_eq!(rounded_division(8, 3), 3);
    }
}
