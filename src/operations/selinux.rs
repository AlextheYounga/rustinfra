use std::error::Error as StdError;

use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::selinux::{
    FileContext, FileContextInfo, FileContextMapping, SEBoolean, SEPort, SEPortInput, SEPorts,
};
use crate::facts::server::{Which, command_available};
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Boolean {
    On,
    Off,
}

impl Boolean {
    fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BooleanInput {
    pub name: String,
    pub value: Boolean,
    pub persistent: bool,
}

impl BooleanInput {
    #[must_use]
    pub fn new(name: impl Into<String>, value: Boolean) -> Self {
        Self { name: name.into(), value, persistent: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SetBoolean;

impl StatefulOperation for SetBoolean {
    type Input = BooleanInput;
    fn name(&self) -> &'static str {
        "selinux.boolean"
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
            let current = FactExecutor::new(connector, options).get(&SEBoolean, &input.name).await?;
            Ok(boolean_commands(input, &current))
        })
    }
}

fn boolean_commands(input: &BooleanInput, current: &str) -> Vec<Command> {
    if current == input.value.as_str() {
        return Vec::new();
    }
    let mut bits = vec![raw("setsebool")];
    if input.persistent {
        bits.push(raw("-P"));
    }
    bits.extend([quoted(&input.name), raw(input.value.as_str())]);
    vec![shell(bits)]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileContextInput {
    pub path: String,
    pub se_type: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SetFileContext;

impl StatefulOperation for SetFileContext {
    type Input = FileContextInput;
    fn name(&self) -> &'static str {
        "selinux.file_context"
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
            let current = FactExecutor::new(connector, options).get(&FileContext, &input.path).await?;
            Ok(file_context_commands(input, current.as_ref()))
        })
    }
}

fn file_context_commands(input: &FileContextInput, current: Option<&FileContextInfo>) -> Vec<Command> {
    (current.is_none_or(|context| context.se_type != input.se_type))
        .then(|| shell([raw("chcon -t"), quoted(&input.se_type), quoted(&input.path)]))
        .into_iter()
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileContextMappingInput {
    pub target: String,
    pub se_type: Option<String>,
    pub present: bool,
}

impl FileContextMappingInput {
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self { target: target.into(), se_type: None, present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SetFileContextMapping;

impl StatefulOperation for SetFileContextMapping {
    type Input = FileContextMappingInput;
    fn name(&self) -> &'static str {
        "selinux.file_context_mapping"
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
            if input.present && input.se_type.is_none() {
                return Err(Box::new(SelinuxOperationError::MissingType) as Box<dyn StdError + Send + Sync>);
            }
            let current = FactExecutor::new(connector, options).get(&FileContextMapping, &input.target).await?;
            Ok(file_context_mapping_commands(input, current.as_ref()))
        })
    }
}

fn file_context_mapping_commands(input: &FileContextMappingInput, current: Option<&FileContextInfo>) -> Vec<Command> {
    if !input.present {
        return current
            .is_some()
            .then(|| shell([raw("semanage fcontext -d"), quoted(&input.target)]))
            .into_iter()
            .collect();
    }
    let Some(se_type) = input.se_type.as_deref() else { return Vec::new() };
    let option = match current {
        None => "-a",
        Some(context) if context.se_type != se_type => "-m",
        Some(_) => return Vec::new(),
    };
    vec![shell([raw("semanage fcontext"), raw(option), raw("-t"), quoted(se_type), quoted(&input.target)])]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortInput {
    pub protocol: String,
    pub port: u16,
    pub se_type: Option<String>,
    pub present: bool,
}

impl PortInput {
    #[must_use]
    pub fn new(protocol: impl Into<String>, port: u16) -> Self {
        Self { protocol: protocol.into(), port, se_type: None, present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Port;

impl StatefulOperation for Port {
    type Input = PortInput;
    fn name(&self) -> &'static str {
        "selinux.port"
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
            if input.present && input.se_type.is_none() {
                return Err(Box::new(SelinuxOperationError::MissingType) as Box<dyn StdError + Send + Sync>);
            }
            let executor = FactExecutor::new(connector, options);
            let direct = command_available(executor.get(&Which, &"sepolicy".to_owned()).await?.as_deref());
            let current = if direct {
                executor
                    .get(&SEPort, &SEPortInput { protocol: input.protocol.clone(), port: input.port.to_string() })
                    .await?
            } else {
                executor
                    .get(&SEPorts, &())
                    .await?
                    .get(&input.protocol)
                    .and_then(|ports| ports.get(&input.port))
                    .cloned()
                    .unwrap_or_default()
            };
            Ok(port_commands(input, &current))
        })
    }
}

fn port_commands(input: &PortInput, current: &str) -> Vec<Command> {
    let option = if input.present {
        let Some(se_type) = input.se_type.as_deref() else { return Vec::new() };
        if current == se_type {
            return Vec::new();
        }
        if current.is_empty() { "-a" } else { "-m" }
    } else {
        if current.is_empty() {
            return Vec::new();
        }
        "-d"
    };
    let mut bits = vec![raw("semanage port"), raw(option)];
    if input.present {
        bits.extend([raw("-t"), quoted(input.se_type.as_deref().unwrap_or_default())]);
    }
    bits.extend([raw("-p"), quoted(&input.protocol), quoted(&input.port.to_string())]);
    vec![shell(bits)]
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

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SelinuxOperationError {
    #[error("se_type must have a valid value if present is set")]
    MissingType,
}

#[cfg(test)]
mod tests {
    use super::{
        Boolean, BooleanInput, FileContextInfo, FileContextMappingInput, PortInput, boolean_commands,
        file_context_mapping_commands, port_commands,
    };
    use crate::api::Command;

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(value) => Some(value.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn boolean_and_mapping_commands_match_reference_transitions() {
        let mut boolean = BooleanInput::new("httpd`id`", Boolean::On);
        boolean.persistent = true;
        assert_eq!(descriptions(&boolean_commands(&boolean, "off")), ["setsebool -P 'httpd`id`' on"]);
        let current = FileContextInfo {
            user: "system_u".to_owned(),
            role: "object_r".to_owned(),
            se_type: "old_t".to_owned(),
            level: "s0".to_owned(),
        };
        let mapping = FileContextMappingInput {
            target: "/web(/.*)?".to_owned(),
            se_type: Some("httpd_sys_content_t".to_owned()),
            present: true,
        };
        assert_eq!(
            descriptions(&file_context_mapping_commands(&mapping, Some(&current))),
            ["semanage fcontext -m -t httpd_sys_content_t '/web(/.*)?'"]
        );
    }

    #[test]
    fn port_commands_add_modify_remove_and_quote_injection_values() {
        let input = PortInput {
            protocol: "tcp; rm -rf /".to_owned(),
            port: 80,
            se_type: Some("http_port_t; whoami".to_owned()),
            present: true,
        };
        assert_eq!(
            descriptions(&port_commands(&input, "")),
            ["semanage port -a -t 'http_port_t; whoami' -p 'tcp; rm -rf /' 80"]
        );
        assert_eq!(
            descriptions(&port_commands(&input, "other_t")),
            ["semanage port -m -t 'http_port_t; whoami' -p 'tcp; rm -rf /' 80"]
        );
        let remove = PortInput { present: false, ..input };
        assert_eq!(descriptions(&port_commands(&remove, "http_port_t")), ["semanage port -d -p 'tcp; rm -rf /' 80"]);
    }
}
