use std::error::Error;

use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand, remote_temporary_filename};
use crate::connectors::Connector;
use crate::facts::files::{File, FindInFile, FindInFileInput, PathInfo, PathInput};
use crate::facts::server::{Home, HomeInput};
use crate::facts::{FactExecutor, FactOptions};

use super::files::{Directory, DirectoryInput};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyscanInput {
    pub hostname: String,
    pub force: bool,
    pub port: String,
}

impl KeyscanInput {
    pub fn new(hostname: impl Into<String>) -> Self {
        Self { hostname: hostname.into(), force: false, port: "22".to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Keyscan;

impl StatefulOperation for Keyscan {
    type Input = KeyscanInput;
    fn name(&self) -> &'static str {
        "ssh.keyscan"
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
        Box::pin(keyscan_commands(input, connector, options))
    }
}

async fn keyscan_commands(
    input: &KeyscanInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let facts = FactExecutor::new(connector, options);
    let home = facts.get(&Home, &HomeInput::default()).await?.ok_or(SshOperationError::MissingHome)?;
    let ssh_directory = format!("{home}/.ssh");
    let mut directory = DirectoryInput::new(&ssh_directory);
    directory.mode = Some(700);
    let mut commands = Directory.commands(&directory, connector, options).await?;
    let known_hosts = format!("{ssh_directory}/known_hosts");
    let lines = facts
        .get(
            &FindInFile,
            &FindInFileInput {
                path: known_hosts.clone(),
                pattern: input.hostname.clone(),
                interpolate_variables: false,
                extended_regex: false,
            },
        )
        .await?;
    commands.extend(keyscan_plan(input, &known_hosts, lines.as_deref().unwrap_or_default()));
    Ok(commands)
}

fn keyscan_plan(input: &KeyscanInput, known_hosts: &str, lines: &[String]) -> Vec<Command> {
    let known_host =
        if input.port == "22" { input.hostname.clone() } else { format!("[{}]:{}", input.hostname, input.port) };
    let present = lines.iter().any(|line| known_host_line_matches(line, &known_host));
    let scan = shell([
        raw("ssh-keyscan"),
        raw("-p"),
        quoted(&input.port),
        quoted(&input.hostname),
        raw(">>"),
        quoted(known_hosts),
    ]);
    if !present {
        vec![scan]
    } else if input.force {
        vec![shell([raw("ssh-keygen"), raw("-R"), quoted(&known_host)]), scan]
    } else {
        Vec::new()
    }
}

fn known_host_line_matches(line: &str, expected: &str) -> bool {
    let mut fields = line.split_whitespace();
    let Some(mut hosts) = fields.next() else {
        return false;
    };
    if hosts.starts_with('#') {
        return false;
    }
    if hosts.starts_with('@') {
        let Some(value) = fields.next() else {
            return false;
        };
        hosts = value;
    }
    fields.next().is_some() && hosts.split(',').any(|host| host == expected)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandInput {
    pub hostname: String,
    pub command: String,
    pub user: Option<String>,
    pub port: String,
}
impl CommandInput {
    pub fn new(hostname: impl Into<String>, command: impl Into<String>) -> Self {
        Self { hostname: hostname.into(), command: command.into(), user: None, port: "22".to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SshCommand;
impl Operation for SshCommand {
    type Input = CommandInput;
    fn name(&self) -> &'static str {
        "ssh.command"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![ssh_command(input)]
    }
}

fn ssh_command(input: &CommandInput) -> Command {
    shell([
        raw("ssh"),
        raw("-p"),
        quoted(&input.port),
        quoted(connection_target(&input.hostname, input.user.as_deref())),
        quoted(&input.command),
    ])
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadInput {
    pub hostname: String,
    pub filename: String,
    pub remote_filename: Option<String>,
    pub port: String,
    pub user: Option<String>,
    pub use_remote_sudo: bool,
    pub ssh_keyscan: bool,
}
impl UploadInput {
    pub fn new(hostname: impl Into<String>, filename: impl Into<String>) -> Self {
        Self {
            hostname: hostname.into(),
            filename: filename.into(),
            remote_filename: None,
            port: "22".to_owned(),
            user: None,
            use_remote_sudo: false,
            ssh_keyscan: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Upload;
impl StatefulOperation for Upload {
    type Input = UploadInput;
    fn name(&self) -> &'static str {
        "ssh.upload"
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
            let mut commands = if input.ssh_keyscan {
                keyscan_commands(
                    &KeyscanInput { hostname: input.hostname.clone(), force: false, port: input.port.clone() },
                    connector,
                    options,
                )
                .await?
            } else {
                Vec::new()
            };
            commands.extend(upload_commands(input, &options.run.remote_temp_directory));
            Ok(commands)
        })
    }
}

fn upload_commands(input: &UploadInput, temporary_directory: &str) -> Vec<Command> {
    let destination = input.remote_filename.as_deref().unwrap_or(&input.filename);
    let target = connection_target(&input.hostname, input.user.as_deref());
    if !input.use_remote_sudo {
        return vec![scp_upload(&input.port, &input.filename, &target, destination)];
    }
    let temporary = remote_temporary_filename(temporary_directory, destination.as_bytes());
    vec![
        scp_upload(&input.port, &input.filename, &target, &temporary),
        ssh_command(&CommandInput {
            hostname: input.hostname.clone(),
            command: StringCommand::new([raw("sudo"), raw("mv"), quoted(&temporary), quoted(destination)]).raw_value(),
            user: input.user.clone(),
            port: input.port.clone(),
        }),
    ]
}

fn scp_upload(port: &str, filename: &str, target: &str, destination: &str) -> Command {
    let remote = StringCommand::with_separator([quoted(target), raw(":"), quoted(destination)], "");
    shell([raw("scp"), raw("-P"), quoted(port), quoted(filename), remote.into()])
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadInput {
    pub hostname: String,
    pub filename: String,
    pub local_filename: Option<String>,
    pub force: bool,
    pub port: String,
    pub user: Option<String>,
    pub ssh_keyscan: bool,
}
impl DownloadInput {
    pub fn new(hostname: impl Into<String>, filename: impl Into<String>) -> Self {
        Self {
            hostname: hostname.into(),
            filename: filename.into(),
            local_filename: None,
            force: false,
            port: "22".to_owned(),
            user: None,
            ssh_keyscan: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Download;
impl StatefulOperation for Download {
    type Input = DownloadInput;
    fn name(&self) -> &'static str {
        "ssh.download"
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
            let local = input.local_filename.as_deref().unwrap_or(&input.filename);
            match FactExecutor::new(connector, options).get(&File, &PathInput::from(local)).await? {
                PathInfo::DifferentType => {
                    return Err(Box::new(SshOperationError::DownloadTargetNotFile(local.to_owned()))
                        as Box<dyn Error + Send + Sync>);
                }
                PathInfo::Present(_) if !input.force => return Ok(Vec::new()),
                PathInfo::Missing | PathInfo::Present(_) => {}
            }
            let mut commands = if input.ssh_keyscan {
                keyscan_commands(
                    &KeyscanInput { hostname: input.hostname.clone(), force: false, port: input.port.clone() },
                    connector,
                    options,
                )
                .await?
            } else {
                Vec::new()
            };
            let source = StringCommand::with_separator(
                [quoted(connection_target(&input.hostname, input.user.as_deref())), raw(":"), quoted(&input.filename)],
                "",
            );
            commands.push(shell([raw("scp"), raw("-P"), quoted(&input.port), source.into(), quoted(local)]));
            Ok(commands)
        })
    }
}

#[derive(Debug, Error)]
enum SshOperationError {
    #[error("could not determine the remote home directory")]
    MissingHome,
    #[error("Local destination {0} already exists and is not a file")]
    DownloadTargetNotFile(String),
}

fn connection_target(hostname: &str, user: Option<&str>) -> String {
    user.map_or_else(|| hostname.to_owned(), |user| format!("{user}@{hostname}"))
}
fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: impl Into<String>) -> CommandBit {
    CommandBit::quoted(value.into())
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests;
