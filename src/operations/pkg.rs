use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{File, PathInfo, PathInput};
use crate::facts::packages::PackageMap;
use crate::facts::pkg::PkgPackages;
use crate::facts::server::{Arch, Os, OsVersion, Which};
use crate::facts::{FactExecutor, FactOptions};

use super::packages::{PackageCommands, PackagePolicy, converge_packages};
use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub pkg_path: Option<String>,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { packages: packages.into_iter().map(Into::into).collect(), present: true, pkg_path: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "pkg.packages"
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
            let facts = FactExecutor::new(connector, fact_options);
            let installed = facts.get(&PkgPackages, &()).await?;
            let is_pkg = facts.get(&Which, &"pkg".to_owned()).await?.is_some();
            let mut pkg_path = input.pkg_path.clone();
            if input.present && pkg_path.is_none() {
                let install_url = facts.get(&File, &PathInput::from("/etc/installurl")).await?;
                if !matches!(install_url, PathInfo::Present(_)) {
                    let os = facts.get(&Os, &()).await?.unwrap_or_default();
                    let version = facts.get(&OsVersion, &()).await?.unwrap_or_default();
                    let arch = facts.get(&Arch, &()).await?.unwrap_or_default();
                    pkg_path =
                        Some(format!("http://ftp.{}.org/pub/{os}/{version}/packages/{arch}/", os.to_ascii_lowercase()));
                }
            }
            Ok(package_commands(input, &installed, is_pkg, pkg_path.as_deref()))
        })
    }
}

fn package_commands(
    input: &PackagesInput,
    installed: &PackageMap,
    is_pkg: bool,
    pkg_path: Option<&str>,
) -> Vec<Command> {
    let mut install = if is_pkg { vec![CommandBit::from("pkg install -y")] } else { vec![CommandBit::from("pkg_add")] };
    if let Some(path) = pkg_path {
        install.insert(
            0,
            CommandBit::from(StringCommand::with_separator(
                [CommandBit::from("PKG_PATH="), CommandBit::quoted(path)],
                "",
            )),
        );
    }
    converge_packages(
        &input.packages,
        installed,
        PackagePolicy { present: input.present, latest: false },
        &PackageCommands {
            install,
            uninstall: vec![CommandBit::from(if is_pkg { "pkg delete -y" } else { "pkg_delete" })],
            upgrade: Vec::new(),
            version_join: "=",
        },
    )
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
    fn selects_bsd_package_command_and_quotes_pkg_path() {
        let input = PackagesInput::new(["py-pip"]);
        assert_eq!(
            descriptions(&package_commands(
                &input,
                &IndexMap::new(),
                false,
                Some("http://ftp.os.org/pub/os/version/packages/arch/"),
            )),
            ["PKG_PATH=http://ftp.os.org/pub/os/version/packages/arch/ pkg_add py-pip"]
        );

        let mut input = input;
        input.present = false;
        let installed = IndexMap::from([("py-pip".to_owned(), Vec::new())]);
        assert_eq!(descriptions(&package_commands(&input, &installed, true, None)), ["pkg delete -y py-pip"]);
    }
}
