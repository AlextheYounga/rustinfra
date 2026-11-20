use indexmap::IndexMap;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::launchd::LaunchdStatus;
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInput {
    pub service: String,
    pub running: bool,
    pub restarted: bool,
    pub command: Option<String>,
}

impl ServiceInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into(), running: true, restarted: false, command: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Service;

impl StatefulOperation for Service {
    type Input = ServiceInput;

    fn name(&self) -> &'static str {
        "launchd.service"
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
            let statuses = FactExecutor::new(connector, options).get(&LaunchdStatus, &()).await?;
            Ok(service_commands(input, &statuses))
        })
    }
}

fn service_commands(input: &ServiceInput, statuses: &IndexMap<String, bool>) -> Vec<Command> {
    let was_running = statuses.get(&input.service).copied();
    let mut commands = Vec::new();
    if !input.running && was_running == Some(true) {
        commands.push(service_command(&input.service, "stop"));
    }
    if input.running && was_running != Some(true) {
        commands.push(service_command(&input.service, "start"));
    }
    if input.restarted && was_running == Some(true) {
        commands.push(service_command(&input.service, "stop"));
        commands.push(service_command(&input.service, "start"));
    }
    commands
}

fn service_command(service: &str, action: &str) -> Command {
    Command::Shell(StringCommand::new([
        CommandBit::from("launchctl"),
        CommandBit::from(action),
        CommandBit::quoted(service),
    ]))
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{ServiceInput, service_commands};
    use crate::api::Command;

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some(command.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn matches_start_stop_noop_and_restart_reference_transitions() {
        let running = IndexMap::from([("com.example.thing".to_owned(), true)]);
        let stopped = IndexMap::from([("com.example.thing".to_owned(), false)]);
        let mut input = ServiceInput::new("com.example.thing");
        assert!(service_commands(&input, &running).is_empty());
        assert_eq!(descriptions(&service_commands(&input, &stopped)), ["launchctl start com.example.thing"]);
        input.running = false;
        assert_eq!(descriptions(&service_commands(&input, &running)), ["launchctl stop com.example.thing"]);
        input.running = true;
        input.restarted = true;
        assert_eq!(
            descriptions(&service_commands(&input, &running)),
            ["launchctl stop com.example.thing", "launchctl start com.example.thing"]
        );
        assert_eq!(descriptions(&service_commands(&input, &stopped)), ["launchctl start com.example.thing"]);
    }

    #[test]
    fn quotes_service_names_and_ignores_the_unsupported_custom_command() {
        let mut input = ServiceInput::new("com.example.thing; reboot");
        input.command = Some("remove".to_owned());
        assert_eq!(
            descriptions(&service_commands(&input, &IndexMap::new())),
            ["launchctl start 'com.example.thing; reboot'"]
        );
    }
}
