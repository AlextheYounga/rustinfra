use std::error::Error;
use std::path::PathBuf;

use thiserror::Error;

use crate::api::{Command, CommandBit, FileUploadCommand, StringCommand, remote_temporary_filename};
use crate::connectors::Connector;
use crate::facts::files::{Directory, File, PathInfo, PathInput};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnarchiveInput {
    pub source: PathBuf,
    pub destination: String,
    pub remote_source: bool,
    pub creates: Option<String>,
    pub extra_options: Vec<String>,
    pub user: Option<String>,
    pub group: Option<String>,
}

impl UnarchiveInput {
    #[must_use]
    pub fn new(source: impl Into<PathBuf>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            remote_source: false,
            creates: None,
            extra_options: Vec::new(),
            user: None,
            group: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Unarchive;

impl StatefulOperation for Unarchive {
    type Input = UnarchiveInput;

    fn name(&self) -> &'static str {
        "files.unarchive"
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
            let facts = FactExecutor::new(connector, fact_options);
            if let Some(creates) = &input.creates {
                let created = facts.get(&File, &PathInput::from(creates.clone())).await?;
                if matches!(created, PathInfo::Present(_)) {
                    return Ok(Vec::new());
                }
            }
            let destination = facts.get(&Directory, &PathInput::from(input.destination.clone())).await?;
            if !matches!(destination, PathInfo::Present(_)) {
                return Err(Box::new(UnarchiveError::DestinationNotDirectory { path: input.destination.clone() })
                    as Box<dyn Error + Send + Sync>);
            }
            if input.remote_source {
                let source = input.source.to_string_lossy().into_owned();
                let info = facts.get(&File, &PathInput::from(source.clone())).await?;
                if !matches!(info, PathInfo::Present(_)) {
                    return Err(
                        Box::new(UnarchiveError::RemoteSourceNotFile { path: source }) as Box<dyn Error + Send + Sync>
                    );
                }
            }
            unarchive_commands(input, &fact_options.run.remote_temp_directory)
                .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
        })
    }
}

fn unarchive_commands(input: &UnarchiveInput, remote_temp_directory: &str) -> Result<Vec<Command>, UnarchiveError> {
    let source = input.source.to_string_lossy();
    let (program, flags) =
        archive_format(&source).ok_or_else(|| UnarchiveError::UnsupportedFormat { path: source.to_string() })?;
    let temporary = remote_temporary_filename(remote_temp_directory, source.as_bytes());
    let archive = if input.remote_source { source.to_string() } else { temporary.clone() };
    let mut commands = Vec::new();
    if !input.remote_source {
        commands.push(Command::Upload(FileUploadCommand {
            source: input.source.clone(),
            destination: temporary.clone(),
            remote_temporary_filename: None,
        }));
    }
    let mut bits = vec![raw(program)];
    bits.extend(flags.iter().map(|flag| raw(*flag)));
    bits.extend(input.extra_options.iter().map(|option| quoted(option)));
    if program == "tar" {
        bits.extend([raw("-f"), quoted(&archive), raw("-C"), quoted(&input.destination)]);
    } else {
        bits.extend([quoted(&archive), raw("-d"), quoted(&input.destination)]);
    }
    commands.push(shell(bits));
    if !input.remote_source {
        commands.push(shell([raw("rm"), raw("-f"), quoted(&temporary)]));
    }
    if let Some(owner) = owner(input.user.as_deref(), input.group.as_deref()) {
        commands.push(shell([
            raw(if input.user.is_some() { "chown" } else { "chgrp" }),
            raw("-R"),
            quoted(&owner),
            quoted(&input.destination),
        ]));
    }
    Ok(commands)
}

fn archive_format(source: &str) -> Option<(&'static str, &'static [&'static str])> {
    const FORMATS: &[(&str, &str, &[&str])] = &[
        (".tar.gz", "tar", &["-xz"]),
        (".tgz", "tar", &["-xz"]),
        (".tar.bz2", "tar", &["-xj"]),
        (".tbz2", "tar", &["-xj"]),
        (".tar.xz", "tar", &["-xJ"]),
        (".txz", "tar", &["-xJ"]),
        (".tar.zst", "tar", &["-x", "--zstd"]),
        (".tar", "tar", &["-x"]),
        (".zip", "unzip", &["-o"]),
    ];
    let source = source.to_ascii_lowercase();
    FORMATS.iter().find(|(extension, _, _)| source.ends_with(extension)).map(|(_, tool, flags)| (*tool, *flags))
}

fn owner(user: Option<&str>, group: Option<&str>) -> Option<String> {
    match (user, group) {
        (Some(user), Some(group)) => Some(format!("{user}:{group}")),
        (Some(user), None) => Some(user.to_owned()),
        (None, Some(group)) => Some(group.to_owned()),
        (None, None) => None,
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

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum UnarchiveError {
    #[error("destination {path} is not an existing directory")]
    DestinationNotDirectory { path: String },
    #[error("remote archive {path} does not exist or is not a file")]
    RemoteSourceNotFile { path: String },
    #[error("unsupported archive format for {path}")]
    UnsupportedFormat { path: String },
}

#[cfg(test)]
mod tests {
    use super::{UnarchiveInput, unarchive_commands};
    use crate::api::Command;

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .map(|command| match command {
                Command::Shell(command) => command.raw_value(),
                Command::Upload(upload) => format!("upload {} {}", upload.source.display(), upload.destination),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn local_tar_is_uploaded_extracted_cleaned_and_chowned() {
        let mut input = UnarchiveInput::new("release file.tar.gz", "/srv/app");
        input.extra_options = vec!["--strip-components=1".to_owned()];
        input.user = Some("app user".to_owned());
        let commands = descriptions(&unarchive_commands(&input, "/tmp").unwrap());

        assert_eq!(commands.len(), 4);
        assert!(commands[0].starts_with("upload release file.tar.gz /tmp/pyinfra-"));
        assert!(commands[1].starts_with("tar -xz --strip-components=1 -f /tmp/pyinfra-"));
        assert!(commands[1].ends_with("-C /srv/app"));
        assert!(commands[2].starts_with("rm -f /tmp/pyinfra-"));
        assert_eq!(commands[3], "chown -R 'app user' /srv/app");
    }

    #[test]
    fn remote_zip_uses_unzip_without_transfer_or_cleanup() {
        let mut input = UnarchiveInput::new("/tmp/release.zip", "/srv/app");
        input.remote_source = true;
        assert_eq!(
            descriptions(&unarchive_commands(&input, "/tmp").unwrap()),
            ["unzip -o /tmp/release.zip -d /srv/app"]
        );
    }
}
