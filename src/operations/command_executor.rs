use thiserror::Error;

use crate::api::Command;
use crate::connectors::{CommandOutput, Connector, ConnectorError, RunOptions};

/// Failure while executing one command in an operation's ordered command list.
#[derive(Debug, Error)]
pub enum CommandExecutionError {
    #[error(transparent)]
    Connector(#[from] ConnectorError),
    #[error("shell command exited with {exit_code:?}: {stderr}")]
    ShellFailed { exit_code: Option<i32>, stderr: String },
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
        let mut output = CommandOutput::default();
        for command in commands {
            match command {
                Command::Shell(command) => {
                    let result = connector.run_shell_command(command.clone(), options).await?;
                    let success = result.success;
                    let exit_code = result.exit_code;
                    let stderr = result.output.stderr();
                    output.extend(result.output);
                    if !success {
                        return Err(CommandExecutionError::ShellFailed { exit_code, stderr });
                    }
                }
                Command::Upload(command) => {
                    connector.put_file(&command.source, &command.destination, options).await?;
                }
                Command::Download(command) => {
                    connector.get_file(&command.source, &command.destination, options).await?;
                }
                Command::Rsync(command) => {
                    connector.rsync(command, options).await?;
                }
            }
        }
        Ok(output)
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
