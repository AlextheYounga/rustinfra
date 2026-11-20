use crate::api::{CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::npm::{NpmPackages, NpmPackagesInput};
use crate::facts::{FactExecutor, FactOptions};

use super::packages::{PackageCommands, PackagePolicy, converge_packages};
use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub directory: Option<String>,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { packages: packages.into_iter().map(Into::into).collect(), present: true, latest: false, directory: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "npm.packages"
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
            let installed = FactExecutor::new(connector, fact_options)
                .get(&NpmPackages, &NpmPackagesInput { directory: input.directory.clone() })
                .await?;
            let (install, uninstall, upgrade) = npm_commands(input.directory.as_deref());
            Ok(converge_packages(
                &input.packages,
                &installed,
                PackagePolicy { present: input.present, latest: input.latest },
                &PackageCommands { install, uninstall, upgrade, version_join: "@" },
            ))
        })
    }
}

fn npm_commands(directory: Option<&str>) -> (Vec<CommandBit>, Vec<CommandBit>, Vec<CommandBit>) {
    let prefix = directory.map(|directory| {
        CommandBit::from(StringCommand::new([
            CommandBit::from("cd"),
            CommandBit::quoted(directory),
            CommandBit::from("&& npm"),
        ]))
    });
    let action = |name| match &prefix {
        Some(prefix) => vec![prefix.clone(), CommandBit::from(name)],
        None => vec![CommandBit::from("npm"), CommandBit::from(name), CommandBit::from("-g")],
    };
    (action("install"), action("uninstall"), action("update"))
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::npm_commands;
    use crate::api::Command;
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
    fn matches_global_and_shell_safe_directory_reference_fixtures() {
        let installed =
            IndexMap::from([("git".to_owned(), vec!["1.0".to_owned()]), ("test".to_owned(), vec!["1.1".to_owned()])]);
        let packages = ["git@1.1".to_owned(), "react".to_owned(), "test@1.1".to_owned()];
        let (install, uninstall, upgrade) = npm_commands(None);
        assert_eq!(
            descriptions(&converge_packages(
                &packages,
                &installed,
                PackagePolicy { present: true, latest: false },
                &PackageCommands { install, uninstall, upgrade, version_join: "@" },
            )),
            ["npm install -g git@1.1 react"]
        );

        let (install, uninstall, upgrade) = npm_commands(Some("/app dir; rm -rf /"));
        assert_eq!(
            descriptions(&converge_packages(
                &["react".to_owned()],
                &IndexMap::new(),
                PackagePolicy { present: true, latest: false },
                &PackageCommands { install, uninstall, upgrade, version_join: "@" },
            )),
            ["cd '/app dir; rm -rf /' && npm install react"]
        );
    }
}
