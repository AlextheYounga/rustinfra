mod commands;
mod plugin;
mod resources;

pub use commands::*;
pub use plugin::*;
pub use resources::*;

use std::collections::BTreeMap;

use crate::api::{Command, CommandBit, HiddenValue, StringCommand};
use crate::connectors::Connector;
use crate::facts::docker::{DockerAuths, DockerImage};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

const DOCKER_HUB_SERVER: &str = "https://index.docker.io/v1/";

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: impl Into<String>) -> CommandBit {
    CommandBit::quoted(value.into())
}
fn command(bits: impl IntoIterator<Item = CommandBit>) -> Vec<Command> {
    vec![Command::Shell(StringCommand::new(bits))]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildInput {
    pub path: String,
    pub tags: Vec<String>,
    pub dockerfile: Option<String>,
    pub build_args: BTreeMap<String, String>,
    pub labels: BTreeMap<String, String>,
    pub target: Option<String>,
    pub platform: Option<String>,
    pub network: Option<String>,
    pub cache_from: Vec<String>,
    pub cache_to: Vec<String>,
    pub secrets: Vec<String>,
    pub pull: bool,
    pub no_cache: bool,
    pub force: bool,
    pub builder: Option<String>,
    pub build_command: String,
}
impl BuildInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            tags: Vec::new(),
            dockerfile: None,
            build_args: BTreeMap::new(),
            labels: BTreeMap::new(),
            target: None,
            platform: None,
            network: None,
            cache_from: Vec::new(),
            cache_to: Vec::new(),
            secrets: Vec::new(),
            pull: false,
            no_cache: false,
            force: false,
            builder: None,
            build_command: "docker build".to_owned(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Build;
impl StatefulOperation for Build {
    type Input = BuildInput;
    fn name(&self) -> &'static str {
        "docker.build"
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
            if input.path.is_empty() {
                return Err("docker.build requires a context path".into());
            }
            if !input.force && !input.tags.is_empty() {
                let facts = FactExecutor::new(connector, options);
                let mut all = true;
                for tag in &input.tags {
                    all &= json_truthy(&facts.get(&DockerImage, tag).await?);
                }
                if all {
                    return Ok(Vec::new());
                }
            }
            let mut bits = vec![raw(&input.build_command)];
            optional(&mut bits, "--builder", input.builder.as_deref());
            for value in &input.tags {
                bits.extend([raw("-t"), quoted(value)]);
            }
            optional(&mut bits, "-f", input.dockerfile.as_deref());
            pairs(&mut bits, "--build-arg", &input.build_args);
            pairs(&mut bits, "--label", &input.labels);
            optional(&mut bits, "--target", input.target.as_deref());
            optional(&mut bits, "--platform", input.platform.as_deref());
            optional(&mut bits, "--network", input.network.as_deref());
            repeated(&mut bits, "--cache-from", &input.cache_from);
            repeated(&mut bits, "--cache-to", &input.cache_to);
            repeated(&mut bits, "--secret", &input.secrets);
            if input.pull {
                bits.push(raw("--pull"));
            }
            if input.no_cache {
                bits.push(raw("--no-cache"));
            }
            bits.push(quoted(&input.path));
            Ok(command(bits))
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginInput {
    pub username: String,
    pub password: HiddenValue,
    pub server: Option<String>,
    pub force: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Login;
impl StatefulOperation for Login {
    type Input = LoginInput;
    fn name(&self) -> &'static str {
        "docker.login"
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
            let target = input.server.as_deref().unwrap_or(DOCKER_HUB_SERVER);
            if !input.force
                && FactExecutor::new(connector, options)
                    .get(&DockerAuths, &())
                    .await?
                    .iter()
                    .any(|value| value == target)
            {
                return Ok(Vec::new());
            }
            let mut bits = vec![
                raw("printf"),
                raw("%s"),
                CommandBit::quoted(input.password.clone()),
                raw("|"),
                raw("docker"),
                raw("login"),
                raw("--username"),
                quoted(&input.username),
                raw("--password-stdin"),
            ];
            if let Some(server) = &input.server {
                bits.push(quoted(server));
            }
            Ok(command(bits))
        })
    }
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LogoutInput {
    pub server: Option<String>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Logout;
impl StatefulOperation for Logout {
    type Input = LogoutInput;
    fn name(&self) -> &'static str {
        "docker.logout"
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
            let target = input.server.as_deref().unwrap_or(DOCKER_HUB_SERVER);
            if !FactExecutor::new(connector, options).get(&DockerAuths, &()).await?.iter().any(|value| value == target)
            {
                return Ok(Vec::new());
            }
            let mut bits = vec![raw("docker"), raw("logout")];
            if let Some(server) = &input.server {
                bits.push(quoted(server));
            }
            Ok(command(bits))
        })
    }
}

fn optional(bits: &mut Vec<CommandBit>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        bits.extend([raw(flag), quoted(value)]);
    }
}
fn repeated(bits: &mut Vec<CommandBit>, flag: &str, values: &[String]) {
    for value in values {
        bits.extend([raw(flag), quoted(value)]);
    }
}
fn pairs(bits: &mut Vec<CommandBit>, flag: &str, values: &BTreeMap<String, String>) {
    for (key, value) in values {
        bits.extend([raw(flag), quoted(format!("{key}={value}"))]);
    }
}
fn json_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        serde_json::Value::String(value) => !value.is_empty(),
        serde_json::Value::Array(value) => !value.is_empty(),
        serde_json::Value::Object(value) => !value.is_empty(),
    }
}
