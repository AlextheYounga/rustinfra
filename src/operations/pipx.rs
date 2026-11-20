use std::str::FromStr;

use pep508_rs::{Requirement, VerbatimUrl, VersionOrUrl};

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::packages::PackageMap;
use crate::facts::pipx::{PipxEnvironment, PipxPackages};
use crate::facts::server::Path;
use crate::facts::{FactExecutor, FactOptions};

use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub extra_arguments: Vec<String>,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            extra_arguments: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "pipx.packages"
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
            let installed = FactExecutor::new(connector, fact_options).get(&PipxPackages, &()).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &PackageMap) -> Vec<Command> {
    input
        .packages
        .iter()
        .filter_map(|package| {
            let requirement = PythonRequirement::parse(package)?;
            let versions = installed.get(&requirement.name);
            let present = versions.is_some_and(|versions| {
                requirement.version.as_ref().is_none_or(|version| versions.iter().any(|value| value == version))
            });
            let needs_change = input.present && (!present || input.latest && requirement.version.is_none())
                || !input.present && present;
            needs_change.then(|| package_command(input, package, &requirement.name))
        })
        .collect()
}

fn package_command(input: &PackagesInput, package: &str, package_name: &str) -> Command {
    let action = if input.present { "install" } else { "uninstall" };
    let mut bits = vec![CommandBit::from("pipx"), CommandBit::from(action)];
    if input.present {
        bits.extend(input.extra_arguments.iter().map(|argument| CommandBit::quoted(argument.as_str())));
        bits.push(CommandBit::quoted(package));
    } else {
        bits.push(CommandBit::quoted(package_name));
    }
    Command::Shell(StringCommand::new(bits))
}

struct PythonRequirement {
    name: String,
    version: Option<String>,
}

impl PythonRequirement {
    fn parse(value: &str) -> Option<Self> {
        let requirement = Requirement::<VerbatimUrl>::from_str(value).ok()?;
        let version = match &requirement.version_or_url {
            Some(VersionOrUrl::VersionSpecifier(specifiers)) if specifiers.len() == 1 => {
                Some(specifiers[0].version().to_string())
            }
            _ => None,
        };
        Some(Self { name: requirement.name.to_string().to_ascii_lowercase(), version })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UpgradeAll;

impl Operation for UpgradeAll {
    type Input = ();

    fn name(&self) -> &'static str {
        "pipx.upgrade_all"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![Command::Shell(StringCommand::new(["pipx upgrade-all"]))]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnsurePath;

impl StatefulOperation for EnsurePath {
    type Input = ();

    fn name(&self) -> &'static str {
        "pipx.ensure_path"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        _input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let executor = FactExecutor::new(connector, fact_options);
            let path = executor.get(&Path, &()).await?.unwrap_or_default();
            let environment = executor.get(&PipxEnvironment, &()).await?;
            Ok(ensure_path_commands(&path, environment.get("PIPX_BIN_DIR").map(String::as_str)))
        })
    }
}

fn ensure_path_commands(path: &str, bin_directory: Option<&str>) -> Vec<Command> {
    let configured = bin_directory.is_some_and(|bin| path.split(':').any(|directory| directory == bin));
    (!configured).then(|| Command::Shell(StringCommand::new(["pipx ensurepath"]))).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{PackagesInput, UpgradeAll, ensure_path_commands, package_commands};
    use crate::api::Command;
    use crate::operations::Operation;

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
    fn packages_match_pep508_install_remove_url_and_extra_argument_fixtures() {
        let installed = IndexMap::from([
            ("copier".to_owned(), vec!["9.9.0".to_owned()]),
            ("invoke".to_owned(), vec!["2.2.0".to_owned()]),
        ]);
        let mut input = PackagesInput::new(["copier @ git@github.com:copier-org/copier.git@v9.9.0", "pytask"]);
        input.extra_arguments = vec!["--index-url".to_owned(), "https://pypi.org/".to_owned()];
        assert_eq!(
            descriptions(&package_commands(&input, &installed)),
            ["pipx install --index-url https://pypi.org/ pytask"]
        );

        input.packages = vec!["invoke".to_owned(), "copier".to_owned()];
        input.present = false;
        assert_eq!(
            descriptions(&package_commands(&input, &installed)),
            ["pipx uninstall invoke", "pipx uninstall copier"]
        );
        assert_eq!(descriptions(&UpgradeAll.commands(&())), ["pipx upgrade-all"]);
    }

    #[test]
    fn ensure_path_only_runs_when_the_pipx_bin_directory_is_absent() {
        assert!(ensure_path_commands("/usr/bin:/home/me/.local/bin", Some("/home/me/.local/bin")).is_empty());
        assert_eq!(
            descriptions(&ensure_path_commands("/usr/bin:/bin", Some("/home/me/.local/bin"))),
            ["pipx ensurepath"]
        );
    }
}
