use std::error::Error;

use indexmap::IndexMap;
use thiserror::Error;

use crate::api::{Command, CommandBit};
use crate::connectors::Connector;
use crate::facts::files::{Directory, File, FileInfo, PathInfo, PathInput};
use crate::facts::git::{
    GIT_HEAD_REFERENCE, GitConfig, GitConfigInput, GitLocalCommit, GitRefInput, GitRemoteBranchCommit, GitRemoteInput,
    GitRepoInput, GitTrackingBranch,
};
use crate::facts::{FactExecutor, FactOptions};

use super::{git_metadata_directory, owner, quoted, raw, shell};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigInput {
    pub key: String,
    pub value: String,
    pub multi_value: bool,
    pub repo: Option<String>,
    pub system: bool,
}

impl ConfigInput {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), multi_value: false, repo: None, system: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Config;

impl StatefulOperation for Config {
    type Input = ConfigInput;
    fn name(&self) -> &'static str {
        "git.config"
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
            let facts = FactExecutor::new(connector, options);
            let fact_input = GitConfigInput { repo: input.repo.clone(), system: input.system, safe_directory: None };
            let existing = if let Some(repo) = &input.repo {
                let git_directory = git_metadata_directory(repo);
                if matches!(facts.get(&Directory, &PathInput::from(git_directory)).await?, PathInfo::Present(_)) {
                    facts.get(&GitConfig, &fact_input).await?
                } else {
                    IndexMap::new()
                }
            } else {
                facts.get(&GitConfig, &fact_input).await?
            };
            Ok(config_commands(input, &existing))
        })
    }
}

fn config_commands(input: &ConfigInput, existing: &IndexMap<String, Vec<String>>) -> Vec<Command> {
    let values = existing.get(&input.key);
    let correct = if input.multi_value {
        values.is_some_and(|values| values.contains(&input.value))
    } else {
        values == Some(&vec![input.value.clone()])
    };
    if correct {
        return Vec::new();
    }
    let mut bits = if let Some(repo) = &input.repo {
        vec![raw("cd"), quoted(repo), raw("&&"), raw("git"), raw("config"), raw("--local")]
    } else {
        vec![raw("git"), raw("config"), raw(if input.system { "--system" } else { "--global" })]
    };
    if input.multi_value {
        bits.push(raw("--add"));
    }
    bits.extend([quoted(&input.key), double_quoted(&input.value)]);
    vec![shell(bits)]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BareRepoInput {
    pub path: String,
    pub user: Option<String>,
    pub group: Option<String>,
    pub present: bool,
}
impl BareRepoInput {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), user: None, group: None, present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BareRepo;
impl StatefulOperation for BareRepo {
    type Input = BareRepoInput;
    fn name(&self) -> &'static str {
        "git.bare_repo"
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
            let facts = FactExecutor::new(connector, options);
            let directory = facts.get(&Directory, &PathInput::from(input.path.clone())).await?;
            let head = facts.get(&File, &PathInput::from(format!("{}/HEAD", input.path.trim_end_matches('/')))).await?;
            bare_repo_commands(input, &directory, &head)
                .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
        })
    }
}

