use crate::api::{Command, CommandBit, ContentUploadCommand, StringCommand, sha1_hex};
use crate::connectors::Connector;
use crate::facts::files::{File as FileFact, PathInfo, PathInput, Sha1File};
use crate::facts::upstart::UpstartStatus;
use crate::facts::{FactExecutor, FactOptions};

use super::service::{ServiceControl, control_commands};
use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInput {
    pub control: ServiceControl,
    pub enabled: Option<bool>,
}

impl ServiceInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self { control: ServiceControl::new(service), enabled: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Service;

impl StatefulOperation for Service {
    type Input = ServiceInput;
    fn name(&self) -> &'static str {
        "upstart.service"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let executor = FactExecutor::new(connector, options);
            let statuses = executor.get(&UpstartStatus, &()).await?;
            let mut commands =
                control_commands(&input.control, statuses.get(&input.control.service).copied(), service_command);
            let override_path = format!("/etc/init/{}.override", input.control.service);
            match input.enabled {
                Some(true) => {
                    let exists = matches!(
                        executor.get(&FileFact, &PathInput::from(override_path.clone())).await?,
                        PathInfo::Present(_)
                    );
                    commands.extend(override_commands(true, exists, None, override_path));
                }
                Some(false) => {
                    let remote_hash = executor.get(&Sha1File, &PathInput::from(override_path.clone())).await?;
                    commands.extend(override_commands(false, false, remote_hash.as_deref(), override_path));
                }
                None => {}
            }
            Ok(commands)
        })
    }
}

fn override_commands(enabled: bool, exists: bool, remote_hash: Option<&str>, path: String) -> Vec<Command> {
    if enabled {
        return exists
            .then(|| Command::Shell(StringCommand::new([CommandBit::from("rm -f"), CommandBit::quoted(path)])))
            .into_iter()
            .collect();
    }

    let content = b"manual\n";
    (remote_hash != Some(sha1_hex(content).as_str()))
        .then(|| {
            Command::UploadContent(ContentUploadCommand {
                content: content.to_vec(),
                destination: path,
                remote_temporary_filename: None,
            })
        })
        .into_iter()
        .collect()
}

fn service_command(service: &str, action: &str) -> Command {
    Command::Shell(StringCommand::new([
        CommandBit::from("initctl"),
        CommandBit::quoted(action),
        CommandBit::quoted(service),
    ]))
}

#[cfg(test)]
mod tests {
    use super::{override_commands, service_command};
    use crate::api::{Command, sha1_hex};

    #[test]
    fn quotes_service_and_action() {
        let Command::Shell(command) = service_command("redis server", "start") else { return };
        assert_eq!(command.raw_value(), "initctl start 'redis server'");
    }

    #[test]
    fn override_file_transitions_are_idempotent() {
        let path = "/etc/init/redis-server.override".to_owned();
        assert!(override_commands(true, false, None, path.clone()).is_empty());
        assert!(override_commands(false, false, Some(&sha1_hex(b"manual\n")), path.clone()).is_empty());
        assert!(matches!(override_commands(false, false, None, path.clone()).as_slice(), [Command::UploadContent(_)]));
        let commands = override_commands(true, true, None, path);
        let [Command::Shell(remove)] = commands.as_slice() else { return };
        assert_eq!(remove.raw_value(), "rm -f /etc/init/redis-server.override");
    }
}
