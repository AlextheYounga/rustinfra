use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::systemd::{SystemdEnabled, SystemdInput, SystemdStatus, systemctl_command};
use crate::facts::{FactExecutor, FactOptions};

use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DaemonReloadInput {
    pub user_mode: bool,
    pub machine: Option<String>,
    pub user_name: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DaemonReload;

impl Operation for DaemonReload {
    type Input = DaemonReloadInput;

    fn name(&self) -> &'static str {
        "systemd.daemon_reload"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let systemd = SystemdInput {
            user_mode: input.user_mode,
            machine: input.machine.clone(),
            user_name: input.user_name.clone(),
            services: Vec::new(),
        };
        vec![systemctl(&systemd, "daemon-reload", None)]
    }
}

const UNIT_SUFFIXES: &[&str] = &[
    ".service",
    ".socket",
    ".device",
    ".mount",
    ".automount",
    ".swap",
    ".target",
    ".path",
    ".timer",
    ".slice",
    ".scope",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInput {
    pub service: String,
    pub running: Option<bool>,
    pub restarted: bool,
    pub reloaded: bool,
    pub command: Option<String>,
    pub enabled: Option<bool>,
    pub daemon_reload: bool,
    pub user_mode: bool,
    pub machine: Option<String>,
    pub user_name: Option<String>,
}

impl ServiceInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            running: Some(true),
            restarted: false,
            reloaded: false,
            command: None,
            enabled: None,
            daemon_reload: false,
            user_mode: false,
            machine: None,
            user_name: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Service;

impl StatefulOperation for Service {
    type Input = ServiceInput;

    fn name(&self) -> &'static str {
        "systemd.service"
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
            let unit = normalized_unit_name(&input.service);
            let fact_input = SystemdInput {
                user_mode: input.user_mode,
                machine: input.machine.clone(),
                user_name: input.user_name.clone(),
                services: vec![unit.clone()],
            };
            let facts = FactExecutor::new(connector, fact_options);
            let statuses = facts.get(&SystemdStatus, &fact_input).await?;
            let enabled =
                if input.enabled.is_some() { Some(facts.get(&SystemdEnabled, &fact_input).await?) } else { None };
            Ok(service_commands(
                input,
                &fact_input,
                &unit,
                ServiceState {
                    running: statuses.get(&unit).copied(),
                    enabled: enabled.as_ref().and_then(|states| states.get(&unit).copied()),
                },
            ))
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ServiceState {
    running: Option<bool>,
    enabled: Option<bool>,
}

fn service_commands(input: &ServiceInput, systemd: &SystemdInput, unit: &str, state: ServiceState) -> Vec<Command> {
    let mut commands = Vec::new();
    if input.daemon_reload {
        commands.push(systemctl(systemd, "daemon-reload", None));
    }
    match input.running {
        Some(false) if state.running == Some(true) => commands.push(systemctl(systemd, "stop", Some(unit))),
        Some(true) if state.running != Some(true) => commands.push(systemctl(systemd, "start", Some(unit))),
        _ => {}
    }
    if input.restarted && state.running == Some(true) {
        commands.push(systemctl(systemd, "restart", Some(unit)));
    }
    if input.reloaded && state.running == Some(true) {
        commands.push(systemctl(systemd, "reload", Some(unit)));
    }
    if let Some(action) = input.command.as_deref() {
        commands.push(systemctl(systemd, action, Some(unit)));
    }
    match (input.enabled, state.enabled) {
        (Some(true), Some(false) | None) => commands.push(systemctl(systemd, "enable", Some(unit))),
        (Some(false), Some(true)) => commands.push(systemctl(systemd, "disable", Some(unit))),
        _ => {}
    }
    commands
}

fn normalized_unit_name(service: &str) -> String {
    if UNIT_SUFFIXES.iter().any(|suffix| service.ends_with(suffix)) {
        service.to_owned()
    } else {
        format!("{service}.service")
    }
}

fn systemctl(input: &SystemdInput, action: &str, unit: Option<&str>) -> Command {
    let mut bits = vec![systemctl_command(input).into(), CommandBit::from(action)];
    if let Some(unit) = unit {
        bits.push(CommandBit::quoted(unit));
    }
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use super::{DaemonReload, DaemonReloadInput, ServiceInput, ServiceState, normalized_unit_name, service_commands};
    use crate::api::Command;
    use crate::facts::systemd::SystemdInput;
    use crate::operations::Operation;

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
    fn service_starts_and_enables_a_stopped_unit() {
        let mut input = ServiceInput::new("redis-server");
        input.enabled = Some(true);
        let unit = normalized_unit_name(&input.service);

        assert_eq!(unit, "redis-server.service");
        assert_eq!(
            descriptions(&service_commands(
                &input,
                &SystemdInput::default(),
                &unit,
                ServiceState { running: Some(false), enabled: Some(false) },
            )),
            ["systemctl start redis-server.service", "systemctl enable redis-server.service"]
        );
    }

    #[test]
    fn running_unit_is_a_noop_unless_restart_reload_or_custom_command_is_requested() {
        let mut input = ServiceInput::new("logrotate.timer");
        let unit = normalized_unit_name(&input.service);
        assert!(
            service_commands(
                &input,
                &SystemdInput::default(),
                &unit,
                ServiceState { running: Some(true), enabled: None },
            )
            .is_empty()
        );

        input.restarted = true;
        input.reloaded = true;
        input.command = Some("try-restart".to_owned());
        assert_eq!(
            descriptions(&service_commands(
                &input,
                &SystemdInput::default(),
                &unit,
                ServiceState { running: Some(true), enabled: None },
            )),
            [
                "systemctl restart logrotate.timer",
                "systemctl reload logrotate.timer",
                "systemctl try-restart logrotate.timer"
            ]
        );
    }

    #[test]
    fn daemon_reload_and_machine_scope_match_systemd_command_order() {
        let mut input = ServiceInput::new("worker");
        input.daemon_reload = true;
        input.machine = Some("container".to_owned());
        input.user_name = Some("app".to_owned());
        let systemd = SystemdInput {
            machine: input.machine.clone(),
            user_name: input.user_name.clone(),
            services: vec!["worker.service".to_owned()],
            ..SystemdInput::default()
        };
        assert_eq!(
            descriptions(&service_commands(
                &input,
                &systemd,
                "worker.service",
                ServiceState { running: Some(false), enabled: None },
            )),
            [
                "systemctl --machine=app@container daemon-reload",
                "systemctl --machine=app@container start worker.service"
            ]
        );
    }

    #[test]
    fn standalone_daemon_reload_supports_user_and_machine_scopes() {
        let input = DaemonReloadInput {
            user_mode: true,
            machine: Some("container".to_owned()),
            user_name: Some("app".to_owned()),
        };
        assert_eq!(
            descriptions(&DaemonReload.commands(&input)),
            ["systemctl --user --machine=app@container daemon-reload"]
        );
        assert!(!DaemonReload.is_idempotent());
    }
}