fn bare_repo_commands(
    input: &BareRepoInput,
    directory: &PathInfo,
    head: &PathInfo,
) -> Result<Vec<Command>, GitExtraError> {
    if matches!(directory, PathInfo::DifferentType) {
        return Err(GitExtraError::NotDirectory(input.path.clone()));
    }
    if !input.present {
        return Ok(if matches!(directory, PathInfo::Present(_)) {
            vec![shell([raw("rm"), raw("-rf"), quoted(&input.path)])]
        } else {
            Vec::new()
        });
    }
    let mut commands = Vec::new();
    if matches!(directory, PathInfo::Missing) {
        commands.push(shell([raw("mkdir"), raw("-p"), quoted(&input.path)]));
    }
    if !matches!(head, PathInfo::Present(_)) {
        commands.push(shell([raw("git"), raw("init"), raw("--bare"), quoted(&input.path)]));
        if let Some(command) = chown(&input.path, input.user.as_deref(), input.group.as_deref(), true) {
            commands.push(command);
        }
    } else if let PathInfo::Present(info) = head
        && ownership_differs(&info, input.user.as_deref(), input.group.as_deref())
        && let Some(command) = chown(&input.path, input.user.as_deref(), input.group.as_deref(), true)
    {
        commands.push(command);
    }
    Ok(commands)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeInput {
    pub worktree: String,
    pub repo: Option<String>,
    pub detached: bool,
    pub new_branch: Option<String>,
    pub commitish: Option<String>,
    pub pull: bool,
    pub rebase: bool,
    pub from_remote_branch: Option<(String, String)>,
    pub present: bool,
    pub assume_repo_exists: bool,
    pub force: bool,
    pub user: Option<String>,
    pub group: Option<String>,
}
impl WorktreeInput {
    pub fn new(worktree: impl Into<String>) -> Self {
        Self {
            worktree: worktree.into(),
            repo: None,
            detached: false,
            new_branch: None,
            commitish: None,
            pull: true,
            rebase: false,
            from_remote_branch: None,
            present: true,
            assume_repo_exists: false,
            force: false,
            user: None,
            group: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Worktree;
impl StatefulOperation for Worktree {
    type Input = WorktreeInput;
    fn name(&self) -> &'static str {
        "git.worktree"
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
            let facts = FactExecutor::new(connector, options);
            let path = facts.get(&Directory, &PathInput::from(input.worktree.clone())).await?;
            let mut state = WorktreeState { path, ..WorktreeState::default() };
            if input.present && state.path == PathInfo::Missing {
                let repo = input.repo.as_deref().ok_or(GitExtraError::MissingRepo)?;
                if !input.assume_repo_exists {
                    state.repo = facts.get(&Directory, &PathInput::from(git_metadata_directory(repo))).await?;
                }
            } else if input.present && matches!(state.path, PathInfo::Present(_)) && input.pull {
                let repo = GitRepoInput::new(&input.worktree);
                state.tracking = facts.get(&GitTrackingBranch, &repo).await?;
                let remote = input.from_remote_branch.clone().or_else(|| {
                    state.tracking.as_deref().and_then(|tracking| {
                        tracking.split_once('/').map(|(remote, branch)| (remote.to_owned(), branch.to_owned()))
                    })
                });
                if let Some((remote, branch)) = remote {
                    state.local_commit = facts
                        .get(
                            &GitLocalCommit,
                            &GitRefInput { repo: repo.clone(), reference: GIT_HEAD_REFERENCE.to_owned() },
                        )
                        .await?;
                    state.remote_commit = facts
                        .get(&GitRemoteBranchCommit, &GitRemoteInput { repo, remote, branch: Some(branch) })
                        .await?;
                }
            }
            worktree_commands(input, &state).map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
        })
    }
}

#[derive(Clone, Debug)]
struct WorktreeState {
    path: PathInfo,
    repo: PathInfo,
    tracking: Option<String>,
    local_commit: Option<String>,
    remote_commit: Option<String>,
}
impl Default for WorktreeState {
    fn default() -> Self {
        Self {
            path: PathInfo::Missing,
            repo: PathInfo::Missing,
            tracking: None,
            local_commit: None,
            remote_commit: None,
        }
    }
}

fn worktree_commands(input: &WorktreeInput, state: &WorktreeState) -> Result<Vec<Command>, GitExtraError> {
    if state.path == PathInfo::DifferentType {
        return Err(GitExtraError::NotDirectory(input.worktree.clone()));
    }
    if state.path == PathInfo::Missing {
        if !input.present {
            return Ok(Vec::new());
        }
        let repo = input.repo.as_deref().ok_or(GitExtraError::MissingRepo)?;
        if !input.assume_repo_exists && !matches!(state.repo, PathInfo::Present(_)) {
            return Err(GitExtraError::InvalidRepo(repo.to_owned()));
        }
        return Ok(create_worktree_commands(input, repo));
    }
    if !input.present {
        let mut bits =
            vec![raw("cd"), quoted(&input.worktree), raw("&&"), raw("git"), raw("worktree"), raw("remove"), raw(".")];
        if input.force {
            bits.push(raw("--force"));
        }
        return Ok(vec![shell(bits)]);
    }
    if !input.pull {
        return Ok(Vec::new());
    }
    let has_remote = input.from_remote_branch.is_some() || state.tracking.is_some();
    let up_to_date = state.local_commit.is_some() && state.local_commit == state.remote_commit;
    if !has_remote || up_to_date {
        return Ok(Vec::new());
    }
    let mut bits = vec![raw("cd"), quoted(&input.worktree), raw("&&"), raw("git"), raw("pull")];
    if input.rebase {
        bits.push(raw("--rebase"));
    }
    if let Some((remote, branch)) = &input.from_remote_branch {
        bits.extend([quoted(remote), quoted(branch)]);
    }
    Ok(vec![shell(bits)])
}

fn create_worktree_commands(input: &WorktreeInput, repo: &str) -> Vec<Command> {
    let mut bits = vec![raw("cd"), quoted(repo), raw("&&"), raw("git"), raw("worktree"), raw("add")];
    if let Some(branch) = &input.new_branch {
        bits.extend([raw("-b"), quoted(branch)]);
    } else if input.detached {
        bits.push(raw("--detach"));
    }
    if input.force {
        bits.push(raw("--force"));
    }
    bits.push(quoted(&input.worktree));
    if let Some(commitish) = &input.commitish {
        bits.push(quoted(commitish));
    }
    let mut commands = vec![shell(bits)];
    let Some(owner) = owner(input.user.as_deref(), input.group.as_deref()) else {
        return commands;
    };
    commands.push(chown_value(&input.worktree, &owner, input.user.is_some(), true));
    let name = input.worktree.trim_end_matches('/').rsplit('/').next().unwrap_or(&input.worktree);
    commands.push(chown_value(
        &format!("{}/.git/worktrees/{name}", repo.trim_end_matches('/')),
        &owner,
        input.user.is_some(),
        true,
    ));
    let branch = input.new_branch.as_deref().or_else(|| (!input.detached && input.commitish.is_none()).then_some(name));
    if let Some(branch) = branch {
        commands.push(chown_value(
            &format!("{}/.git/refs/heads/{branch}", repo.trim_end_matches('/')),
            &owner,
            input.user.is_some(),
            false,
        ));
        commands.push(chown_value(
            &format!("{}/.git/logs/refs/heads/{branch}", repo.trim_end_matches('/')),
            &owner,
            input.user.is_some(),
            true,
        ));
    }
    commands
}

fn ownership_differs(info: &FileInfo, user: Option<&str>, group: Option<&str>) -> bool {
    user.is_some_and(|user| info.user != user) || group.is_some_and(|group| info.group != group)
}
fn chown(path: &str, user: Option<&str>, group: Option<&str>, recursive: bool) -> Option<Command> {
    owner(user, group).map(|owner| chown_value(path, &owner, user.is_some(), recursive))
}
fn chown_value(path: &str, owner: &str, has_user: bool, recursive: bool) -> Command {
    let mut bits = vec![raw(if has_user { "chown" } else { "chgrp" })];
    if recursive {
        bits.push(raw("-R"));
    }
    bits.extend([quoted(owner), quoted(path)]);
    shell(bits)
}
fn double_quoted(value: &str) -> CommandBit {
    raw(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace(char::from(96), "\\\u{60}")
    ))
}

#[derive(Debug, Error)]
enum GitExtraError {
    #[error("repo must be specified when creating a worktree")]
    MissingRepo,
    #[error("The following folder is not a valid GIT repository : {0}")]
    InvalidRepo(String),
    #[error("{0} exists and is not a directory")]
    NotDirectory(String),
}

#[cfg(test)]
#[path = "extra_tests.rs"]
mod tests;
