use std::error::Error;

use thiserror::Error;

use crate::api::{CommandBit, StringCommand};
use crate::connectors::{Connector, ConnectorError, RunOptions};

const MISSING_COMMAND_MARKER: &str = "##PYINFRA_NOCMD##";
const SU_OUTPUT_MARKER: &str = "##PYINFRA_SU_READY##";

/// A typed read-only query against a host.
pub trait Fact: Sync {
    type Input: Sync;
    type Output;

    fn name(&self) -> &'static str;
    fn command(&self, input: &Self::Input) -> StringCommand;
    fn default(&self) -> Self::Output;

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        None
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>>;
}

/// Whether a fact is being used to prepare or execute a deploy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Prepare,
    Execute,
}

/// Fact-specific execution controls.
#[derive(Clone, Debug, PartialEq)]
pub struct FactOptions {
    pub run: RunOptions,
    pub ignore_errors: bool,
    pub phase: Phase,
}

impl Default for FactOptions {
    fn default() -> Self {
        Self { run: RunOptions::default(), ignore_errors: false, phase: Phase::Prepare }
    }
}

/// Failure to execute or process a fact.
#[derive(Debug, Error)]
pub enum FactError {
    #[error("could not execute fact {fact}: {source}")]
    Connector {
        fact: &'static str,
        #[source]
        source: ConnectorError,
    },
    #[error("could not load fact {fact}: command exited with {exit_code:?}: {stderr}")]
    CommandFailed { fact: &'static str, exit_code: Option<i32>, stderr: String },
    #[error("could not process fact {fact}: {source}")]
    Process {
        fact: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Runs typed facts through one connector using shared execution options.
pub struct FactExecutor<'a> {
    connector: &'a dyn Connector,
    options: &'a FactOptions,
}

impl<'a> FactExecutor<'a> {
    #[must_use]
    pub fn new(connector: &'a dyn Connector, options: &'a FactOptions) -> Self {
        Self { connector, options }
    }

    pub async fn get<F: Fact>(&self, fact: &F, input: &F::Input) -> Result<F::Output, FactError> {
        let command = prepare_command(fact.command(input), fact.requires_command(input), &self.options.run);
        let result = match self.connector.run_shell_command(command, &self.options.run).await {
            Ok(result) => result,
            Err(_source) if self.options.ignore_errors => return Ok(fact.default()),
            Err(source) => return Err(FactError::Connector { fact: fact.name(), source }),
        };
        let stdout = strip_su_output_marker(result.output.stdout_lines(), &self.options.run);

        if result.success && stdout == [MISSING_COMMAND_MARKER] {
            return Ok(fact.default());
        }
        if !result.success {
            if self.options.ignore_errors {
                return Ok(fact.default());
            }
            return Err(FactError::CommandFailed {
                fact: fact.name(),
                exit_code: result.exit_code,
                stderr: result.output.stderr(),
            });
        }
        if stdout.is_empty() {
            return Ok(fact.default());
        }

        fact.process(&stdout, input).map_err(|source| FactError::Process { fact: fact.name(), source })
    }
}

fn prepare_command(command: StringCommand, requires_command: Option<&str>, options: &RunOptions) -> StringCommand {
    let command = match requires_command {
        Some(required) => StringCommand::new(vec![
            CommandBit::from("if command -v"),
            CommandBit::quoted(required),
            CommandBit::from(">/dev/null 2>&1; then ("),
            command.into(),
            CommandBit::from("); else echo"),
            CommandBit::quoted(MISSING_COMMAND_MARKER),
            CommandBit::from("; fi"),
        ]),
        None => command,
    };

    if options.unix.su_user.is_some() {
        StringCommand::new(vec![
            CommandBit::from("printf"),
            CommandBit::quoted(SU_OUTPUT_MARKER),
            CommandBit::from("&&"),
            command.into(),
        ])
    } else {
        command
    }
}

fn strip_su_output_marker(lines: Vec<&str>, options: &RunOptions) -> Vec<String> {
    if options.unix.su_user.is_none() {
        return lines.into_iter().map(str::to_owned).collect();
    }

    let Some((index, marker_line)) = lines.iter().enumerate().find(|(_, line)| line.contains(SU_OUTPUT_MARKER)) else {
        return lines.into_iter().map(str::to_owned).collect();
    };
    let mut stripped = lines[index..].iter().map(|line| (*line).to_owned()).collect::<Vec<_>>();
    stripped[0] = marker_line.split_once(SU_OUTPUT_MARKER).map_or_else(String::new, |(_, value)| value.to_owned());
    stripped
}

#[cfg(test)]
mod tests {
    use super::{FactExecutor, FactOptions};
    use crate::connectors::LocalConnector;
    use crate::facts::server::{Command, Which};

    #[tokio::test]
    async fn command_fact_returns_raw_output_lines_joined_by_newlines() {
        let options = FactOptions::default();
        let output = FactExecutor::new(&LocalConnector, &options)
            .get(&Command, &"printf 'first\\nsecond\\n'".to_owned())
            .await
            .unwrap();

        assert_eq!(output.as_deref(), Some("first\nsecond"));
    }

    #[tokio::test]
    async fn missing_required_command_returns_fact_default() {
        let options = FactOptions::default();
        let output = FactExecutor::new(&LocalConnector, &options)
            .get(&Which, &"rustinfra-command-that-does-not-exist".to_owned())
            .await
            .unwrap();

        assert_eq!(output, None);
    }
}
