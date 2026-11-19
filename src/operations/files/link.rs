use std::error::Error;

use crate::api::{Command, CommandBit};
use crate::connectors::Connector;
use crate::facts::files::{Link as LinkFact, PathInfo, PathInput};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

use super::{FilesOperationError, quoted, raw, remote_parent, shell};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkInput {
    pub path: String,
    pub target: String,
    pub present: bool,
    pub user: Option<String>,
    pub group: Option<String>,
    pub create_remote_dir: bool,
}

impl LinkInput {
    #[must_use]
    pub fn new(path: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            target: target.into(),
            present: true,
            user: None,
            group: None,
            create_remote_dir: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Link;

impl StatefulOperation for Link {
    type Input = LinkInput;

    fn name(&self) -> &'static str {
        "files.link"
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
                FactExecutor::new(connector, fact_options).get(&LinkFact, &PathInput::from(input.path.clone())).await?;
            link_commands(input, info).map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
        })
    }
}

fn link_commands(input: &LinkInput, info: PathInfo) -> Result<Vec<Command>, FilesOperationError> {
    match info {
        PathInfo::DifferentType => {
            return Err(FilesOperationError::WrongType { path: input.path.clone(), expected: "link" });
        }
        PathInfo::Missing if !input.present => return Ok(Vec::new()),
        PathInfo::Present(_) if !input.present => return Ok(vec![remove(&input.path)]),
        _ => {}
    }

    let mut commands = Vec::new();
    match info {
        PathInfo::Missing => {
            add_link(input, &mut commands, input.create_remote_dir);
            if let Some(command) = owner_command(input, "", "") {
                commands.push(command);
            }
        }
        PathInfo::Present(current) => {
            if current.link_target.as_deref() != Some(input.target.as_str()) {
                commands.push(remove(&input.path));
                add_link(input, &mut commands, false);
            }
            if let Some(command) = owner_command(input, &current.user, &current.group) {
                commands.push(command);
            }
        }
        PathInfo::DifferentType => {}
    }
    Ok(commands)
}

fn add_link(input: &LinkInput, commands: &mut Vec<Command>, create_remote_dir: bool) {
    if create_remote_dir && let Some(parent) = remote_parent(&input.path) {
        commands.push(shell([raw("mkdir"), raw("-p"), quoted(parent)]));
    }
    commands.push(shell([raw("ln"), raw("-s"), quoted(&input.target), quoted(&input.path)]));
}

fn owner_command(input: &LinkInput, current_user: &str, current_group: &str) -> Option<Command> {
    let changed = input.user.as_deref().is_some_and(|user| user != current_user)
        || input.group.as_deref().is_some_and(|group| group != current_group);
    changed.then(|| {
        let mut bits = vec![raw(if input.user.is_some() { "chown" } else { "chgrp" }), raw("-h")];
        let owner = match (&input.user, &input.group) {
            (Some(user), Some(group)) => format!("{user}:{group}"),
            (Some(user), None) => user.clone(),
            (None, Some(group)) => group.clone(),
            (None, None) => String::new(),
        };
        bits.extend([CommandBit::quoted(owner), quoted(&input.path)]);
        shell(bits)
    })
}

fn remove(path: &str) -> Command {
    shell([raw("rm"), raw("-f"), quoted(path)])
}

#[cfg(test)]
mod tests {
    use super::{LinkInput, link_commands};
    use crate::api::Command;
    use crate::facts::files::{FileInfo, PathInfo};

    fn link(target: &str) -> PathInfo {
        PathInfo::Present(FileInfo {
            user: "root".to_owned(),
            group: "root".to_owned(),
            mode: 777,
            accessed_at: None,
            modified_at: None,
            changed_at: None,
            size: 0,
            link_target: Some(target.to_owned()),
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
    fn matching_link_is_a_noop_and_wrong_target_is_replaced() {
        let input = LinkInput::new("/srv/current", "releases/one");

        assert!(link_commands(&input, link("releases/one")).unwrap().is_empty());
        assert_eq!(
            descriptions(&link_commands(&input, link("releases/old")).unwrap()),
            ["rm -f /srv/current", "ln -s releases/one /srv/current"]
        );
    }
}
