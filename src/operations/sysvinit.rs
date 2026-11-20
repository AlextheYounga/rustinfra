use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{FindLinks, FindLinksInput};
use crate::facts::server::LinuxDistribution;
use crate::facts::sysvinit::InitdStatus;
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::{Link, LinkInput};

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
        "sysvinit.service"
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
            let statuses = executor.get(&InitdStatus, &()).await?;
            let mut commands = control_commands(
                &input.control,
                statuses.get(&input.control.service).copied().flatten(),
                service_command,
            );
            if let Some(enabled) = input.enabled {
                let mut find = FindLinksInput::new("/etc");
                find.path_match = Some(format!("/etc/rc*.d/S*{}", input.control.service));
                let links = executor.get(&FindLinks, &find).await?;
                if enabled && links.is_empty() {
                    let distribution = executor.get(&LinuxDistribution, &()).await?;
                    commands.extend(enable_commands(&input.control.service, distribution.name.as_deref()));
                } else if !enabled {
                    commands.extend(links.iter().map(|link| shell([raw("rm -f"), quoted(link)])));
                }
            }
            Ok(commands)
        })
    }
}

fn service_command(service: &str, action: &str) -> Command {
    Command::Shell(StringCommand::with_separator([raw("/etc/init.d/"), quoted(service), raw(" "), quoted(action)], ""))
}

fn enable_commands(service: &str, distribution: Option<&str>) -> Vec<Command> {
    match distribution {
        Some("Ubuntu" | "Debian") => vec![shell([raw("update-rc.d"), quoted(service), raw("defaults")])],
        Some("CentOS" | "Fedora" | "Red Hat Enterprise Linux") => vec![
            shell([raw("chkconfig"), quoted(service), raw("--add")]),
            shell([raw("chkconfig"), quoted(service), raw("on")]),
        ],
        Some("Gentoo") => vec![shell([raw("rc-update add"), quoted(service), raw("default")])],
        _ => Vec::new(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnableInput {
    pub service: String,
    pub start_priority: u8,
    pub stop_priority: u8,
    pub start_levels: Vec<u8>,
    pub stop_levels: Vec<u8>,
}

impl EnableInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            start_priority: 20,
            stop_priority: 80,
            start_levels: vec![2, 3, 4, 5],
            stop_levels: vec![0, 1, 6],
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Enable;

impl StatefulOperation for Enable {
    type Input = EnableInput;
    fn name(&self) -> &'static str {
        "sysvinit.enable"
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
            let target = format!("/etc/init.d/{}", input.service);
            let links = input
                .start_levels
                .iter()
                .map(|level| format!("/etc/rc{level}.d/S{}{}", input.start_priority, input.service))
                .chain(
                    input
                        .stop_levels
                        .iter()
                        .map(|level| format!("/etc/rc{level}.d/K{}{}", input.stop_priority, input.service)),
                );
            let mut commands = Vec::new();
            for path in links {
                let link = LinkInput::new(path, &target);
                commands.extend(Link.commands(&link, connector, options).await?);
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
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use super::{enable_commands, service_command};
    use crate::api::Command;

    fn description(command: Command) -> String {
        match command {
            Command::Shell(value) => value.raw_value(),
            _ => String::new(),
        }
    }

    #[test]
    fn quotes_service_paths_and_selects_distribution_enable_commands() {
        assert_eq!(
            description(service_command("my service; whoami", "start")),
            "/etc/init.d/'my service; whoami' start"
        );
        assert_eq!(
            enable_commands("x; reboot", Some("CentOS")).into_iter().map(description).collect::<Vec<_>>(),
            ["chkconfig 'x; reboot' --add", "chkconfig 'x; reboot' on"]
        );
    }
}
