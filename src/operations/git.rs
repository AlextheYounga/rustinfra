use std::error::Error;
use std::slice;

use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{Directory, PathInfo, PathInput};
use crate::facts::git::{
    GitBranch, GitConfig, GitLocalCommit, GitRefInput, GitRemoteBranchCommit, GitRemoteInput, GitRepoInput, GitTag,
    git_command,
};
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepoInput {
    pub source: String,
    pub destination: String,
    pub branch: Option<String>,
    pub pull: bool,
    pub rebase: bool,
    pub user: Option<String>,
    pub group: Option<String>,
    pub update_submodules: bool,
    pub recursive_submodules: bool,
    pub depth: Option<u64>,
    pub fetch_tags: bool,
    pub force: bool,
}

impl RepoInput {
    #[must_use]
    pub fn new(source: impl Into<String>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            branch: None,
            pull: true,
            rebase: false,
            user: None,
            group: None,
            update_submodules: false,
            recursive_submodules: false,
            depth: None,
            fetch_tags: false,
            force: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Repo;

impl StatefulOperation for Repo {
    type Input = RepoInput;

    fn name(&self) -> &'static str {
        "git.repo"
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
            let destination = facts.get(&Directory, &PathInput::from(input.destination.clone())).await?;
            if destination == PathInfo::DifferentType {
                return Err(Box::new(GitOperationError::DestinationNotDirectory { path: input.destination.clone() })
                    as Box<dyn Error + Send + Sync>);
            }
            let git_directory = format!("{}/.git", input.destination.trim_end_matches('/'));
            let repository = facts.get(&Directory, &PathInput::from(git_directory)).await?;
            let mut state = RepoState { destination, repository, ..RepoState::default() };
            if matches!(state.repository, PathInfo::Present(_)) {
                let safe_directory = (input.user.is_some() || input.group.is_some()).then(|| input.destination.clone());
                let repo = GitRepoInput { repo: input.destination.clone(), safe_directory };
                state.config = facts.get(&GitConfig, &repo).await?;
                state.branch = facts.get(&GitBranch, &repo).await?;
                if input.branch.is_some() {
                    state.tags = facts.get(&GitTag, &repo).await?;
                }
                let effective_branch = input.branch.as_ref().or(state.branch.as_ref());
                if input.pull
                    && let Some(branch) = effective_branch
                {
                    state.local_commit = facts
                        .get(&GitLocalCommit, &GitRefInput { repo: repo.clone(), reference: branch.clone() })
                        .await?;
                    state.remote_commit = facts
                        .get(
                            &GitRemoteBranchCommit,
                            &GitRemoteInput { repo, remote: "origin".to_owned(), branch: Some(branch.clone()) },
                        )
                        .await?;
                }
            }
            Ok(repo_commands(input, &state))
        })
    }
}

#[derive(Clone, Debug)]
struct RepoState {
    destination: PathInfo,
    repository: PathInfo,
    config: indexmap::IndexMap<String, Vec<String>>,
    branch: Option<String>,
    tags: Vec<String>,
    local_commit: Option<String>,
    remote_commit: Option<String>,
}

impl Default for RepoState {
    fn default() -> Self {
        Self {
            destination: PathInfo::Missing,
            repository: PathInfo::Missing,
            config: indexmap::IndexMap::new(),
            branch: None,
            tags: Vec::new(),
            local_commit: None,
            remote_commit: None,
        }
    }
}

fn repo_commands(input: &RepoInput, state: &RepoState) -> Vec<Command> {
    let mut commands = Vec::new();
    if state.destination == PathInfo::Missing {
        commands.push(shell([raw("mkdir"), raw("-p"), quoted(&input.destination)]));
    }
    let safe_directory = (input.user.is_some() || input.group.is_some()).then(|| input.destination.clone());
    let repo = GitRepoInput { repo: input.destination.clone(), safe_directory };
    let mut git_commands = Vec::new();
    if !matches!(state.repository, PathInfo::Present(_)) {
        let mut bits = vec![raw("clone"), quoted(&input.source)];
        add_depth(&mut bits, input.depth);
        if let Some(branch) = &input.branch {
            bits.extend([raw("--branch"), quoted(branch)]);
        }
        bits.push(raw("."));
        git_commands.push(bits);
    } else {
        existing_repo_commands(input, state, &mut git_commands);
    }
    if input.update_submodules {
        let mut bits = vec![raw("submodule"), raw("update"), raw("--init")];
        if input.recursive_submodules {
            bits.push(raw("--recursive"));
        }
        git_commands.push(bits);
    }
    for bits in &git_commands {
        commands.push(git_shell(&repo, bits.clone()));
    }
    if !git_commands.is_empty()
        && let Some(owner) = owner(input.user.as_deref(), input.group.as_deref())
    {
        commands.push(shell([
            raw(if input.user.is_some() { "chown" } else { "chgrp" }),
            raw("-R"),
            quoted(&owner),
            quoted(&input.destination),
        ]));
    }
    commands
}

