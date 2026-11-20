use crate::api::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceControl {
    pub service: String,
    pub running: bool,
    pub restarted: bool,
    pub reloaded: bool,
    pub command: Option<String>,
}

impl ServiceControl {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into(), running: true, restarted: false, reloaded: false, command: None }
    }
}

pub fn control_commands(
    input: &ServiceControl,
    status: Option<bool>,
    command: impl Fn(&str, &str) -> Command,
) -> Vec<Command> {
    let mut commands = Vec::new();
    if !input.running && status == Some(true) {
        commands.push(command(&input.service, "stop"));
    }
    if input.running && status != Some(true) {
        commands.push(command(&input.service, "start"));
    }
    if input.restarted && status == Some(true) {
        commands.push(command(&input.service, "restart"));
    }
    if input.reloaded && status == Some(true) {
        commands.push(command(&input.service, "reload"));
    }
    if let Some(custom) = input.command.as_deref().filter(|command| !command.is_empty()) {
        commands.push(command(&input.service, custom));
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::{ServiceControl, control_commands};
    use crate::api::{Command, StringCommand};

    fn command(service: &str, action: &str) -> Command {
        Command::Shell(StringCommand::new([format!("service {service} {action}")]))
    }

    #[test]
    fn starts_down_services_and_only_restarts_running_services() {
        let mut input = ServiceControl::new("nginx");
        input.restarted = true;
        assert_eq!(control_commands(&input, Some(false), command).len(), 1);
        assert_eq!(control_commands(&input, Some(true), command).len(), 1);
    }
}
