use crate::api::CommandBit;
use crate::connectors::Connector;
use crate::facts::gem::GemPackages;
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
        "gem.packages"
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
            let installed = FactExecutor::new(connector, fact_options).get(&GemPackages, &()).await?;
            Ok(converge_packages(
                &input.packages,
                &installed,
                PackagePolicy { present: input.present, latest: input.latest },
                &PackageCommands {
                    install: vec![CommandBit::from("gem install")],
                    uninstall: vec![CommandBit::from("gem uninstall")],
                    upgrade: vec![CommandBit::from("gem update")],
                    version_join: ":",
                },
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

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
        let commands = PackageCommands {
            install: vec![CommandBit::from("gem install")],
            uninstall: vec![CommandBit::from("gem uninstall")],
            upgrade: vec![CommandBit::from("gem update")],
            version_join: ":",
        };
        assert_eq!(
            descriptions(&converge_packages(
                &["rubyenv".to_owned()],
                &IndexMap::new(),
                PackagePolicy { present: true, latest: false },
                &commands,
            )),
            ["gem install rubyenv"]
        );
        let installed = IndexMap::from([("rubyenv".to_owned(), vec![String::new()])]);
        assert_eq!(
            descriptions(&converge_packages(
                &["rubyenv".to_owned()],
                &installed,
                PackagePolicy { present: false, latest: false },
                &commands,
            )),
            ["gem uninstall rubyenv"]
        );
    }
}
