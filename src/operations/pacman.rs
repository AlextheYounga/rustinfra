use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::packages::PackageMap;
use crate::facts::pacman::{PacmanPackages, PacmanUnpackGroup};
use crate::facts::{FactExecutor, FactOptions};

use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Copy, Debug, Default)]
pub struct Upgrade;

impl Operation for Upgrade {
    type Input = ();

    fn name(&self) -> &'static str {
        "pacman.upgrade"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![shell("pacman --noconfirm -Su")]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;

impl Operation for Update {
    type Input = ();

    fn name(&self) -> &'static str {
        "pacman.update"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![shell("pacman -Sy")]
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
        "pacman.packages"
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
            let installed = facts.get(&PacmanPackages, &()).await?;
            let mut expanded = Vec::new();
            for package in &input.packages {
                expanded.push(facts.get(&PacmanUnpackGroup, package).await?);
            }
            Ok(package_commands(input, &installed, &expanded))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &PackageMap, expanded: &[Vec<String>]) -> Vec<Command> {
    let mut commands = Vec::new();
    if input.update {
        commands.push(shell("pacman -Sy"));
    }
    if input.upgrade {
        commands.push(shell("pacman --noconfirm -Su"));
    }
    let changed = input
        .packages
        .iter()
        .zip(expanded)
        .filter(|(package, expanded)| {
            let names = if expanded.is_empty() {
                vec![package.as_str()]
            } else {
                expanded.iter().map(String::as_str).collect()
            };
            let matches = names.iter().filter(|name| installed.contains_key(**name)).count();
            if input.present { matches != names.len() } else { matches > 0 }
        })
        .map(|(package, _)| package)
        .collect::<Vec<_>>();
    if !changed.is_empty() {
        let action = if input.present { "pacman --noconfirm -S" } else { "pacman --noconfirm -R" };
        commands.push(Command::Shell(StringCommand::new(
            [CommandBit::from(action)]
                .into_iter()
                .chain(changed.into_iter().map(|package| CommandBit::quoted(package.as_str()))),
        )));
    }
    commands
}

fn shell(command: &str) -> Command {
    Command::Shell(StringCommand::new([command]))
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
    fn expands_groups_for_presence_and_removal() {
        let installed = IndexMap::from([
            ("curl".to_owned(), vec!["1".to_owned()]),
            ("xorg-font-util".to_owned(), vec!["1".to_owned()]),
        ]);
        let mut input = PackagesInput::new(["curl", "i-dont-exist", "xorg-fonts"]);
        let expanded = vec![
            vec!["curl".to_owned()],
            Vec::new(),
            vec!["xorg-font-util".to_owned(), "xorg-font-encodings".to_owned()],
        ];
        assert_eq!(
            descriptions(&package_commands(&input, &installed, &expanded)),
            ["pacman --noconfirm -S i-dont-exist xorg-fonts"]
        );
        input.present = false;
        assert_eq!(
            descriptions(&package_commands(&input, &installed, &expanded)),
            ["pacman --noconfirm -R curl xorg-fonts"]
        );
    }
}
