mod plugin;
mod resources;

pub use plugin::*;
pub use resources::*;

use std::collections::BTreeMap;

use crate::api::{Command, CommandBit, HiddenValue, StringCommand};
use crate::connectors::Connector;
use crate::facts::docker::{DockerAuths, DockerImage};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{Operation, StatefulOperation, StatefulOperationFuture};

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
                    all &= json_exists(&facts.get(&DockerImage, tag).await?);
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
pub struct ComposeInput {
    pub project_directory: String,
    pub files: Vec<String>,
    pub project_name: Option<String>,
    pub present: bool,
    pub pull: Option<String>,
    pub build: bool,
    pub force_recreate: bool,
    pub remove_orphans: bool,
    pub remove_volumes: bool,
    pub compose_command: String,
}
impl ComposeInput {
    #[must_use]
    pub fn new(directory: impl Into<String>) -> Self {
        Self {
            project_directory: directory.into(),
            files: Vec::new(),
            project_name: None,
            present: true,
            pull: None,
            build: false,
            force_recreate: false,
            remove_orphans: true,
            remove_volumes: false,
            compose_command: "docker compose".to_owned(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Compose;
impl Operation for Compose {
    type Input = ComposeInput;
    fn name(&self) -> &'static str {
        "docker.compose"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let mut bits = vec![raw(&input.compose_command), raw("--project-directory"), quoted(&input.project_directory)];
        optional(&mut bits, "--project-name", input.project_name.as_deref());
        repeated(&mut bits, "-f", &input.files);
        if input.present {
            bits.extend([raw("up"), raw("-d")]);
            optional(&mut bits, "--pull", input.pull.as_deref());
            if input.build {
                bits.push(raw("--build"));
            }
            if input.force_recreate {
                bits.push(raw("--force-recreate"));
            }
        } else {
            bits.push(raw("down"));
            if input.remove_volumes {
                bits.push(raw("-v"));
            }
        }
        if input.remove_orphans {
            bits.push(raw("--remove-orphans"));
        }
        command(bits)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecCommand {
    Shell(String),
    Arguments(Vec<String>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerExecInput {
    pub container: String,
    pub command: ExecCommand,
    pub user: Option<String>,
    pub workdir: Option<String>,
    pub env: BTreeMap<String, String>,
    pub detach: bool,
    pub privileged: bool,
    pub tty: bool,
}
impl ContainerExecInput {
    #[must_use]
    pub fn new(container: impl Into<String>, command_value: impl Into<String>) -> Self {
        Self {
            container: container.into(),
            command: ExecCommand::Shell(command_value.into()),
            user: None,
            workdir: None,
            env: BTreeMap::new(),
            detach: false,
            privileged: false,
            tty: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ContainerExec;
impl Operation for ContainerExec {
    type Input = ContainerExecInput;
    fn name(&self) -> &'static str {
        "docker.container_exec"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let mut bits = vec![raw("docker"), raw("exec")];
        for (flag, on) in [("--detach", input.detach), ("--privileged", input.privileged), ("--tty", input.tty)] {
            if on {
                bits.push(raw(flag));
            }
        }
        optional(&mut bits, "--user", input.user.as_deref());
        optional(&mut bits, "--workdir", input.workdir.as_deref());
        pairs(&mut bits, "--env", &input.env);
        bits.push(quoted(&input.container));
        match &input.command {
            ExecCommand::Shell(value) => bits.extend([raw("sh"), raw("-c"), quoted(value)]),
            ExecCommand::Arguments(values) => bits.extend(values.iter().map(quoted)),
        }
        command(bits)
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PruneInput {
    pub all: bool,
    pub volumes: bool,
    pub filter: Option<String>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Prune;
impl Operation for Prune {
    type Input = PruneInput;
    fn name(&self) -> &'static str {
        "docker.prune"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let mut bits = vec![raw("docker"), raw("system"), raw("prune")];
        if input.all {
            bits.push(raw("-a"));
        }
        if let Some(value) = &input.filter {
            bits.push(quoted(format!("--filter={value}")));
        }
        if input.volumes {
            bits.push(raw("--volumes"));
        }
        bits.push(raw("-f"));
        command(bits)
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
fn json_exists(value: &serde_json::Value) -> bool {
    value.as_array().is_some_and(|values| !values.is_empty())
}

#[cfg(test)]
#[path = "docker_tests.rs"]
mod tests;
