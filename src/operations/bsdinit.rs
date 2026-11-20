use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::bsdinit::{RcdEnabled, RcdStatus, ServiceEnabled};
use crate::facts::server::Os;
use crate::facts::{FactError, FactExecutor, FactOptions};
use crate::operations::files::{Line, LineInput};

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
        "bsdinit.service"
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
            let os = executor.get(&Os, &()).await?.unwrap_or_default();
            let statuses = executor.get(&RcdStatus, &()).await?;
            let status = statuses.get(&input.control.service).copied().flatten();
            let mut commands = Vec::new();
            if input.enabled == Some(true) {
                commands.extend(enabled_commands(&executor, &os, &input.control.service, true).await?);
            }
            commands.extend(control_commands(&input.control, status, |service, action| {
                service_command(&os, service, action)
            }));
            if input.enabled == Some(false) {
                commands.extend(enabled_commands(&executor, &os, &input.control.service, false).await?);
            }
            if input.enabled.is_some() && os != "OpenBSD" && os != "FreeBSD" {
                let mut line = LineInput::new("/etc/rc.conf.local", format!("^{}_enable=", input.control.service));
                line.replace = Some(format!("{}_enable=\"YES\"", input.control.service));
                line.present = input.enabled == Some(true);
                commands.extend(Line.commands(&line, connector, options).await?);
            }
            Ok(commands)
        })
    }
}

async fn enabled_commands(
    executor: &FactExecutor<'_>,
    os: &str,
    service: &str,
    enabled: bool,
) -> Result<Vec<Command>, FactError> {
    let current = match os {
        "OpenBSD" => executor.get(&RcdEnabled, &()).await?.get(service).copied().unwrap_or(false),
        "FreeBSD" => executor.get(&ServiceEnabled, &()).await?.get(service).copied().unwrap_or(false),
        _ => return Ok(Vec::new()),
    };
    if current == enabled {
        return Ok(Vec::new());
    }
    Ok(vec![match os {
        "OpenBSD" => shell([raw("rcctl"), raw(if enabled { "enable" } else { "disable" }), quoted(service)]),
        _ => shell([raw("service"), quoted(service), raw(if enabled { "enable" } else { "disable" })]),
    }])
}

fn service_command(os: &str, service: &str, action: &str) -> Command {
    if os == "OpenBSD" {
        return shell([raw("rcctl"), quoted(action), quoted(service)]);
    }
    let path = StringCommand::new([CommandBit::quoted(service)]).raw_value();
    let action = StringCommand::new([CommandBit::quoted(action)]).raw_value();
    shell([raw(format!(
        "test -e /etc/rc.d/{path} && /etc/rc.d/{path} {action} || /usr/local/etc/rc.d/{path} {action}"
    ))])
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
    use super::service_command;
    use crate::api::Command;

    fn description(command: Command) -> String {
        match command {
            Command::Shell(value) => value.raw_value(),
            _ => String::new(),
        }
    }

    #[test]
    fn selects_openbsd_and_freebsd_command_formats_with_safe_service_names() {
        assert_eq!(description(service_command("OpenBSD", "rad", "start")), "rcctl start rad");
        assert_eq!(
            description(service_command("FreeBSD", "my service; whoami", "start")),
            "test -e /etc/rc.d/'my service; whoami' && /etc/rc.d/'my service; whoami' start || /usr/local/etc/rc.d/'my service; whoami' start"
        );
    }
}
