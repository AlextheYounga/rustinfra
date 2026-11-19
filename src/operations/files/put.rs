use std::path::PathBuf;

use tokio::fs as async_fs;

use crate::api::{Command, FileUploadCommand, sha1_hex};
use crate::connectors::Connector;
use crate::facts::files::{File as FileFact, FileInfo, PathInfo, PathInput, Sha1File};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

use super::{RequestedMetadata, add_metadata_commands, quoted, raw, remote_parent, shell};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PutInput {
    pub source: PathBuf,
    pub destination: String,
    pub user: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u16>,
    pub create_remote_dir: bool,
}

impl PutInput {
    #[must_use]
    pub fn new(source: impl Into<PathBuf>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            user: None,
            group: None,
            mode: None,
            create_remote_dir: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Put;

impl StatefulOperation for Put {
    type Input = PutInput;

    fn name(&self) -> &'static str {
        "files.put"
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
            let local_data = async_fs::read(&input.source).await?;
            let local_hash = sha1_hex(&local_data);
            let remote_hash = FactExecutor::new(connector, fact_options)
                .get(&Sha1File, &PathInput::from(input.destination.clone()))
                .await?;
            let remote_info = if input.user.is_some() || input.group.is_some() || input.mode.is_some() {
                match FactExecutor::new(connector, fact_options)
                    .get(&FileFact, &PathInput::from(input.destination.clone()))
                    .await?
                {
                    PathInfo::Present(info) => Some(info),
                    PathInfo::Missing | PathInfo::DifferentType => None,
                }
            } else {
                None
            };
            Ok(put_commands(input, remote_hash.as_deref(), &local_hash, remote_info.as_ref()))
        })
    }
}

fn put_commands(
    input: &PutInput,
    remote_hash: Option<&str>,
    local_hash: &str,
    remote_info: Option<&FileInfo>,
) -> Vec<Command> {
    if remote_hash == Some(local_hash) && input.user.is_none() && input.group.is_none() && input.mode.is_none() {
        return Vec::new();
    }

    let mut commands = Vec::new();
    if remote_hash != Some(local_hash) {
        if input.create_remote_dir
            && let Some(parent) = remote_parent(&input.destination)
        {
            commands.push(shell([raw("mkdir"), raw("-p"), quoted(parent)]));
        }
        commands.push(Command::Upload(FileUploadCommand {
            source: input.source.clone(),
            destination: input.destination.clone(),
            remote_temporary_filename: None,
        }));
    }
    let content_changed = remote_hash != Some(local_hash);
    let metadata = RequestedMetadata {
        mode: input.mode.filter(|mode| content_changed || remote_info.is_none_or(|info| info.mode != *mode)),
        user: input.user.as_deref().filter(|user| content_changed || remote_info.is_none_or(|info| info.user != *user)),
        group: input
            .group
            .as_deref()
            .filter(|group| content_changed || remote_info.is_none_or(|info| info.group != *group)),
        recursive: false,
    };
    add_metadata_commands(&mut commands, &input.destination, metadata);
    commands
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Put, PutInput, put_commands};
    use crate::api::{Command, sha1_hex};
    use crate::connectors::{LocalConnector, RunOptions};
    use crate::facts::FactOptions;
    use crate::operations::{CommandExecutor, StatefulOperation};

    #[test]
    fn matching_content_is_a_noop_and_changed_content_uploads_with_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        fs::write(&source, "hello").unwrap();
        let hash = sha1_hex(b"hello");
        let mut input = PutInput::new(&source, "/srv/app/config file");

        assert!(put_commands(&input, Some(&hash), &hash, None).is_empty());
        input.mode = Some(640);
        let commands = put_commands(&input, None, &hash, None);
        assert!(matches!(&commands[0], Command::Shell(command) if command.raw_value() == "mkdir -p /srv/app"));
        assert!(
            matches!(&commands[1], Command::Upload(upload) if upload.source == source && upload.destination == "/srv/app/config file")
        );
        assert!(
            matches!(&commands[2], Command::Shell(command) if command.raw_value() == "chmod 640 '/srv/app/config file'")
        );
    }

    #[tokio::test]
    async fn put_transfers_changed_content_and_rechecks_the_remote_hash() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("remote/destination");
        fs::write(&source, "content").unwrap();
        let input = PutInput::new(&source, destination.to_string_lossy());
        let options = FactOptions::default();

        let first = Put.commands(&input, &LocalConnector, &options).await.unwrap();
        CommandExecutor.execute(&LocalConnector, &first, &RunOptions::default()).await.unwrap();
        let second = Put.commands(&input, &LocalConnector, &options).await.unwrap();

        assert_eq!(fs::read_to_string(destination).unwrap(), "content");
        assert!(second.is_empty());
    }
}
