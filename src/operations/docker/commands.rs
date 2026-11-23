use crate::api::Command;
use crate::connectors::Connector;
use crate::facts::FactOptions;
use crate::operations::{Operation, StatefulOperation, StatefulOperationFuture};
use indexmap::IndexMap;

use super::{command, optional, pairs, quoted, raw, repeated};

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
    pub env: IndexMap<String, String>,
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
            env: IndexMap::new(),
            detach: false,
            privileged: false,
            tty: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ContainerExec;
impl StatefulOperation for ContainerExec {
    type Input = ContainerExecInput;
    fn name(&self) -> &'static str {
        "docker.container_exec"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        _connector: &'a dyn Connector,
        _options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move { container_exec_commands(input).map_err(Into::into) })
    }
}

fn container_exec_commands(input: &ContainerExecInput) -> Result<Vec<Command>, &'static str> {
    if match &input.command {
        ExecCommand::Shell(value) => value.is_empty(),
        ExecCommand::Arguments(values) => values.is_empty(),
    } {
        return Err("docker.container_exec requires a command");
    }
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
    Ok(command(bits))
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

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
