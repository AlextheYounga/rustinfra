use std::error::Error;

use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::deb::{DebPackages, InstalledPackages};
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

mod extra;
mod repo;
pub use extra::{
    Deb, DebInput, DistUpgrade, Hold, HoldInput, Ppa, PpaInput, Update, UpdateInput, Upgrade, UpgradeInput,
};
pub use repo::{Repo, RepoInput, SourcesFile, SourcesFileInput};

const APT_PREFIX: &str = "DEBIAN_FRONTEND=noninteractive apt-get -y";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub update: bool,
    pub force: bool,
    pub no_recommends: bool,
    pub allow_downgrades: bool,
    pub purge: bool,
    pub wait_for_lock_seconds: Option<u64>,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            update: false,
            force: false,
            no_recommends: false,
            allow_downgrades: false,
            purge: false,
            wait_for_lock_seconds: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "apt.packages"
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
            validate_packages(&input.packages)?;
            let installed = FactExecutor::new(connector, fact_options).get(&DebPackages, &()).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn validate_packages(packages: &[String]) -> Result<(), Box<dyn Error + Send + Sync>> {
    if packages.iter().any(|package| package_name(package).is_empty()) {
        Err(Box::new(AptOperationError::EmptyPackageName))
    } else {
        Ok(())
    }
}

fn package_commands(input: &PackagesInput, installed: &InstalledPackages) -> Vec<Command> {
    let mut commands = Vec::new();
    if input.update {
        commands.push(apt_command(input, "update", &[]));
    }

    let packages = input
        .packages
        .iter()
        .filter(|package| package_needs_change(package, input, installed))
        .map(|package| if input.present { package.clone() } else { package_name(package).to_owned() })
        .collect::<Vec<_>>();
    if packages.is_empty() {
        return commands;
    }
    let action = if input.present {
        "install"
    } else if input.purge {
        "purge"
    } else {
        "remove"
    };
    commands.push(apt_command(input, action, &packages));
    commands
}

fn package_needs_change(package: &str, input: &PackagesInput, installed: &InstalledPackages) -> bool {
    let name = package_name(package);
    let versions = installed.get(name);
    if !input.present {
        return versions.is_some();
    }
    let Some((_, required_version)) = package.split_once('=') else {
        return versions.is_none() || input.latest;
    };
    versions.is_none_or(|versions| !versions.iter().any(|version| version == required_version))
}

fn package_name(package: &str) -> &str {
    package.split_once('=').map_or(package, |(name, _)| name).trim()
}

fn apt_command(input: &PackagesInput, action: &str, packages: &[String]) -> Command {
    let mut bits = vec![raw(APT_PREFIX)];
    if input.force {
        bits.push(raw("--force-yes"));
    }
    if let Some(timeout) = input.wait_for_lock_seconds {
        bits.extend([raw("-o"), raw(format!("DPkg::Lock::Timeout={timeout}"))]);
    }
    bits.extend([
        raw("-o"),
        raw("Dpkg::Options::=\"--force-confdef\""),
        raw("-o"),
        raw("Dpkg::Options::=\"--force-confold\""),
    ]);
    bits.push(raw(action));
    if action == "install" && input.no_recommends {
        bits.push(raw("--no-install-recommends"));
    }
    if action == "install" && input.allow_downgrades {
        bits.push(raw("--allow-downgrades"));
    }
    bits.extend(packages.iter().map(|package| CommandBit::quoted(package.clone())));
    Command::Shell(StringCommand::new(bits))
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AptOperationError {
    #[error("all packages must have non-empty names")]
    EmptyPackageName,
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
    fn installs_only_missing_or_wrong_version_packages_with_safe_quoting() {
        let mut installed = IndexMap::new();
        installed.insert("curl".to_owned(), vec!["8.0".to_owned()]);
        let input = PackagesInput::new(["curl=8.1", "vim", "bad; command"]);
        let commands = descriptions(&package_commands(&input, &installed));

        assert_eq!(commands.len(), 1);
        assert!(commands[0].ends_with("install curl=8.1 vim 'bad; command'"));
    }

    #[test]
    fn absent_packages_use_remove_or_purge_and_ignore_missing_packages() {
        let installed = [("vim".to_owned(), vec!["9".to_owned()])].into_iter().collect();
        let mut input = PackagesInput::new(["vim=9", "curl"]);
        input.present = false;
        input.purge = true;

        assert!(descriptions(&package_commands(&input, &installed))[0].ends_with("purge vim"));
    }
}
