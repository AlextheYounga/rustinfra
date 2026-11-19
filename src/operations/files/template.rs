use std::path::PathBuf;

use minijinja::{Environment, UndefinedBehavior, Value as TemplateValue};
use serde_json::{Map, Value};
use tokio::fs;

use crate::api::{Command, ContentUploadCommand, sha1_hex};
use crate::connectors::Connector;
use crate::facts::files::{File as FileFact, FileInfo, PathInfo, PathInput, Sha1File};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

use super::{RequestedMetadata, add_metadata_commands, quoted, raw, remote_parent, shell};

#[derive(Clone, Debug, PartialEq)]
pub struct TemplateInput {
    pub source: PathBuf,
    pub destination: String,
    pub data: Map<String, Value>,
    pub user: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u16>,
    pub create_remote_dir: bool,
}

impl TemplateInput {
    #[must_use]
    pub fn new(source: impl Into<PathBuf>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            data: Map::new(),
            user: None,
            group: None,
            mode: None,
            create_remote_dir: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Template;

impl StatefulOperation for Template {
    type Input = TemplateInput;

    fn name(&self) -> &'static str {
        "files.template"
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
            let source = fs::read_to_string(&input.source).await?;
            let rendered = render(&source, &input.data)?;
            let local_hash = sha1_hex(rendered.as_bytes());
            let facts = FactExecutor::new(connector, fact_options);
            let remote_hash = facts.get(&Sha1File, &PathInput::from(input.destination.clone())).await?;
            let remote_info = if input.user.is_some() || input.group.is_some() || input.mode.is_some() {
                match facts.get(&FileFact, &PathInput::from(input.destination.clone())).await? {
                    PathInfo::Present(info) => Some(info),
                    PathInfo::Missing | PathInfo::DifferentType => None,
                }
            } else {
                None
            };
            Ok(template_commands(
                input,
                rendered,
                &local_hash,
                RemoteTemplateState { hash: remote_hash.as_deref(), info: remote_info.as_ref() },
            ))
        })
    }
}

fn render(source: &str, data: &Map<String, Value>) -> Result<String, minijinja::Error> {
    let mut environment = Environment::new();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    environment.add_template("template", source)?;
    environment
        .get_template("template")?
        .render(TemplateValue::from_pairs(data.iter().map(|(key, value)| (key.clone(), template_value(value)))))
}

fn template_value(value: &Value) -> TemplateValue {
    match value {
        Value::Null => TemplateValue::from(()),
        Value::Bool(value) => TemplateValue::from(*value),
        Value::Number(value) => value
            .as_i64()
            .map(TemplateValue::from)
            .or_else(|| value.as_u64().map(TemplateValue::from))
            .or_else(|| value.as_f64().map(TemplateValue::from))
            .unwrap_or_else(|| TemplateValue::from(value.to_string())),
        Value::String(value) => TemplateValue::from(value.clone()),
        Value::Array(values) => values.iter().map(template_value).collect(),
        Value::Object(values) => {
            TemplateValue::from_pairs(values.iter().map(|(key, value)| (key.clone(), template_value(value))))
        }
    }
}

#[derive(Clone, Copy)]
struct RemoteTemplateState<'a> {
    hash: Option<&'a str>,
    info: Option<&'a FileInfo>,
}

fn template_commands(
    input: &TemplateInput,
    rendered: String,
    local_hash: &str,
    remote: RemoteTemplateState<'_>,
) -> Vec<Command> {
    let content_changed = remote.hash != Some(local_hash);
    let mut commands = Vec::new();
    if content_changed {
        if input.create_remote_dir
            && let Some(parent) = remote_parent(&input.destination)
        {
            commands.push(shell([raw("mkdir"), raw("-p"), quoted(parent)]));
        }
        commands.push(Command::UploadContent(ContentUploadCommand {
            content: rendered.into_bytes(),
            destination: input.destination.clone(),
            remote_temporary_filename: None,
        }));
    }
    let metadata = RequestedMetadata {
        mode: input.mode.filter(|mode| content_changed || remote.info.is_none_or(|info| info.mode != *mode)),
        user: input.user.as_deref().filter(|user| content_changed || remote.info.is_none_or(|info| info.user != *user)),
        group: input
            .group
            .as_deref()
            .filter(|group| content_changed || remote.info.is_none_or(|info| info.group != *group)),
        recursive: false,
    };
    add_metadata_commands(&mut commands, &input.destination, metadata);
    commands
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::{Template, TemplateInput, render};
    use crate::connectors::{LocalConnector, RunOptions};
    use crate::facts::FactOptions;
    use crate::operations::{CommandExecutor, StatefulOperation};

    #[test]
    fn renderer_handles_structured_data_and_rejects_undefined_values() {
        let data = json!({ "service": { "name": "worker" }, "ports": [80, 443] });
        let object = data.as_object().unwrap();

        assert_eq!(
            render("{{ service.name }}:{% for port in ports %} {{ port }}{% endfor %}", object).unwrap(),
            "worker: 80 443"
        );
        assert!(render("{{ missing }}", object).is_err());
    }

    #[tokio::test]
    async fn template_uploads_rendered_content_and_then_becomes_a_noop() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("config.j2");
        let destination = directory.path().join("remote/config");
        fs::write(&source, "name={{ name }}\n").unwrap();
        let mut input = TemplateInput::new(&source, destination.to_string_lossy());
        input.data.insert("name".to_owned(), json!("worker"));
        let options = FactOptions::default();

        let first = Template.commands(&input, &LocalConnector, &options).await.unwrap();
        CommandExecutor.execute(&LocalConnector, &first, &RunOptions::default()).await.unwrap();
        let second = Template.commands(&input, &LocalConnector, &options).await.unwrap();

        assert_eq!(fs::read_to_string(destination).unwrap(), "name=worker");
        assert!(second.is_empty());
    }
}
