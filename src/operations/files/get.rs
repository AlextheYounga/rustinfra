use std::io::ErrorKind;
use std::path::PathBuf;

use tokio::fs;

use crate::api::{Command, FileDownloadCommand, sha1_hex};
use crate::connectors::Connector;
use crate::facts::files::{PathInput, Sha1File};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GetInput {
    pub source: String,
    pub destination: PathBuf,
    pub create_local_dir: bool,
    pub force: bool,
}

impl GetInput {
    #[must_use]
    pub fn new(source: impl Into<String>, destination: impl Into<PathBuf>) -> Self {
        Self { source: source.into(), destination: destination.into(), create_local_dir: false, force: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Get;

impl StatefulOperation for Get {
    type Input = GetInput;

    fn name(&self) -> &'static str {
        "files.get"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            if input.create_local_dir
                && let Some(parent) = input.destination.parent()
            {
                fs::create_dir_all(parent).await?;
            }
            let remote_hash = FactExecutor::new(connector, fact_options)
                .get(&Sha1File, &PathInput::from(input.source.clone()))
                .await?;
            let local_hash = match fs::read(&input.destination).await {
                Ok(contents) => Some(sha1_hex(&contents)),
                Err(error) if error.kind() == ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            if !input.force && remote_hash.is_some() && remote_hash == local_hash {
                return Ok(Vec::new());
            }
            let temporary = format!(
                "{}/pyinfra-{}",
                fact_options.run.remote_temp_directory.trim_end_matches('/'),
                sha1_hex(input.destination.to_string_lossy().as_bytes())
            );
            Ok(vec![Command::Download(FileDownloadCommand {
                source: input.source.clone(),
                destination: input.destination.clone(),
                remote_temporary_filename: Some(temporary),
            })])
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Get, GetInput};
    use crate::connectors::{LocalConnector, RunOptions};
    use crate::facts::FactOptions;
    use crate::operations::{CommandExecutor, StatefulOperation};

    #[tokio::test]
    async fn get_downloads_changed_content_and_then_becomes_a_noop() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("remote");
        let destination = directory.path().join("local/destination");
        fs::write(&source, "content").unwrap();
        let mut input = GetInput::new(source.to_string_lossy(), &destination);
        input.create_local_dir = true;
        let options = FactOptions::default();

        let first = Get.commands(&input, &LocalConnector, &options).await.unwrap();
        CommandExecutor.execute(&LocalConnector, &first, &RunOptions::default()).await.unwrap();
        let second = Get.commands(&input, &LocalConnector, &options).await.unwrap();

        assert_eq!(fs::read_to_string(destination).unwrap(), "content");
        assert!(second.is_empty());
    }
}
