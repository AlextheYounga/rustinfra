use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::apk::ApkPackages;
use crate::facts::packages::PackageMap;
use crate::facts::{FactExecutor, FactOptions};

use super::packages::{PackageCommands, PackagePolicy, converge_packages};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpgradeInput {
    pub available: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Upgrade;

impl Operation for Upgrade {
    type Input = UpgradeInput;

    fn name(&self) -> &'static str {
        "apk.upgrade"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![shell(if input.available { "apk upgrade --available" } else { "apk upgrade" })]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;

impl Operation for Update {
    type Input = ();

    fn name(&self) -> &'static str {
        "apk.update"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![shell("apk update")]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub update: bool,
    pub upgrade: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            update: false,
            upgrade: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "apk.packages"
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
            let installed = FactExecutor::new(connector, fact_options).get(&ApkPackages, &()).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &PackageMap) -> Vec<Command> {
    let mut commands = Vec::new();
    if input.update {
        commands.push(shell("apk update"));
    }
    if input.upgrade {
        commands.push(shell("apk upgrade"));
    }
    commands.extend(converge_packages(
        &input.packages,
        installed,
        PackagePolicy { present: input.present, latest: input.latest },
        &PackageCommands {
            install: vec![CommandBit::from("apk add")],
            uninstall: vec![CommandBit::from("apk del")],
            upgrade: vec![CommandBit::from("apk upgrade")],
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

    use super::{PackagesInput, Upgrade, UpgradeInput, package_commands};
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
    fn matches_package_and_upgrade_reference_fixtures() {
        let installed = IndexMap::from([("curl".to_owned(), vec!["1".to_owned()])]);
        let mut input = PackagesInput::new(["curl", "i-dont-exist", "pinned=2-r1"]);
        input.present = false;
        assert_eq!(descriptions(&package_commands(&input, &installed)), ["apk del curl"]);

        input.present = true;
        input.update = true;
        input.upgrade = true;
        assert_eq!(
            descriptions(&package_commands(&input, &installed)),
            ["apk update", "apk upgrade", "apk add i-dont-exist pinned=2-r1"]
        );
        assert_eq!(descriptions(&Upgrade.commands(&UpgradeInput { available: true })), ["apk upgrade --available"]);
    }
}
