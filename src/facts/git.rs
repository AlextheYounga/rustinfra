use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, IGNORE_STDERR_AND_FAILURE, StringCommand};

use super::Fact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRepoInput {
    pub repo: String,
    pub safe_directory: Option<String>,
}

impl GitRepoInput {
    #[must_use]
    pub fn new(repo: impl Into<String>) -> Self {
        Self { repo: repo.into(), safe_directory: None }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRefInput {
    pub repo: GitRepoInput,
    pub reference: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitRemoteInput {
    pub repo: GitRepoInput,
    pub remote: String,
    pub branch: Option<String>,
}

macro_rules! git_lines_fact {
    ($name:ident, $fact_name:literal, $arguments:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl Fact for $name {
            type Input = GitRepoInput;
            type Output = Vec<String>;

            fn name(&self) -> &'static str {
                $fact_name
            }

            fn command(&self, input: &Self::Input) -> StringCommand {
                repo_command(input, $arguments)
            }

            fn default(&self) -> Self::Output {
                Vec::new()
            }

            fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
                Some("git")
            }

            fn process(
                &self,
                output: &[String],
                _input: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(output.to_vec())
            }
        }
    };
}

git_lines_fact!(GitTag, "git.GitTag", vec![CommandBit::from("tag")]);

#[derive(Clone, Copy, Debug, Default)]
pub struct GitBranch;

impl Fact for GitBranch {
    type Input = GitRepoInput;
    type Output = Option<String>;

    fn name(&self) -> &'static str {
        "git.GitBranch"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        repo_command(input, vec![CommandBit::from("describe"), CommandBit::from("--all")])
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("git")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok((!output.is_empty()).then(|| output.join("\n").replace("heads/", "").replace("tags/", "")))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GitConfig;

impl Fact for GitConfig {
    type Input = GitRepoInput;
    type Output = IndexMap<String, Vec<String>>;

    fn name(&self) -> &'static str {
        "git.GitConfig"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        repo_command(input, vec![CommandBit::from("config"), CommandBit::from("--local"), CommandBit::from("-l")])
    }

    fn default(&self) -> Self::Output {
        IndexMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("git")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut config = IndexMap::new();
        for line in output {
            if let Some((key, value)) = line.split_once('=') {
                config.entry(key.to_owned()).or_insert_with(Vec::new).push(value.to_owned());
            }
        }
        Ok(config)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GitLocalCommit;

impl Fact for GitLocalCommit {
    type Input = GitRefInput;
    type Output = Option<String>;

    fn name(&self) -> &'static str {
        "git.GitLocalCommit"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        repo_command(
            &input.repo,
            vec![
                CommandBit::from("rev-parse"),
                CommandBit::quoted(input.reference.clone()),
                CommandBit::from(IGNORE_STDERR_AND_FAILURE),
            ],
        )
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("git")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().filter(|value| valid_sha(value)).cloned())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GitRemoteBranchCommit;

impl Fact for GitRemoteBranchCommit {
    type Input = GitRemoteInput;
    type Output = Option<String>;

    fn name(&self) -> &'static str {
        "git.GitRemoteBranchCommit"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        repo_command(
            &input.repo,
            vec![
                CommandBit::from("ls-remote"),
                CommandBit::quoted(input.remote.clone()),
                CommandBit::quoted(input.branch.clone().unwrap_or_else(|| "HEAD".to_owned())),
                CommandBit::from("2>/dev/null | head -n1"),
            ],
        )
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("git")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .first()
            .and_then(|line| line.split_whitespace().next())
            .filter(|value| valid_sha(value))
            .map(str::to_owned))
    }
}

pub(crate) fn git_command(input: &GitRepoInput) -> StringCommand {
    let mut bits = vec![CommandBit::from("git")];
    if let Some(directory) = &input.safe_directory {
        bits.extend([
            CommandBit::from("-c"),
            StringCommand::with_separator(
                [CommandBit::from("safe.directory="), CommandBit::quoted(directory.clone())],
                "",
            )
            .into(),
        ]);
    }
    StringCommand::new(bits)
}

fn repo_command(input: &GitRepoInput, arguments: Vec<CommandBit>) -> StringCommand {
    let mut bits = vec![
        CommandBit::from("! test -d"),
        CommandBit::quoted(input.repo.clone()),
        CommandBit::from("|| ( cd"),
        CommandBit::quoted(input.repo.clone()),
        CommandBit::from("&&"),
        git_command(input).into(),
    ];
    bits.extend(arguments);
    bits.push(CommandBit::from(")"));
    StringCommand::new(bits)
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{Fact, GitBranch, GitConfig, GitLocalCommit, GitRefInput, GitRepoInput};

    #[test]
    fn git_facts_quote_repository_and_parse_reference_shapes() {
        let input = GitRepoInput::new("/srv/my repo");
        assert_eq!(
            GitBranch.command(&input).raw_value(),
            "! test -d '/srv/my repo' || ( cd '/srv/my repo' && git describe --all )"
        );
        assert_eq!(GitBranch.process(&["heads/main".to_owned()], &input).unwrap().as_deref(), Some("main"));
        let config = GitConfig
            .process(&["remote.origin.url=one".to_owned(), "remote.origin.url=two".to_owned()], &input)
            .unwrap();
        assert_eq!(config["remote.origin.url"], ["one", "two"]);

        let reference = GitRefInput { repo: input, reference: "main".to_owned() };
        assert_eq!(
            GitLocalCommit
                .process(&["0123456789012345678901234567890123456789".to_owned()], &reference)
                .unwrap()
                .as_deref(),
            Some("0123456789012345678901234567890123456789")
        );
    }
}
