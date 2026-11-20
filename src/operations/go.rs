use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::go::GoPackages;
use crate::facts::packages::PackageMap;
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

const BINDIR_PREFIX: &str = "BINDIR=$(go env GOBIN); [ -z \"$BINDIR\" ] && BINDIR=\"$(go env GOPATH)/bin\";";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { packages: packages.into_iter().map(Into::into).collect(), present: true, latest: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "go.packages"
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
            let installed = FactExecutor::new(connector, fact_options).get(&GoPackages, &()).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &PackageMap) -> Vec<Command> {
    let mut installs = Vec::new();
    let mut upgrades = Vec::new();
    let mut removals = Vec::new();

    for package in &input.packages {
        let (name, version) = package.split_once('@').map_or((package.as_str(), None), |(name, version)| {
            (name, (!version.is_empty() && version != "latest").then_some(version))
        });
        let versions = installed.get(name);
        if input.present {
            if version.is_none() {
                if versions.is_none_or(Vec::is_empty) {
                    installs.push(format!("{name}@latest"));
                } else if input.latest {
                    upgrades.push(format!("{name}@latest"));
                }
            } else if versions.is_none_or(|versions| !versions.iter().any(|value| Some(value.as_str()) == version)) {
                installs.push(package.clone());
            }
        } else if versions.is_some_and(|versions| !versions.is_empty()) {
            removals.push(binary_name(name));
        }
    }

    let mut commands = Vec::new();
    if !installs.is_empty() {
        commands.push(go_install(&installs));
    }
    if !upgrades.is_empty() {
        commands.push(go_install(&upgrades));
    }
    if !removals.is_empty() {
        let bits = [CommandBit::from(BINDIR_PREFIX), CommandBit::from("rm -f")].into_iter().chain(
            removals.into_iter().map(|binary| {
                CommandBit::from(StringCommand::with_separator(
                    [CommandBit::from("\"$BINDIR\"/"), CommandBit::quoted(binary)],
                    "",
                ))
            }),
        );
        commands.push(Command::Shell(StringCommand::new(bits)));
    }
    commands
}

fn go_install(packages: &[String]) -> Command {
    Command::Shell(StringCommand::new(
        [CommandBit::from("go install")]
            .into_iter()
            .chain(packages.iter().map(|package| CommandBit::quoted(package.as_str()))),
    ))
}

fn binary_name(package: &str) -> &str {
    let mut parts = package.rsplit('/');
    let Some(last) = parts.next() else {
        return package;
    };
    if last.len() > 1 && last.starts_with('v') && last[1..].bytes().all(|byte| byte.is_ascii_digit()) {
        if let Some(name) = parts.next() { name } else { last }
    } else {
        last
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{PackagesInput, package_commands};
    use crate::api::Command;

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
    fn matches_install_and_upgrade_reference_fixtures() {
        let installed = IndexMap::from([("github.com/foo/bar".to_owned(), vec!["v1.2.3".to_owned()])]);
        let mut input = PackagesInput::new(["github.com/foo/bar", "github.com/bitmagnet-io/bitmagnet@v0.10.0"]);
        input.latest = true;

        assert_eq!(
            descriptions(&package_commands(&input, &installed)),
            ["go install github.com/bitmagnet-io/bitmagnet@v0.10.0", "go install github.com/foo/bar@latest"]
        );
    }

    #[test]
    fn matches_safe_removal_and_major_version_reference_fixtures() {
        let installed = IndexMap::from([
            ("github.com/foo/bar/v2".to_owned(), vec!["v2.0.5".to_owned()]),
            ("a/b/cmd/tool; id".to_owned(), vec!["v1.0.0".to_owned()]),
        ]);
        let mut input = PackagesInput::new(["github.com/foo/bar/v2", "a/b/cmd/tool; id"]);
        input.present = false;

        assert_eq!(
            descriptions(&package_commands(&input, &installed)),
            [
                "BINDIR=$(go env GOBIN); [ -z \"$BINDIR\" ] && BINDIR=\"$(go env GOPATH)/bin\"; rm -f \"$BINDIR\"/bar \"$BINDIR\"/'tool; id'"
            ]
        );
    }
}
