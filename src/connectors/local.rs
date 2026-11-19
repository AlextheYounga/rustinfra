use std::io;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tempfile::NamedTempFile;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::time;
use tokio::{fs, task};

use crate::api::{CommandBit, RsyncCommand, StringCommand};

use super::{CommandOutput, OutputStream, UnixCommandOptions, make_unix_command};

/// Per-command controls handled by a connector rather than shell generation.
#[derive(Clone, Debug, PartialEq)]
pub struct RunOptions {
    pub unix: UnixCommandOptions,
    pub success_exit_codes: Vec<i32>,
    pub timeout: Option<Duration>,
    pub stdin: Vec<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { unix: UnixCommandOptions::default(), success_exit_codes: vec![0], timeout: None, stdin: Vec::new() }
    }
}

/// Result of executing a command through a connector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub output: CommandOutput,
}

/// Failure to start, communicate with, or complete a local command.
#[derive(Debug, Error)]
pub enum LocalConnectorError {
    #[error("failed to execute local command: {0}")]
    Process(#[from] io::Error),
    #[error("local command did not provide its {0} pipe")]
    MissingPipe(&'static str),
    #[error("local command timed out after {0:?}")]
    Timeout(Duration),
    #[error("local command input task failed: {0}")]
    InputTask(#[from] task::JoinError),
    #[error("local file copy failed: {stderr}")]
    CopyFailed { stderr: String },
}

/// Executes commands and transfers against the machine running rustinfra.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalConnector;

impl LocalConnector {
    /// Runs a command using the same Unix wrappers used by remote connectors.
    pub async fn run_shell_command(
        &self,
        command: StringCommand,
        options: &RunOptions,
    ) -> Result<CommandResult, LocalConnectorError> {
        let unix_command = make_unix_command(command, &options.unix);
        let process = run_process(unix_command.raw_value(), &options.stdin);
        let (exit_code, output) = match options.timeout {
            Some(timeout) => {
                time::timeout(timeout, process).await.map_err(|_| LocalConnectorError::Timeout(timeout))??
            }
            None => process.await?,
        };
        let success = exit_code.is_some_and(|code| options.success_exit_codes.contains(&code));
        Ok(CommandResult { success, exit_code, output })
    }

    /// Copies a local file to a target path, using `cp` so privilege wrappers apply.
    pub async fn put_file(
        &self,
        source: &Path,
        destination: &str,
        options: &RunOptions,
    ) -> Result<(), LocalConnectorError> {
        let temporary_file = NamedTempFile::new()?;
        fs::copy(source, temporary_file.path()).await?;
        let command = StringCommand::new(vec![
            CommandBit::from("cp"),
            CommandBit::quoted(temporary_file.path().to_path_buf()),
            CommandBit::quoted(destination),
        ]);
        self.require_success(command, options).await
    }

    /// Copies a target path to a local file, using `cp` so privilege wrappers apply.
    pub async fn get_file(
        &self,
        source: &str,
        destination: &Path,
        options: &RunOptions,
    ) -> Result<(), LocalConnectorError> {
        let temporary_file = NamedTempFile::new()?;
        let command = StringCommand::new(vec![
            CommandBit::from("cp"),
            CommandBit::quoted(source),
            CommandBit::quoted(temporary_file.path().to_path_buf()),
        ]);
        self.require_success(command, options).await?;
        fs::copy(temporary_file.path(), destination).await?;
        Ok(())
    }

    /// Runs local rsync with privilege and shell wrappers applied.
    pub async fn rsync(&self, command: &RsyncCommand, options: &RunOptions) -> Result<(), LocalConnectorError> {
        let mut bits = vec![CommandBit::from("rsync")];
        bits.extend(command.flags.iter().cloned().map(CommandBit::quoted));
        bits.push(CommandBit::quoted(command.source.clone()));
        bits.push(CommandBit::quoted(command.destination.clone()));
        self.require_success(StringCommand::new(bits), options).await
    }

    async fn require_success(&self, command: StringCommand, options: &RunOptions) -> Result<(), LocalConnectorError> {
        let result = self.run_shell_command(command, options).await?;
        if result.success { Ok(()) } else { Err(LocalConnectorError::CopyFailed { stderr: result.output.stderr() }) }
    }
}

async fn run_process(
    command: String,
    stdin_lines: &[String],
) -> Result<(Option<i32>, CommandOutput), LocalConnectorError> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdin = child.stdin.take().ok_or(LocalConnectorError::MissingPipe("stdin"))?;
    let stdin_lines = stdin_lines.to_vec();
    let input_task = task::spawn(write_stdin(stdin, stdin_lines));

    let stdout = child.stdout.take().ok_or(LocalConnectorError::MissingPipe("stdout"))?;
    let stderr = child.stderr.take().ok_or(LocalConnectorError::MissingPipe("stderr"))?;
    let result = read_process_output(&mut child, stdout, stderr).await;
    input_task.await??;
    result
}

async fn write_stdin(mut stdin: ChildStdin, lines: Vec<String>) -> Result<(), io::Error> {
    for line in &lines {
        stdin.write_all(line.as_bytes()).await?;
        if !line.ends_with('\n') {
            stdin.write_all(b"\n").await?;
        }
    }
    stdin.shutdown().await?;
    Ok(())
}

async fn read_process_output(
    child: &mut Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
) -> Result<(Option<i32>, CommandOutput), LocalConnectorError> {
    let mut stdout = BufReader::new(stdout).lines();
    let mut stderr = BufReader::new(stderr).lines();
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut output = CommandOutput::default();

    while stdout_open || stderr_open {
        tokio::select! {
            line = stdout.next_line(), if stdout_open => {
                match line? {
                    Some(line) => output.push(OutputStream::Stdout, line),
                    None => stdout_open = false,
                }
            }
            line = stderr.next_line(), if stderr_open => {
                match line? {
                    Some(line) => output.push(OutputStream::Stderr, line),
                    None => stderr_open = false,
                }
            }
        }
    }

    let status = child.wait().await?;
    Ok((status.code(), output))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{LocalConnector, LocalConnectorError, RunOptions};
    use crate::api::{CommandBit, HiddenValue, StringCommand};

    #[tokio::test]
    async fn shell_command_captures_both_streams_and_honors_success_codes() {
        let options = RunOptions { success_exit_codes: vec![7], ..RunOptions::default() };
        let command = StringCommand::new(["printf 'out\\n'; printf 'err\\n' >&2; exit 7"]);

        let result = LocalConnector.run_shell_command(command, &options).await.unwrap();

        assert!(result.success);
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.output.stdout(), "out");
        assert_eq!(result.output.stderr(), "err");
    }

    #[tokio::test]
    async fn shell_command_writes_each_stdin_value_as_a_line() {
        let options = RunOptions { stdin: vec!["hello".to_owned(), "abc\n".to_owned()], ..RunOptions::default() };

        let result = LocalConnector.run_shell_command(StringCommand::new(["cat"]), &options).await.unwrap();

        assert_eq!(result.output.stdout(), "hello\nabc");
    }

    #[tokio::test]
    async fn shell_command_times_out() {
        let options = RunOptions { timeout: Some(Duration::from_millis(20)), ..RunOptions::default() };

        let error = LocalConnector.run_shell_command(StringCommand::new(["sleep 1"]), &options).await.unwrap_err();

        assert!(matches!(error, LocalConnectorError::Timeout(_)));
    }

    #[tokio::test]
    async fn hidden_command_value_executes_without_appearing_in_display() {
        let command = StringCommand::new(vec![
            CommandBit::from("printf %s"),
            CommandBit::quoted(HiddenValue::new("top-secret-stuff")),
        ]);
        assert!(!command.to_string().contains("top-secret-stuff"));

        let result = LocalConnector.run_shell_command(command, &RunOptions::default()).await.unwrap();

        assert_eq!(result.output.stdout(), "top-secret-stuff");
    }
}
