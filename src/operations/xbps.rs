use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::packages::PackageMap;
use crate::facts::xbps::{XbpsPackages, XbpsSelfUpdate};
use crate::facts::{FactExecutor, FactOptions};

use super::packages::{PackageCommands, PackagePolicy, converge_packages};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

const SELF_UPDATE: &str = "xbps-install -y -u xbps";

#[derive(Clone, Copy, Debug, Default)]
pub struct Upgrade;

impl Operation for Upgrade {
    type Input = ();

    fn name(&self) -> &'static str {
        "xbps.upgrade"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![shell(SELF_UPDATE), shell("xbps-install -y -u")]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;

impl Operation for Update {
    type Input = ();

    fn name(&self) -> &'static str {
        "xbps.update"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![shell("xbps-install -S")]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub update: bool,
    pub upgrade: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { packages: packages.into_iter().map(Into::into).collect(), present: true, update: false, upgrade: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "xbps.packages"
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
            let facts = FactExecutor::new(connector, fact_options);
            let installed = facts.get(&XbpsPackages, &()).await?;
            let changes = package_changes(input, &installed);
            let needs_self_update = if changes.is_empty() || !input.present || input.upgrade {
                false
            } else if input.update {
                true
            } else {
                !facts.get(&XbpsSelfUpdate, &()).await?.is_empty()
            };
            Ok(package_commands(input, changes, needs_self_update))
        })
    }
}

fn package_changes(input: &PackagesInput, installed: &PackageMap) -> Vec<Command> {
    converge_packages(
        &input.packages,
        installed,
        PackagePolicy { present: input.present, latest: false },
        &PackageCommands {
            install: vec![CommandBit::from("xbps-install -y -u")],
            uninstall: vec![CommandBit::from("xbps-remove -y")],
            upgrade: Vec::new(),
            version_join: "=",
        },
    )
}

fn package_commands(input: &PackagesInput, changes: Vec<Command>, needs_self_update: bool) -> Vec<Command> {
    let mut commands = Vec::new();
    if input.update {
        commands.push(shell("xbps-install -S"));
    }
    if input.upgrade {
        commands.extend([shell(SELF_UPDATE), shell("xbps-install -y -u")]);
    } else if needs_self_update {
        commands.push(shell(SELF_UPDATE));
    }
    commands.extend(changes);
    commands
}

fn shell(command: &str) -> Command {
    Command::Shell(StringCommand::new([command]))
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{PackagesInput, package_changes, package_commands};
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
    fn self_updates_before_install_after_repository_sync() {
        let mut input = PackagesInput::new(["curl"]);
        input.update = true;
        let changes = package_changes(&input, &IndexMap::new());
        assert_eq!(
            descriptions(&package_commands(&input, changes, true)),
            ["xbps-install -S", "xbps-install -y -u xbps", "xbps-install -y -u curl"]
        );
    }

    #[test]
    fn full_upgrade_already_includes_self_update() {
        let mut input = PackagesInput::new(["curl"]);
        input.update = true;
        input.upgrade = true;
        let installed = IndexMap::from([("curl".to_owned(), vec!["1".to_owned()])]);
        assert_eq!(
            descriptions(&package_commands(&input, package_changes(&input, &installed), false)),
            ["xbps-install -S", "xbps-install -y -u xbps", "xbps-install -y -u"]
        );
    }
}
