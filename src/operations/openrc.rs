use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::openrc::{OpenrcEnabled, OpenrcStatus};
use crate::facts::{FactExecutor, FactOptions};

use super::service::{ServiceControl, control_commands};
use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInput {
    pub control: ServiceControl,
    pub enabled: Option<bool>,
    pub runlevel: String,
}

impl ServiceInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self { control: ServiceControl::new(service), enabled: None, runlevel: "default".to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Service;

impl StatefulOperation for Service {
    type Input = ServiceInput;
    fn name(&self) -> &'static str {
        "openrc.service"
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
            let statuses = executor.get(&OpenrcStatus, &input.runlevel).await?;
            let mut commands =
                control_commands(&input.control, statuses.get(&input.control.service).copied(), service_command);
            if let Some(enabled) = input.enabled {
                let current = executor.get(&OpenrcEnabled, &input.runlevel).await?;
                if current.get(&input.control.service).copied().unwrap_or(false) != enabled {
                    commands.push(enable_command(&input.control.service, &input.runlevel, enabled));
                }
            }
            Ok(commands)
        })
    }
}

fn service_command(service: &str, action: &str) -> Command {
    shell([raw("rc-service"), quoted(service), quoted(action)])
}

fn enable_command(service: &str, runlevel: &str, enabled: bool) -> Command {
    shell([raw(if enabled { "rc-update add" } else { "rc-update del" }), quoted(service), quoted(runlevel)])
}

fn raw(value: &str) -> CommandBit {
    CommandBit::from(value)
}
fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use super::{ServiceInput, enable_command, service_command};
    use crate::api::Command;
    use crate::operations::service::control_commands;

    fn description(command: Command) -> String {
        match command {
            Command::Shell(value) => value.raw_value(),
            _ => String::new(),
        }
    }

    #[test]
    fn matches_start_enable_and_safe_runlevel_commands() {
        let input = ServiceInput::new("my service; whoami");
        assert_eq!(
            description(control_commands(&input.control, Some(false), service_command).remove(0)),
            "rc-service 'my service; whoami' start"
        );
        assert_eq!(description(enable_command("nftables", "boot", false)), "rc-update del nftables boot");
    }
}