fn existing_repo_commands(input: &RepoInput, state: &RepoState, commands: &mut Vec<Vec<CommandBit>>) {
    let remote = state.config.get("remote.origin.url");
    let remote_changed = remote.is_some_and(|values| values != slice::from_ref(&input.source));
    if remote_changed {
        commands.push(vec![raw("remote"), raw("set-url"), raw("origin"), quoted(&input.source)]);
    }
    let branch_changed = input.branch.as_ref().is_some_and(|branch| state.branch.as_ref() != Some(branch));
    if branch_changed {
        let mut fetch = vec![raw("fetch")];
        add_depth(&mut fetch, input.depth);
        if input.fetch_tags {
            fetch.push(raw("--tags"));
        }
        if input.force {
            fetch.push(raw("--force"));
        }
        commands.push(fetch);
        let mut checkout = vec![raw("checkout"), quoted(input.branch.as_deref().unwrap_or_default())];
        if input.force {
            checkout.push(raw("--force"));
        }
        commands.push(checkout);
    }
    let is_tag = input.branch.as_ref().is_some_and(|branch| state.tags.contains(branch));
    if is_tag {
        let mut checkout = vec![raw("checkout"), quoted(input.branch.as_deref().unwrap_or_default())];
        if input.force {
            checkout.push(raw("--force"));
        }
        commands.push(checkout);
    }
    let up_to_date = state.local_commit.is_some() && state.local_commit == state.remote_commit;
    if input.pull && !is_tag && (remote_changed || !up_to_date) {
        let mut pull = vec![raw("pull")];
        add_depth(&mut pull, input.depth);
        if input.rebase {
            pull.push(raw("--rebase"));
        }
        if input.force {
            pull.push(raw("--force"));
        }
        commands.push(pull);
    }
}

fn git_shell(repo: &GitRepoInput, arguments: Vec<CommandBit>) -> Command {
    let mut bits = vec![raw("cd"), quoted(&repo.repo), raw("&&"), git_command(repo).into()];
    bits.extend(arguments);
    shell(bits)
}

fn add_depth(bits: &mut Vec<CommandBit>, depth: Option<u64>) {
    if let Some(depth) = depth {
        bits.extend([raw("--depth"), quoted(&depth.to_string())]);
    }
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
pub enum GitOperationError {
    #[error("git destination {path} exists and is not a directory")]
    DestinationNotDirectory { path: String },
}

#[cfg(test)]
mod tests {
    use super::{RepoInput, RepoState, repo_commands};
    use crate::api::Command;
    use crate::facts::files::{FileInfo, PathInfo};

    fn directory() -> PathInfo {
        PathInfo::Present(FileInfo {
            user: "root".to_owned(),
            group: "root".to_owned(),
            mode: 755,
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
    fn missing_repository_is_cloned_with_branch_depth_and_ownership() {
        let mut input = RepoInput::new("git@example/repo.git", "/srv/my repo");
        input.branch = Some("main branch".to_owned());
        input.depth = Some(1);
        input.user = Some("app".to_owned());
        input.pull = false;
        let state = RepoState { destination: directory(), ..RepoState::default() };

        assert_eq!(
            descriptions(&repo_commands(&input, &state)),
            [
                "cd '/srv/my repo' && git -c safe.directory='/srv/my repo' clone git@example/repo.git --depth 1 --branch 'main branch' .",
                "chown -R app '/srv/my repo'"
            ]
        );
    }

    #[test]
    fn current_repository_skips_pull_and_changed_remote_forces_it() {
        let input = RepoInput::new("new-source", "/srv/repo");
        let commit = "a".repeat(40);
        let mut state = RepoState {
            destination: directory(),
            repository: directory(),
            branch: Some("main".to_owned()),
            local_commit: Some(commit.clone()),
            remote_commit: Some(commit),
            ..RepoState::default()
        };
        state.config.insert("remote.origin.url".to_owned(), vec!["new-source".to_owned()]);
        assert!(repo_commands(&input, &state).is_empty());

        state.config.insert("remote.origin.url".to_owned(), vec!["old-source".to_owned()]);
        assert_eq!(
            descriptions(&repo_commands(&input, &state)),
            ["cd /srv/repo && git remote set-url origin new-source", "cd /srv/repo && git pull"]
        );
    }
}
