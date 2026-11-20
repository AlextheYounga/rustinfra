use std::error::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{File as FileFact, PathInfo, PathInput};
use crate::facts::runit::{RunitInput, RunitManaged, RunitStatus};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::{File, FileInput, Link, LinkInput};

use super::service::{ServiceControl, control_commands};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

pub const DEFAULT_SERVICE_DIRECTORY: &str = "/var/service";
pub const DEFAULT_SOURCE_DIRECTORY: &str = "/etc/sv";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManageInput {
    pub service: String,
    pub managed: bool,
    pub service_directory: String,
    pub source_directory: String,
}

impl ManageInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            managed: true,
            service_directory: DEFAULT_SERVICE_DIRECTORY.to_owned(),
            source_directory: DEFAULT_SOURCE_DIRECTORY.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Manage;

impl StatefulOperation for Manage {
    type Input = ManageInput;
    fn name(&self) -> &'static str {
        "runit.manage"
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
        Box::pin(async move { manage_commands(input, connector, options).await })
    }
}

async fn manage_commands(
    input: &ManageInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let mut link = LinkInput::new(
        format!("{}/{}", input.service_directory.trim_end_matches('/'), input.service),
        format!("{}/{}", input.source_directory.trim_end_matches('/'), input.service),
    );
    link.present = input.managed;
    link.create_remote_dir = false;
    Link.commands(&link, connector, options).await
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoInput {
    pub service: String,
    pub auto: bool,
    pub source_directory: String,
}

impl AutoInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into(), auto: true, source_directory: DEFAULT_SOURCE_DIRECTORY.to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Auto;

impl StatefulOperation for Auto {
    type Input = AutoInput;
    fn name(&self) -> &'static str {
        "runit.auto"
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
        Box::pin(async move { auto_commands(input, connector, options).await })
    }
}

async fn auto_commands(
    input: &AutoInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let mut file = FileInput::new(format!("{}/{}/down", input.source_directory.trim_end_matches('/'), input.service));
    file.present = !input.auto;
    file.create_remote_dir = false;
    File.commands(&file, connector, options).await
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitRunsvInput {
    pub service: String,
    pub service_directory: String,
    pub timeout_seconds: u32,
}

impl WaitRunsvInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into(), service_directory: DEFAULT_SERVICE_DIRECTORY.to_owned(), timeout_seconds: 10 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WaitRunsv;

impl Operation for WaitRunsv {
    type Input = WaitRunsvInput;
    fn name(&self) -> &'static str {
        "runit.wait_runsv"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![wait_runsv_command(input)]
    }
}

fn wait_runsv_command(input: &WaitRunsvInput) -> Command {
    let directory = quote(&input.service_directory);
    let service = quote(&input.service);
    let timeout = quote(&input.timeout_seconds.to_string());
    Command::Shell(StringCommand::new([format!(
        "export SVDIR={directory}\nfor i in $(seq {timeout}); do\n    sv status {service} > /dev/null && exit 0\n    sleep 1;\ndone\nexit 1"
    )]))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInput {
    pub control: ServiceControl,
    pub enabled: Option<bool>,
    pub managed: bool,
    pub service_directory: String,
    pub source_directory: String,
}

impl ServiceInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            control: ServiceControl::new(service),
            enabled: None,
            managed: true,
            service_directory: DEFAULT_SERVICE_DIRECTORY.to_owned(),
            source_directory: DEFAULT_SOURCE_DIRECTORY.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Service;

impl StatefulOperation for Service {
    type Input = ServiceInput;
    fn name(&self) -> &'static str {
        "runit.service"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let executor = FactExecutor::new(connector, options);
            let fact_input = RunitInput {
                service: Some(input.control.service.clone()),
                service_directory: input.service_directory.clone(),
            };
            let was_managed = executor.get(&RunitManaged, &fact_input).await?.contains(&input.control.service);
            let down_path = format!("{}/{}/down", input.source_directory.trim_end_matches('/'), input.control.service);
            let was_auto = !matches!(executor.get(&FileFact, &PathInput::from(down_path)).await?, PathInfo::Present(_));
            let mut commands = Vec::new();
            if !was_managed && input.managed && was_auto {
                let mut auto = AutoInput::new(&input.control.service);
                auto.auto = false;
                auto.source_directory.clone_from(&input.source_directory);
                commands.extend(auto_commands(&auto, connector, options).await?);
            }
            let mut manage = ManageInput::new(&input.control.service);
            manage.managed = input.managed;
            manage.service_directory.clone_from(&input.service_directory);
            manage.source_directory.clone_from(&input.source_directory);
            commands.extend(manage_commands(&manage, connector, options).await?);
            if !was_managed && input.managed {
                let mut wait = WaitRunsvInput::new(&input.control.service);
                wait.service_directory.clone_from(&input.service_directory);
                commands.push(wait_runsv_command(&wait));
            }
            if input.enabled.is_some() && was_managed {
                let mut auto = AutoInput::new(&input.control.service);
                auto.auto = input.enabled == Some(true);
                auto.source_directory.clone_from(&input.source_directory);
                commands.extend(auto_commands(&auto, connector, options).await?);
            }
            if !input.managed {
                return Ok(commands);
            }
            let statuses = executor.get(&RunitStatus, &fact_input).await?;
            commands.extend(control_commands(
                &input.control,
                statuses.get(&input.control.service).copied().flatten(),
                |service, action| service_command(&input.service_directory, service, action),
            ));
            Ok(commands)
        })
    }
}

fn service_command(directory: &str, service: &str, action: &str) -> Command {
    Command::Shell(StringCommand::new([format!("SVDIR={} sv {} {}", quote(directory), quote(action), quote(service))]))
}

fn quote(value: &str) -> String {
    StringCommand::new([CommandBit::quoted(value)]).raw_value()
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_SERVICE_DIRECTORY, WaitRunsv, WaitRunsvInput, service_command};
    use crate::api::Command;
    use crate::operations::Operation;

    fn description(command: Command) -> String {
        match command {
            Command::Shell(value) => value.raw_value(),
            _ => String::new(),
        }
    }

    #[test]
    fn wait_and_control_commands_quote_user_values() {
        let input = WaitRunsvInput::new("my service; reboot");
        assert!(description(WaitRunsv.commands(&input).remove(0)).contains("sv status 'my service; reboot'"));
        assert_eq!(
            description(service_command(DEFAULT_SERVICE_DIRECTORY, "nginx", "start")),
            "SVDIR=/var/service sv start nginx"
        );
    }
}
