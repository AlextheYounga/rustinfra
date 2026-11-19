use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{FileContents, PathInput};
use crate::facts::server::{Hostname as HostnameFact, Os, Which};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum HostnameFile {
    #[default]
    Auto,
    Disabled,
    Path(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostnameInput {
    pub hostname: String,
    pub file: HostnameFile,
}

impl HostnameInput {
    #[must_use]
    pub fn new(hostname: impl Into<String>) -> Self {
        Self { hostname: hostname.into(), file: HostnameFile::Auto }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Hostname;

impl StatefulOperation for Hostname {
    type Input = HostnameInput;

    fn name(&self) -> &'static str {
        "server.hostname"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, fact_options);
            let current = facts.get(&HostnameFact, &()).await?;
            let hostnamectl = facts.get(&Which, &"hostnamectl".to_owned()).await?;
            if hostnamectl.is_some() {
                return Ok(if current.as_deref() == Some(input.hostname.as_str()) {
                    Vec::new()
                } else {
                    vec![command([raw("hostnamectl"), raw("set-hostname"), quoted(&input.hostname)])]
                });
            }

            let mut commands = Vec::new();
            if current.as_deref() != Some(input.hostname.as_str()) {
                commands.push(command([raw("hostname"), quoted(&input.hostname)]));
            }
            let path = match &input.file {
                HostnameFile::Disabled => None,
                HostnameFile::Path(path) => Some(path.as_str()),
                HostnameFile::Auto => match facts.get(&Os, &()).await?.as_deref() {
                    Some("Linux") => Some("/etc/hostname"),
                    Some("OpenBSD") => Some("/etc/myname"),
                    _ => None,
                },
            };
            if let Some(path) = path {
                let contents = facts.get(&FileContents, &PathInput::from(path)).await?;
                if contents.as_deref() != Some(slice::from_ref(&input.hostname)) {
                    commands.push(command([
                        raw("printf"),
                        raw("'%s\\n'"),
                        quoted(&input.hostname),
                        raw(">"),
                        quoted(path),
                    ]));
                }
            }
            Ok(commands)
        })
    }
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}

fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}

fn command(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Hostname, HostnameFile, HostnameInput};
    use crate::connectors::LocalConnector;
    use crate::facts::FactOptions;
    use crate::operations::StatefulOperation;

    #[tokio::test]
    async fn current_hostname_with_disabled_file_is_a_noop() {
        let current = Hostname
            .commands(
                &HostnameInput {
                    hostname: fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_owned(),
                    file: HostnameFile::Disabled,
                },
                &LocalConnector,
                &FactOptions::default(),
            )
            .await
            .unwrap();

        assert!(current.is_empty());
    }
}
use std::slice;
