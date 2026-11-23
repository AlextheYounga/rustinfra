use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::rpm::{InstalledPackages, RpmPackages};
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

mod extra;
pub use extra::{Key, Module, ModuleInput, Repo, RepoInput, Rpm, RpmInput, Update};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub update: bool,
    pub clean: bool,
    pub nobest: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            update: false,
            clean: false,
            nobest: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "dnf.packages"
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
            let installed = FactExecutor::new(connector, fact_options).get(&RpmPackages, &()).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &InstalledPackages) -> Vec<Command> {
    let mut commands = Vec::new();
    if input.clean {
        commands.push(command([raw("dnf"), raw("clean"), raw("all")]));
    }
    if input.update {
        commands.push(command([raw("dnf"), raw("update"), raw("-y")]));
    }
    let packages = input
        .packages
        .iter()
        .filter(|package| needs_change(package, input, installed))
        .map(|package| package.clone())
        .collect::<Vec<_>>();
    if packages.is_empty() {
        return commands;
    }

    let action = if input.present && input.latest {
        "update"
    } else if input.present {
        "install"
    } else {
        "remove"
    };
    let mut bits = vec![raw("dnf"), raw(action), raw("-y")];
    if input.present && input.nobest {
        bits.push(raw("--nobest"));
    }
    bits.extend(packages.into_iter().map(CommandBit::quoted));
    commands.push(command(bits));
    commands
}

fn needs_change(package: &str, input: &PackagesInput, installed: &InstalledPackages) -> bool {
    let (name, version) = package.split_once('=').map_or((package, None), |(name, version)| (name, Some(version)));
    let versions = installed.get(name);
    if !input.present {
        versions.is_some()
    } else if let Some(version) = version {
        versions.is_none_or(|versions| !versions.iter().any(|installed| installed == version))
    } else {
        versions.is_none() || input.latest
    }
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}

fn command(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
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
    fn dnf_filters_installed_packages_and_preserves_update_controls() {
        let installed = [("bash".to_owned(), vec!["5.2".to_owned()])].into_iter().collect::<IndexMap<_, _>>();
        let mut input = PackagesInput::new(["bash", "vim", "unsafe; name"]);
        input.clean = true;
        input.update = true;
        input.nobest = true;

        assert_eq!(
            descriptions(&package_commands(&input, &installed)),
            ["dnf clean all", "dnf update -y", "dnf install -y --nobest vim 'unsafe; name'"]
        );
    }

    #[test]
    fn latest_uses_dnf_update_for_requested_packages() {
        let installed = [("bash".to_owned(), vec!["5.2".to_owned()])].into_iter().collect();
        let mut input = PackagesInput::new(["bash"]);
        input.latest = true;

        assert_eq!(descriptions(&package_commands(&input, &installed)), ["dnf update -y bash"]);
    }
}
