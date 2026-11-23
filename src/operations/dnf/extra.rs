use std::error::Error;
use std::fmt;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::dnf::{DnfDisabledModules, DnfEnabledModules};
use crate::facts::{FactExecutor, FactOptions};

use crate::operations::rpm_manager::{RepoConfig, YUM_REPOSITORY_DIRECTORY, repo_commands, rpm_commands};
pub use crate::operations::rpm_manager::{RepoInput, RpmInput};
use crate::operations::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Copy, Debug, Default)]
pub struct Key;
impl Operation for Key {
    type Input = String;
    fn name(&self) -> &'static str {
        "dnf.key"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, source: &String) -> Vec<Command> {
        vec![Command::Shell(StringCommand::new([CommandBit::from("rpm --import"), CommandBit::quoted(source.clone())]))]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;
impl Operation for Update {
    type Input = ();
    fn name(&self) -> &'static str {
        "dnf.update"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, _: &()) -> Vec<Command> {
        vec![Command::Shell(StringCommand::new(["dnf update -y"]))]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Repo;
impl StatefulOperation for Repo {
    type Input = RepoInput;
    fn name(&self) -> &'static str {
        "dnf.repo"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a RepoInput,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            repo_commands(
                input,
                connector,
                options,
                RepoConfig { directory: YUM_REPOSITORY_DIRECTORY, repository_type: None },
            )
            .await
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rpm;
impl StatefulOperation for Rpm {
    type Input = RpmInput;
    fn name(&self) -> &'static str {
        "dnf.rpm"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a RpmInput,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move { rpm_commands(input, connector, options, "dnf").await })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleInput {
    pub module: String,
    pub stream: Option<String>,
    pub enabled: bool,
}
impl ModuleInput {
    pub fn enable(module: impl Into<String>, stream: impl Into<String>) -> Self {
        Self { module: module.into(), stream: Some(stream.into()), enabled: true }
    }
    pub fn disable(module: impl Into<String>) -> Self {
        Self { module: module.into(), stream: None, enabled: false }
    }
}

#[derive(Debug)]
struct MissingStream;
impl fmt::Display for MissingStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("stream is required when enabled is true")
    }
}
impl Error for MissingStream {}

#[derive(Clone, Copy, Debug, Default)]
pub struct Module;
impl StatefulOperation for Module {
    type Input = ModuleInput;
    fn name(&self) -> &'static str {
        "dnf.module"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a ModuleInput,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, options);
            if input.enabled {
                let stream =
                    input.stream.as_deref().ok_or_else(|| Box::new(MissingStream) as Box<dyn Error + Send + Sync>)?;
                let enabled = facts.get(&DnfEnabledModules, &()).await?;
                if enabled.get(&input.module).map(String::as_str) == Some(stream) {
                    return Ok(Vec::new());
                }
                Ok(vec![Command::Shell(StringCommand::new([
                    CommandBit::from("dnf module enable -y"),
                    CommandBit::quoted(format!("{}:{stream}", input.module)),
                ]))])
            } else {
                let disabled = facts.get(&DnfDisabledModules, &()).await?;
                if disabled.contains(&input.module) {
                    return Ok(Vec::new());
                }
                Ok(vec![Command::Shell(StringCommand::new([
                    CommandBit::from("dnf module disable -y"),
                    CommandBit::quoted(input.module.clone()),
                ]))])
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn non_stateful_commands_quote_sources() {
        assert!(
            matches!(&Key.commands(&"key; id".to_owned())[0], Command::Shell(command) if command.raw_value() == "rpm --import 'key; id'")
        );
        assert!(matches!(&Update.commands(&())[0], Command::Shell(command) if command.raw_value() == "dnf update -y"));
    }
}
