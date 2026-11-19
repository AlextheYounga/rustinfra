use std::error::Error;

use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{Directory as DirectoryFact, File as FileFact, PathInfo, PathInput};
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInput {
    pub path: String,
    pub present: bool,
    pub user: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u16>,
    pub touch: bool,
    pub create_remote_dir: bool,
}

impl FileInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            present: true,
            user: None,
            group: None,
            mode: None,
            touch: false,
            create_remote_dir: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct File;

impl StatefulOperation for File {
    type Input = FileInput;

    fn name(&self) -> &'static str {
        "files.file"
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
            let info =
                FactExecutor::new(connector, fact_options).get(&FileFact, &PathInput::from(input.path.clone())).await?;
            file_commands(input, info).map_err(boxed)
        })
    }
}

fn file_commands(input: &FileInput, info: PathInfo) -> Result<Vec<Command>, FilesOperationError> {
    match info {
        PathInfo::DifferentType => {
            return Err(FilesOperationError::WrongType { path: input.path.clone(), expected: "file" });
        }
        PathInfo::Missing if !input.present => return Ok(Vec::new()),
        PathInfo::Present(_) if !input.present => return Ok(vec![shell([raw("rm"), raw("-f"), quoted(&input.path)])]),
        _ => {}
    }

    let mut commands = Vec::new();
    match info {
        PathInfo::Missing => {
            if input.create_remote_dir
                && let Some(parent) = remote_parent(&input.path)
            {
                commands.push(shell([raw("mkdir"), raw("-p"), quoted(parent)]));
            }
            commands.push(shell([raw("touch"), quoted(&input.path)]));
            add_metadata_commands(&mut commands, &input.path, RequestedMetadata::from_file(input));
        }
        PathInfo::Present(current) => {
            if input.touch {
                commands.push(shell([raw("touch"), quoted(&input.path)]));
            }
            if input.mode.is_some_and(|mode| mode != current.mode) {
                commands.push(chmod(&input.path, input.mode, false));
            }
            if input.user.as_deref().is_some_and(|user| user != current.user)
                || input.group.as_deref().is_some_and(|group| group != current.group)
            {
                if let Some(command) = chown(&input.path, input.user.as_deref(), input.group.as_deref(), false) {
                    commands.push(command);
                }
            }
        }
        PathInfo::DifferentType => {}
    }
    Ok(commands)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryInput {
    pub path: String,
    pub present: bool,
    pub user: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u16>,
    pub recursive: bool,
}

impl DirectoryInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), present: true, user: None, group: None, mode: None, recursive: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Directory;

impl StatefulOperation for Directory {
    type Input = DirectoryInput;

    fn name(&self) -> &'static str {
        "files.directory"
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
            let info = FactExecutor::new(connector, fact_options)
                .get(&DirectoryFact, &PathInput::from(input.path.clone()))
                .await?;
            directory_commands(input, info).map_err(boxed)
        })
    }
}

fn directory_commands(input: &DirectoryInput, info: PathInfo) -> Result<Vec<Command>, FilesOperationError> {
    match info {
        PathInfo::DifferentType => {
            return Err(FilesOperationError::WrongType { path: input.path.clone(), expected: "directory" });
        }
        PathInfo::Missing if !input.present => return Ok(Vec::new()),
        PathInfo::Present(_) if !input.present => return Ok(vec![shell([raw("rm"), raw("-rf"), quoted(&input.path)])]),
        _ => {}
    }

    let mut commands = Vec::new();
    match info {
        PathInfo::Missing => {
            commands.push(shell([raw("mkdir"), raw("-p"), quoted(&input.path)]));
            add_metadata_commands(&mut commands, &input.path, RequestedMetadata::from_directory(input));
        }
        PathInfo::Present(current) => {
            if input.mode.is_some_and(|mode| mode != current.mode) {
                commands.push(chmod(&input.path, input.mode, input.recursive));
            }
            if input.user.as_deref().is_some_and(|user| user != current.user)
                || input.group.as_deref().is_some_and(|group| group != current.group)
            {
                if let Some(command) =
                    chown(&input.path, input.user.as_deref(), input.group.as_deref(), input.recursive)
                {
                    commands.push(command);
                }
            }
        }
        PathInfo::DifferentType => {}
    }
    Ok(commands)
}

#[derive(Clone, Copy)]
struct RequestedMetadata<'a> {
    mode: Option<u16>,
    user: Option<&'a str>,
    group: Option<&'a str>,
    recursive: bool,
}

