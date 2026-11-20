use std::error::Error as StdError;

use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::packages::PackageMap;
use crate::facts::pnpm::{PnpmModulesUpToDate, PnpmPackages, PnpmPackagesInput};
use crate::facts::{FactExecutor, FactOptions};

use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub directory: Option<String>,
    pub dev: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            directory: None,
            dev: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "pnpm.packages"
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
            if input.dev && input.directory.is_none() {
                return Err(Box::new(PnpmOperationError::GlobalDevDependencies) as Box<dyn StdError + Send + Sync>);
            }
            let installed = FactExecutor::new(connector, fact_options)
                .get(&PnpmPackages, &PnpmPackagesInput { directory: input.directory.clone() })
                .await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &PackageMap) -> Vec<Command> {
    let mut changed = Vec::new();
    let mut upgrades = Vec::new();
    for package in &input.packages {
        let spec = PackageSpec::parse(package);
        let present = installed
            .get(spec.name)
            .is_some_and(|versions| spec.version.is_none_or(|version| versions.iter().any(|value| value == version)));
        if input.present && !present || !input.present && present {
            changed.push(if input.present { package.as_str() } else { spec.name });
        } else if input.present && input.latest && spec.version.is_none() && present {
            upgrades.push(package.as_str());
        }
    }

    let mut commands = Vec::new();
    if !changed.is_empty() {
        let action = if input.present { "add" } else { "remove" };
        let mut bits = pnpm_prefix(input.directory.as_deref());
        bits.push(CommandBit::from(action));
        if input.present && input.dev {
            bits.push(CommandBit::from("--save-dev"));
        }
        bits.extend(changed.into_iter().map(CommandBit::quoted));
        commands.push(Command::Shell(StringCommand::new(bits)));
    }
    if !upgrades.is_empty() {
        let mut bits = pnpm_prefix(input.directory.as_deref());
        bits.extend([CommandBit::from("update"), CommandBit::from("--latest")]);
        bits.extend(upgrades.into_iter().map(CommandBit::quoted));
        commands.push(Command::Shell(StringCommand::new(bits)));
    }
    commands
}

fn pnpm_prefix(directory: Option<&str>) -> Vec<CommandBit> {
    match directory {
        Some(directory) => vec![CommandBit::from("cd"), CommandBit::quoted(directory), CommandBit::from("&& pnpm")],
        None => vec![CommandBit::from("pnpm"), CommandBit::from("--global")],
    }
}

struct PackageSpec<'a> {
    name: &'a str,
    version: Option<&'a str>,
}

impl<'a> PackageSpec<'a> {
    fn parse(package: &'a str) -> Self {
        let split = package.rfind('@').filter(|index| *index > 0);
        let Some(index) = split else {
            return Self { name: package, version: None };
        };
        let (name, version) = (&package[..index], &package[index + 1..]);
        Self { name, version: (version != "latest").then_some(version) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageImportMethod {
    Auto,
    Clone,
    Copy,
    Hardlink,
}

impl PackageImportMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Clone => "clone",
            Self::Copy => "copy",
            Self::Hardlink => "hardlink",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallInput {
    pub directory: String,
    pub frozen_lockfile: Option<bool>,
    pub ignore_scripts: bool,
    pub package_import_method: Option<PackageImportMethod>,
}

impl InstallInput {
    #[must_use]
    pub fn new(directory: impl Into<String>) -> Self {
        Self { directory: directory.into(), frozen_lockfile: None, ignore_scripts: false, package_import_method: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Install;

impl StatefulOperation for Install {
    type Input = InstallInput;

    fn name(&self) -> &'static str {
        "pnpm.install"
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
            if FactExecutor::new(connector, fact_options).get(&PnpmModulesUpToDate, &input.directory).await? {
                return Ok(Vec::new());
            }
            Ok(install_commands(input))
        })
    }
}

fn install_commands(input: &InstallInput) -> Vec<Command> {
    let mut bits = pnpm_prefix(Some(&input.directory));
    bits.push(CommandBit::from("install"));
    if let Some(frozen) = input.frozen_lockfile {
        bits.push(CommandBit::from(if frozen { "--frozen-lockfile" } else { "--no-frozen-lockfile" }));
    }
    if input.ignore_scripts {
        bits.push(CommandBit::from("--ignore-scripts"));
    }
    if let Some(method) = input.package_import_method {
        bits.extend([CommandBit::from("--package-import-method"), CommandBit::from(method.as_str())]);
    }
    vec![Command::Shell(StringCommand::new(bits))]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunInput {
    pub script: String,
    pub directory: String,
    pub args: Vec<String>,
    pub if_present: bool,
}

impl RunInput {
    #[must_use]
    pub fn new(script: impl Into<String>, directory: impl Into<String>) -> Self {
        Self { script: script.into(), directory: directory.into(), args: Vec::new(), if_present: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Run;

impl Operation for Run {
    type Input = RunInput;

    fn name(&self) -> &'static str {
        "pnpm.run"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let mut bits = pnpm_prefix(Some(&input.directory));
        bits.push(CommandBit::from("run"));
        if input.if_present {
            bits.push(CommandBit::from("--if-present"));
        }
        bits.push(CommandBit::quoted(input.script.as_str()));
        bits.extend(input.args.iter().map(|arg| CommandBit::quoted(arg.as_str())));
        vec![Command::Shell(StringCommand::new(bits))]
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PnpmOperationError {
    #[error("pnpm has no global dev dependencies, `directory` is required")]
    GlobalDevDependencies,
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{InstallInput, PackageImportMethod, PackagesInput, Run, RunInput, install_commands, package_commands};
    use crate::api::Command;
    use crate::operations::Operation;

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
    fn packages_match_scoped_version_remove_latest_and_quoting_fixtures() {
        let installed = IndexMap::from([
            ("typescript".to_owned(), vec!["5.7.2".to_owned()]),
            ("@types/node".to_owned(), vec!["22.10.2".to_owned()]),
        ]);
        let mut input = PackagesInput::new(["typescript@latest", "@types/node", "x; reboot"]);
        input.latest = true;
        assert_eq!(
            descriptions(&package_commands(&input, &installed)),
            ["pnpm --global add 'x; reboot'", "pnpm --global update --latest typescript@latest @types/node"]
        );
        input.present = false;
        assert_eq!(
            descriptions(&package_commands(&input, &installed)),
            ["pnpm --global remove typescript @types/node"]
        );
    }

    #[test]
    fn install_and_run_match_all_option_and_quoting_fixtures() {
        let mut install = InstallInput::new("/app dir; reboot");
        install.frozen_lockfile = Some(true);
        install.ignore_scripts = true;
        install.package_import_method = Some(PackageImportMethod::Copy);
        assert_eq!(
            descriptions(&install_commands(&install)),
            ["cd '/app dir; reboot' && pnpm install --frozen-lockfile --ignore-scripts --package-import-method copy"]
        );

        let mut run = RunInput::new("build; reboot", "/app dir");
        run.if_present = true;
        run.args = vec!["--out".to_owned(), "/tmp/dist dir".to_owned()];
        assert_eq!(
            descriptions(&Run.commands(&run)),
            ["cd '/app dir' && pnpm run --if-present 'build; reboot' --out '/tmp/dist dir'"]
        );
    }
}
