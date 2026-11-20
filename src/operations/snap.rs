use std::iter::once;

use indexmap::IndexMap;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::snap::{SnapPackage, SnapPackageInfo, SnapPackages};
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageInput {
    pub packages: Vec<String>,
    pub channel: String,
    pub classic: bool,
    pub present: bool,
}

impl PackageInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            channel: "latest/stable".to_owned(),
            classic: false,
            present: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Package;

impl StatefulOperation for Package {
    type Input = PackageInput;

    fn name(&self) -> &'static str {
        "snap.package"
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
            let executor = FactExecutor::new(connector, fact_options);
            let installed = executor.get(&SnapPackages, &()).await?;
            let mut info = IndexMap::new();
            if input.present {
                for package in input.packages.iter().filter(|package| installed.contains(package)) {
                    info.insert(package.clone(), executor.get(&SnapPackage, package).await?);
                }
            }
            Ok(package_commands(input, &installed, &info))
        })
    }
}

fn package_commands(
    input: &PackageInput,
    installed: &[String],
    info: &IndexMap<String, SnapPackageInfo>,
) -> Vec<Command> {
    let install =
        input.packages.iter().filter(|package| input.present && !installed.contains(package)).collect::<Vec<_>>();
    let remove =
        input.packages.iter().filter(|package| !input.present && installed.contains(package)).collect::<Vec<_>>();
    let refresh = input
        .packages
        .iter()
        .filter(|package| {
            input.present
                && installed.contains(package)
                && info.get(*package).and_then(|value| value.get("channel")) != Some(&input.channel)
        })
        .collect::<Vec<_>>();

    let mut commands = Vec::new();
    if !install.is_empty() {
        let mut bits = vec![CommandBit::from("snap install")];
        if input.classic {
            bits.push(CommandBit::from("--classic"));
        }
        bits.extend(install.into_iter().map(|package| CommandBit::quoted(package.as_str())));
        bits.push(CommandBit::quoted(format!("--channel={}", input.channel)));
        commands.push(Command::Shell(StringCommand::new(bits)));
    }
    if !remove.is_empty() {
        commands.push(shell_with_packages("snap remove", remove));
    }
    if !refresh.is_empty() {
        let mut bits = vec![CommandBit::from("snap refresh")];
        bits.extend(refresh.into_iter().map(|package| CommandBit::quoted(package.as_str())));
        bits.push(CommandBit::quoted(format!("--channel={}", input.channel)));
        commands.push(Command::Shell(StringCommand::new(bits)));
    }
    commands
}

fn shell_with_packages<'a>(prefix: &str, packages: impl IntoIterator<Item = &'a String>) -> Command {
    Command::Shell(StringCommand::new(
        once(CommandBit::from(prefix)).chain(packages.into_iter().map(|package| CommandBit::quoted(package.as_str()))),
    ))
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{PackageInput, package_commands};
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
    fn matches_install_remove_and_refresh_reference_fixtures() {
        let mut input = PackageInput::new(["lxd", "present", "not_present"]);
        input.channel = "latest/edge".to_owned();
        let info = IndexMap::from([
            ("lxd".to_owned(), IndexMap::from([("channel".to_owned(), "latest/stable".to_owned())])),
            ("present".to_owned(), IndexMap::from([("channel".to_owned(), "latest/edge".to_owned())])),
        ]);
        assert_eq!(
            descriptions(&package_commands(&input, &["lxd".to_owned(), "present".to_owned()], &info)),
            ["snap install not_present --channel=latest/edge", "snap refresh lxd --channel=latest/edge"]
        );

        input.present = false;
        assert_eq!(
            descriptions(&package_commands(&input, &["lxd".to_owned(), "present".to_owned()], &IndexMap::new())),
            ["snap remove lxd present"]
        );
    }
}
