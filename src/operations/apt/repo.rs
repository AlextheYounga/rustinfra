use std::error::Error;

use thiserror::Error;

use crate::api::{Command, ContentUploadCommand, sha1_hex};
use crate::connectors::Connector;
use crate::facts::apt::{AptRepo, AptSources, parse_apt_repo};
use crate::facts::files::{PathInput, Sha1File};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::{File, FileInput, Line, LineInput};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepoInput {
    pub source: String,
    pub present: bool,
    pub filename: Option<String>,
}

impl RepoInput {
    #[must_use]
    pub fn new(source: impl Into<String>) -> Self {
        Self { source: source.into(), present: true, filename: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Repo;

impl StatefulOperation for Repo {
    type Input = RepoInput;

    fn name(&self) -> &'static str {
        "apt.repo"
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
            let repositories = FactExecutor::new(connector, fact_options).get(&AptSources, &()).await?;
            let Some(line) = repo_line_input(input, &repositories)
                .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)?
            else {
                return Ok(Vec::new());
            };
            Line.commands(&line, connector, fact_options).await
        })
    }
}

fn repo_line_input(input: &RepoInput, repositories: &[AptRepo]) -> Result<Option<LineInput>, AptRepoError> {
    let parsed =
        parse_apt_repo(&input.source).ok_or_else(|| AptRepoError::InvalidSource { value: input.source.clone() })?;
    if repositories.contains(&parsed) == input.present {
        return Ok(None);
    }
    let path = input.filename.as_ref().map_or_else(
        || "/etc/apt/sources.list".to_owned(),
        |filename| format!("/etc/apt/sources.list.d/{filename}.list"),
    );
    let mut line = LineInput::new(path, &input.source);
    line.present = input.present;
    line.escape_regex_characters = true;
    Ok(Some(line))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcesFileInput {
    pub filename: String,
    pub repository_types: Vec<String>,
    pub uris: Vec<String>,
    pub suites: Vec<String>,
    pub components: Vec<String>,
    pub architectures: Vec<String>,
    pub signed_by: Option<String>,
    pub present: bool,
}

impl SourcesFileInput {
    #[must_use]
    pub fn new(
        filename: impl Into<String>,
        uris: impl IntoIterator<Item = impl Into<String>>,
        suites: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            filename: filename.into(),
            repository_types: vec!["deb".to_owned()],
            uris: uris.into_iter().map(Into::into).collect(),
            suites: suites.into_iter().map(Into::into).collect(),
            components: Vec::new(),
            architectures: Vec::new(),
            signed_by: None,
            present: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SourcesFile;

impl StatefulOperation for SourcesFile {
    type Input = SourcesFileInput;

    fn name(&self) -> &'static str {
        "apt.sources_file"
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
            let path = format!("/etc/apt/sources.list.d/{}.sources", input.filename);
            if !input.present {
                let mut file = FileInput::new(path);
                file.present = false;
                return File.commands(&file, connector, fact_options).await;
            }
            validate_sources_file(input)?;
            let content = sources_file_content(input);
            let remote_hash =
                FactExecutor::new(connector, fact_options).get(&Sha1File, &PathInput::from(path.clone())).await?;
            if remote_hash.as_deref() == Some(&sha1_hex(content.as_bytes())) {
                return Ok(Vec::new());
            }
            Ok(vec![Command::UploadContent(ContentUploadCommand {
                content: content.into_bytes(),
                destination: path,
                remote_temporary_filename: None,
            })])
        })
    }
}

fn validate_sources_file(input: &SourcesFileInput) -> Result<(), Box<dyn Error + Send + Sync>> {
    if input.repository_types.is_empty() || input.uris.is_empty() || input.suites.is_empty() {
        Err(Box::new(AptRepoError::MissingRequiredSourcesField))
    } else if input.repository_types.iter().any(|value| !matches!(value.as_str(), "deb" | "deb-src")) {
        Err(Box::new(AptRepoError::InvalidRepositoryType))
    } else {
        Ok(())
    }
}

fn sources_file_content(input: &SourcesFileInput) -> String {
    let mut lines = vec![
        format!("Types: {}", input.repository_types.join(" ")),
        format!("URIs: {}", input.uris.join(" ")),
        format!("Suites: {}", input.suites.join(" ")),
    ];
    if !input.components.is_empty() {
        lines.push(format!("Components: {}", input.components.join(" ")));
    }
    if !input.architectures.is_empty() {
        lines.push(format!("Architectures: {}", input.architectures.join(" ")));
    }
    if let Some(signed_by) = &input.signed_by {
        lines.push(format!("Signed-By: {signed_by}"));
    }
    lines.push(String::new());
    lines.join("\n")
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AptRepoError {
    #[error("invalid apt repository source: {value}")]
    InvalidSource { value: String },
    #[error("apt sources files require at least one type, URI, and suite")]
    MissingRequiredSourcesField,
    #[error("apt repository types must be deb or deb-src")]
    InvalidRepositoryType,
}

#[cfg(test)]
mod tests {
    use super::{RepoInput, SourcesFileInput, repo_line_input, sources_file_content};
    use crate::facts::apt::parse_apt_repo;

    #[test]
    fn renders_modern_deb822_sources_with_optional_security_fields() {
        let mut input = SourcesFileInput::new("docker", ["https://download.docker.com/linux/debian"], ["bookworm"]);
        input.components = vec!["stable".to_owned()];
        input.architectures = vec!["amd64".to_owned()];
        input.signed_by = Some("/etc/apt/keyrings/docker.gpg".to_owned());

        assert_eq!(
            sources_file_content(&input),
            "Types: deb\nURIs: https://download.docker.com/linux/debian\nSuites: bookworm\nComponents: stable\nArchitectures: amd64\nSigned-By: /etc/apt/keyrings/docker.gpg\n"
        );
    }

    #[test]
    fn legacy_repo_only_edits_the_selected_file_when_state_differs() {
        let source = "deb http://archive.example/ubuntu stable main";
        let mut input = RepoInput::new(source);
        input.filename = Some("vendor".to_owned());
        let line = repo_line_input(&input, &[]).unwrap().unwrap();
        assert_eq!(line.path, "/etc/apt/sources.list.d/vendor.list");
        assert!(line.present);
        assert!(line.escape_regex_characters);

        let repositories = [parse_apt_repo(source).unwrap()];
        assert!(repo_line_input(&input, &repositories).unwrap().is_none());
        input.present = false;
        assert!(!repo_line_input(&input, &repositories).unwrap().unwrap().present);
    }
}
