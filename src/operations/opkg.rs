use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::opkg::OpkgPackages;
use crate::facts::packages::PackageMap;
use crate::facts::{FactExecutor, FactOptions};

use super::packages::{PackageCommands, PackagePolicy, converge_packages};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;

impl Operation for Update {
    type Input = ();

    fn name(&self) -> &'static str {
        "opkg.update"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![shell("opkg update")]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub update: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { packages: packages.into_iter().map(Into::into).collect(), present: true, latest: false, update: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "opkg.packages"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            validate(input)?;
            if input.packages.iter().all(String::is_empty) {
                return Ok(Vec::new());
            }
            let installed = FactExecutor::new(connector, fact_options).get(&OpkgPackages, &()).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

#[derive(Debug, Error)]
enum OpkgError {
    #[error("opkg does not support version pinning but found for: '{0}'")]
    VersionPinning(String),
}

fn validate(input: &PackagesInput) -> Result<(), OpkgError> {
    let pinned =
        input.packages.iter().filter_map(|package| package.split_once('=').map(|(name, _)| name)).collect::<Vec<_>>();
    if pinned.is_empty() { Ok(()) } else { Err(OpkgError::VersionPinning(pinned.join(","))) }
}

fn package_commands(input: &PackagesInput, installed: &PackageMap) -> Vec<Command> {
    let mut commands = input.update.then(|| shell("opkg update")).into_iter().collect::<Vec<_>>();
    commands.extend(converge_packages(
        &input.packages,
        installed,
        PackagePolicy { present: input.present, latest: input.latest },
        &PackageCommands {
            install: vec![CommandBit::from("opkg install")],
            uninstall: vec![CommandBit::from("opkg remove")],
            upgrade: vec![CommandBit::from("opkg upgrade")],
            version_join: "=",
        },
    ));
    commands
}

fn shell(command: &str) -> Command {
    Command::Shell(StringCommand::new([command]))
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{PackagesInput, package_commands, validate};
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
    fn matches_install_remove_and_update_reference_fixtures() {
        let mut input = PackagesInput::new(["curl", "wget"]);
        input.update = false;
        assert_eq!(descriptions(&package_commands(&input, &IndexMap::new())), ["opkg install curl wget"]);

        input.present = false;
        input.update = true;
        let installed = IndexMap::from([("curl".to_owned(), vec!["7.66.0-2".to_owned()])]);
        assert_eq!(descriptions(&package_commands(&input, &installed)), ["opkg update", "opkg remove curl"]);
    }

    #[test]
    fn rejects_version_pinning_like_reference() {
        let input = PackagesInput::new(["curl=7.66.0-2"]);
        assert_eq!(
            validate(&input).unwrap_err().to_string(),
            "opkg does not support version pinning but found for: 'curl'"
        );
    }
}