impl<'a> RequestedMetadata<'a> {
    fn from_file(input: &'a FileInput) -> Self {
        Self { mode: input.mode, user: input.user.as_deref(), group: input.group.as_deref(), recursive: false }
    }

    fn from_directory(input: &'a DirectoryInput) -> Self {
        Self {
            mode: input.mode,
            user: input.user.as_deref(),
            group: input.group.as_deref(),
            recursive: input.recursive,
        }
    }
}

fn add_metadata_commands(commands: &mut Vec<Command>, path: &str, metadata: RequestedMetadata<'_>) {
    if metadata.mode.is_some() {
        commands.push(chmod(path, metadata.mode, metadata.recursive));
    }
    if metadata.user.is_some() || metadata.group.is_some() {
        if let Some(command) = chown(path, metadata.user, metadata.group, metadata.recursive) {
            commands.push(command);
        }
    }
}

fn chmod(path: &str, mode: Option<u16>, recursive: bool) -> Command {
    let mut bits = vec![raw("chmod")];
    if recursive {
        bits.push(raw("-R"));
    }
    if let Some(mode) = mode {
        bits.push(raw(mode.to_string()));
    }
    bits.push(quoted(path));
    shell(bits)
}

fn chown(path: &str, user: Option<&str>, group: Option<&str>, recursive: bool) -> Option<Command> {
    let (program, owner) = match (user, group) {
        (Some(user), Some(group)) => ("chown", format!("{user}:{group}")),
        (Some(user), None) => ("chown", user.to_owned()),
        (None, Some(group)) => ("chgrp", group.to_owned()),
        (None, None) => return None,
    };
    let mut bits = vec![raw(program)];
    if recursive {
        bits.push(raw("-R"));
    }
    bits.extend([CommandBit::quoted(owner), quoted(path)]);
    Some(shell(bits))
}

fn remote_parent(path: &str) -> Option<&str> {
    let (parent, _) = path.rsplit_once('/')?;
    if parent.is_empty() { Some("/") } else { Some(parent) }
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

fn boxed(error: FilesOperationError) -> Box<dyn Error + Send + Sync> {
    Box::new(error)
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FilesOperationError {
    #[error("path {path} exists and is not a {expected}")]
    WrongType { path: String, expected: &'static str },
}

#[cfg(test)]
mod tests {
    use super::{DirectoryInput, File, FileInput, directory_commands, file_commands};
    use crate::api::Command;
    use crate::connectors::{LocalConnector, RunOptions};
    use crate::facts::FactOptions;
    use crate::facts::files::{FileInfo, PathInfo};
    use crate::operations::{CommandExecutor, StatefulOperation};

    fn current(mode: u16) -> PathInfo {
        PathInfo::Present(FileInfo {
            user: "root".to_owned(),
            group: "root".to_owned(),
            mode,
            accessed_at: None,
            modified_at: None,
            changed_at: None,
            size: 0,
            link_target: None,
        })
    }

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
    fn missing_file_creates_parent_and_applies_requested_metadata() {
        let mut input = FileInput::new("/tmp/my dir/file");
        input.mode = Some(600);
        input.user = Some("app user".to_owned());

        assert_eq!(
            descriptions(&file_commands(&input, PathInfo::Missing).unwrap()),
            [
                "mkdir -p '/tmp/my dir'",
                "touch '/tmp/my dir/file'",
                "chmod 600 '/tmp/my dir/file'",
                "chown 'app user' '/tmp/my dir/file'"
            ]
        );
    }

    #[test]
    fn matching_directory_is_a_noop_and_recursive_metadata_is_updated_when_different() {
        let mut input = DirectoryInput::new("/srv/app");
        assert!(directory_commands(&input, current(755)).unwrap().is_empty());

        input.mode = Some(750);
        input.recursive = true;
        assert_eq!(descriptions(&directory_commands(&input, current(755)).unwrap()), ["chmod -R 750 /srv/app"]);
    }

    #[tokio::test]
    async fn file_rechecks_current_state_and_becomes_a_noop_after_convergence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("created");
        let input = FileInput::new(path.to_string_lossy());
        let fact_options = FactOptions::default();

        let first = File.commands(&input, &LocalConnector, &fact_options).await.unwrap();
        CommandExecutor.execute(&LocalConnector, &first, &RunOptions::default()).await.unwrap();
        let second = File.commands(&input, &LocalConnector, &fact_options).await.unwrap();

        assert!(!first.is_empty());
        assert!(second.is_empty());
    }
}
