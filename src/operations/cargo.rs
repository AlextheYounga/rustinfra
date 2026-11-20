use crate::api::CommandBit;
use crate::connectors::Connector;
use crate::facts::cargo::CargoPackages;
use crate::facts::{FactExecutor, FactOptions};

use super::packages::{PackageCommands, PackagePolicy, converge_packages};
use super::{StatefulOperation, StatefulOperationFuture};

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
        "cargo.packages"
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
            let installed = FactExecutor::new(connector, fact_options).get(&CargoPackages, &()).await?;
            Ok(converge_packages(
                &input.packages,
                &installed,
                PackagePolicy { present: input.present, latest: input.latest },
                &PackageCommands {
                    install: vec![CommandBit::from("cargo install")],
                    uninstall: vec![CommandBit::from("cargo uninstall")],
                    upgrade: vec![CommandBit::from("cargo install")],
                    version_join: "@",
                },
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::PackagesInput;
    use crate::api::{Command, CommandBit};
    use crate::operations::packages::{PackageCommands, PackagePolicy, converge_packages};

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
    fn matches_add_and_remove_reference_fixtures() {
        let installed = IndexMap::from([
            ("git".to_owned(), vec!["1.0".to_owned()]),
            ("react".to_owned(), vec!["1".to_owned()]),
            ("test".to_owned(), vec!["1.1".to_owned()]),
            ("ansible".to_owned(), vec!["1.2.1.0".to_owned()]),
        ]);
        let commands = PackageCommands {
            install: vec![CommandBit::from("cargo install")],
            uninstall: vec![CommandBit::from("cargo uninstall")],
            upgrade: vec![CommandBit::from("cargo install")],
            version_join: "@",
        };
        let mut input = PackagesInput::new(["git@1.1", "react", "test@1.1", "ansible"]);
        assert_eq!(
            descriptions(&converge_packages(
                &input.packages,
                &installed,
                PackagePolicy { present: true, latest: false },
                &commands,
            )),
            ["cargo install git@1.1"]
        );
        input.present = false;
        assert_eq!(
            descriptions(&converge_packages(
                &input.packages,
                &installed,
                PackagePolicy { present: false, latest: false },
                &commands,
            )),
            ["cargo uninstall react test@1.1 ansible"]
        );
    }
}
