use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::choco::ChocoPackages;
use crate::facts::{FactExecutor, FactOptions};

use super::packages::{PackageCommands, PackagePolicy, converge_packages};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

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
        "choco.packages"
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
            let installed = FactExecutor::new(connector, fact_options).get(&ChocoPackages, &()).await?;
            Ok(converge_packages(
                &input.packages,
                &installed,
                PackagePolicy { present: input.present, latest: input.latest },
                &PackageCommands {
                    install: vec![CommandBit::from("choco install -y")],
                    uninstall: vec![CommandBit::from("choco uninstall -y -x")],
                    upgrade: vec![CommandBit::from("choco update -y")],
                    version_join: ":",
                },
            ))
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Install;

impl Operation for Install {
    type Input = ();

    fn name(&self) -> &'static str {
        "choco.install"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![Command::Shell(StringCommand::new([
            "Set-ExecutionPolicy Bypass -Scope Process -Force ;iex ((New-Object System.Net.WebClient).DownloadString(\"https://chocolatey.org/install.ps1\"))",
        ]))]
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{Install, PackagesInput};
    use crate::api::{Command, CommandBit};
    use crate::operations::Operation;
    use crate::operations::packages::{PackageCommands, PackagePolicy, converge_packages};

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(value) => Some(value.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn matches_package_and_install_reference_fixtures() {
        let installed = IndexMap::from([("notepadplusplus".to_owned(), vec!["7.8.3".to_owned()])]);
        let mut input = PackagesInput::new(["notepadplusplus"]);
        input.present = false;
        let commands = PackageCommands {
            install: vec![CommandBit::from("choco install -y")],
            uninstall: vec![CommandBit::from("choco uninstall -y -x")],
            upgrade: vec![CommandBit::from("choco update -y")],
            version_join: ":",
        };
        assert_eq!(
            descriptions(&converge_packages(
                &input.packages,
                &installed,
                PackagePolicy { present: false, latest: false },
                &commands
            )),
            ["choco uninstall -y -x notepadplusplus"]
        );
        assert!(descriptions(&Install.commands(&())).first().is_some_and(|command| command.contains("install.ps1")));
    }
}
