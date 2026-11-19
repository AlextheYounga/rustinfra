use thiserror::Error;

use crate::api::Command;
use crate::connectors::{CommandOutput, Connector, ConnectorError, FileTransferOptions, RunOptions};

/// Failure while executing one command in an operation's ordered command list.
#[derive(Debug, Error)]
pub enum CommandExecutionError {
    #[error("could not generate operation commands: {message}")]
    Generation { message: String },
    #[error(transparent)]
    Connector(#[from] ConnectorError),
    #[error("shell command exited with {exit_code:?}: {stderr}")]
    ShellFailed { exit_code: Option<i32>, stderr: String },
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
}

#[cfg(test)]
mod tests {
    use super::{CommandExecutionError, CommandExecutor};
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
}
