use std::iter::once;
use std::str::FromStr;

use pep508_rs::{Requirement, VerbatimUrl, VersionOrUrl};

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::packages::PackageMap;
use crate::facts::server::Path;
use crate::facts::uv::{UvInstalledPythonsByVersion, UvPipPackages, UvToolDir, UvTools};
use crate::facts::{FactExecutor, FactOptions};

use super::{Operation, StatefulOperation, StatefulOperationFuture};

mod venv;

pub use venv::{LinkMode, UvOperationError, Venv, VenvInput};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub requirements: Option<String>,
    pub present: bool,
    pub latest: bool,
    pub extra_arguments: Vec<String>,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            requirements: None,
            present: true,
            latest: false,
            extra_arguments: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;
    fn name(&self) -> &'static str {
        "uv.packages"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let installed = FactExecutor::new(connector, options).get(&UvPipPackages, &()).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &PackageMap) -> Vec<Command> {
    let mut commands = Vec::new();
    if let Some(requirements) = &input.requirements {
        let action = if input.present { "install" } else { "uninstall" };
        let mut bits = uv_pip_bits(action, input.latest && input.present, &input.extra_arguments);
        bits.extend([raw("-r"), quoted(requirements)]);
        commands.push(shell(bits));
    }
    let specs = input.packages.iter().map(|package| PythonSpec::parse(package)).collect::<Vec<_>>();
    let changed = specs
        .iter()
        .filter(|spec| {
            let present = installed
                .get(&spec.name)
                .is_some_and(|versions| spec.version.as_ref().is_none_or(|version| versions.contains(version)));
            input.present != present
        })
        .collect::<Vec<_>>();
    let upgrades = specs
        .iter()
        .filter(|spec| input.present && input.latest && spec.version.is_none() && installed.contains_key(&spec.name))
        .collect::<Vec<_>>();
    if !changed.is_empty() {
        let mut bits = uv_pip_bits(if input.present { "install" } else { "uninstall" }, false, &input.extra_arguments);
        bits.extend(changed.iter().map(|spec| quoted(&spec.requirement)));
        commands.push(shell(bits));
    }
    if !upgrades.is_empty() {
        let mut bits = uv_pip_bits("install", true, &input.extra_arguments);
        bits.extend(upgrades.iter().map(|spec| quoted(&spec.requirement)));
        commands.push(shell(bits));
    }
    commands
}

fn uv_pip_bits(action: &str, upgrade: bool, extras: &[String]) -> Vec<CommandBit> {
    let mut bits = vec![raw("uv pip"), raw(action)];
    if upgrade {
        bits.push(raw("--upgrade"));
    }
    bits.push(raw("--no-progress"));
    bits.extend(extras.iter().map(|argument| quoted(argument)));
    bits
}

struct PythonSpec {
    name: String,
    version: Option<String>,
    requirement: String,
}

impl PythonSpec {
    fn parse(value: &str) -> Self {
        let Ok(parsed) = Requirement::<VerbatimUrl>::from_str(value) else {
            return Self { name: value.to_owned(), version: None, requirement: value.to_owned() };
        };
        let (version, requirement) = match &parsed.version_or_url {
            Some(VersionOrUrl::VersionSpecifier(specifiers)) if specifiers.len() > 1 => {
                return Self { name: value.to_owned(), version: None, requirement: value.to_owned() };
            }
            Some(VersionOrUrl::VersionSpecifier(specifiers)) if specifiers.len() == 1 => {
                (Some(specifiers[0].version().to_string()), parsed.to_string())
            }
            Some(VersionOrUrl::Url(url)) => (None, url.to_string()),
            _ => (None, parsed.to_string()),
        };
        Self { name: parsed.name.to_string().to_ascii_lowercase(), version, requirement }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonsInput {
    pub versions: Vec<String>,
    pub present: bool,
    pub managed: bool,
    pub extra_arguments: Vec<String>,
}

impl PythonsInput {
    #[must_use]
    pub fn new(versions: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            versions: versions.into_iter().map(Into::into).collect(),
            present: true,
            managed: true,
            extra_arguments: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Pythons;

impl StatefulOperation for Pythons {
    type Input = PythonsInput;
    fn name(&self) -> &'static str {
        "uv.pythons"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let installed =
                FactExecutor::new(connector, options).get(&UvInstalledPythonsByVersion, &input.managed).await?;
            Ok(input
                .versions
                .iter()
                .filter(|version| installed.contains_key(*version) != input.present)
                .map(|version| {
                    let mut bits = vec![
                        raw("uv python"),
                        raw(if input.present { "install" } else { "uninstall" }),
                        raw("--no-progress"),
                    ];
                    if input.managed {
                        bits.push(raw("--managed-python"));
                    }
                    bits.extend(input.extra_arguments.iter().map(|argument| quoted(argument)));
                    bits.push(quoted(version));
                    shell(bits)
                })
                .collect())
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolsInput {
    pub tools: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub extra_arguments: Vec<String>,
}

impl ToolsInput {
    #[must_use]
    pub fn new(tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            tools: tools.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            extra_arguments: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tools;

impl StatefulOperation for Tools {
    type Input = ToolsInput;
    fn name(&self) -> &'static str {
        "uv.tools"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let installed = FactExecutor::new(connector, options).get(&UvTools, &()).await?;
            Ok(tool_commands(input, &installed))
        })
    }
}

fn tool_commands(input: &ToolsInput, installed: &PackageMap) -> Vec<Command> {
    input
        .tools
        .iter()
        .filter_map(|tool| {
            let spec = PythonSpec::parse(tool);
            let present = installed
                .get(&spec.name)
                .is_some_and(|versions| spec.version.as_ref().is_none_or(|version| versions.contains(version)));
            let upgrade = input.present && input.latest && spec.version.is_none() && present;
            if input.present == present && !upgrade {
                return None;
            }
            let mut bits = vec![raw("uv tool"), raw(if input.present { "install" } else { "uninstall" })];
            if upgrade {
                bits.push(raw("--upgrade"));
            }
            bits.push(raw("--no-progress"));
            bits.extend(input.extra_arguments.iter().map(|argument| quoted(argument)));
            bits.push(quoted(&spec.requirement));
            Some(shell(bits))
        })
        .collect()
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolsUpgradeAllInput {
    pub extra_arguments: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ToolsUpgradeAll;

impl Operation for ToolsUpgradeAll {
    type Input = ToolsUpgradeAllInput;
    fn name(&self) -> &'static str {
        "uv.tools_upgrade_all"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![shell(
            once(raw("uv tool upgrade --all")).chain(input.extra_arguments.iter().map(|argument| quoted(argument))),
        )]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ToolsUpdateShell;

impl StatefulOperation for ToolsUpdateShell {
    type Input = ();
    fn name(&self) -> &'static str {
        "uv.tools_update_shell"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        _input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let executor = FactExecutor::new(connector, options);
            let tool_directory = executor.get(&UvToolDir, &()).await?;
            let path = executor.get(&Path, &()).await?.unwrap_or_default();
            Ok((!path.split(':').any(|directory| directory == tool_directory))
                .then(|| shell([raw("uv tool update-shell")]))
                .into_iter()
                .collect())
        })
    }
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests;
