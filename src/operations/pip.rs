use std::str::FromStr;

use pep508_rs::{Requirement, VerbatimUrl, VersionOrUrl};

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{File, PathInfo, PathInput};
use crate::facts::pip::{PipPackageMap, PipPackages, PipPackagesInput};
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualenvInput {
    pub path: String,
    pub python: Option<String>,
    pub venv: bool,
    pub site_packages: bool,
    pub always_copy: bool,
    pub present: bool,
}

impl VirtualenvInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), python: None, venv: false, site_packages: false, always_copy: false, present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Virtualenv;

impl StatefulOperation for Virtualenv {
    type Input = VirtualenvInput;

    fn name(&self) -> &'static str {
        "pip.virtualenv"
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
            let activate = format!("{}/bin/activate", input.path.trim_end_matches('/'));
            let state = FactExecutor::new(connector, fact_options).get(&File, &PathInput::from(activate)).await?;
            Ok(virtualenv_commands(input, matches!(state, PathInfo::Present(_))))
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Venv;

impl StatefulOperation for Venv {
    type Input = VirtualenvInput;

    fn name(&self) -> &'static str {
        "pip.venv"
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
            let mut input = input.clone();
            input.venv = true;
            Virtualenv.commands(&input, connector, fact_options).await
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub requirements: Option<String>,
    pub pip: String,
    pub virtualenv: Option<VirtualenvInput>,
    pub extra_install_arguments: Vec<String>,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            requirements: None,
            pip: "pip".to_owned(),
            virtualenv: None,
            extra_install_arguments: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "pip.packages"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, fact_options);
            let mut commands = Vec::new();
            let (pip, environment_exists) = if let Some(environment) = &input.virtualenv {
                let activate = format!("{}/bin/activate", environment.path.trim_end_matches('/'));
                let state = facts.get(&File, &PathInput::from(activate)).await?;
                let exists = matches!(state, PathInfo::Present(_));
                commands.extend(virtualenv_commands(environment, exists));
                (format!("{}/bin/{}", environment.path.trim_end_matches('/'), input.pip), exists)
            } else {
                (input.pip.clone(), true)
            };
            let installed = if input.packages.is_empty() || !environment_exists {
                PipPackageMap::new()
            } else {
                facts.get(&PipPackages, &PipPackagesInput { pip: pip.clone() }).await?
            };
            commands.extend(package_commands(input, &pip, &installed));
            Ok(commands)
        })
    }
}

fn virtualenv_commands(input: &VirtualenvInput, exists: bool) -> Vec<Command> {
    if !input.present {
        return exists.then(|| shell([raw("rm"), raw("-rf"), quoted(&input.path)])).into_iter().collect();
    }
    if exists {
        return Vec::new();
    }
    let mut bits = if input.venv {
        vec![quoted(input.python.as_deref().unwrap_or("python")), raw("-m"), raw("venv")]
    } else {
        vec![raw("virtualenv")]
    };
    if !input.venv
        && let Some(python) = &input.python
    {
        bits.extend([raw("-p"), quoted(python)]);
    }
    if input.site_packages {
        bits.push(raw("--system-site-packages"));
    }
    if input.always_copy {
        bits.push(raw(if input.venv { "--copies" } else { "--always-copy" }));
    }
    bits.push(quoted(&input.path));
    vec![shell(bits)]
}

fn package_commands(input: &PackagesInput, pip: &str, installed: &PipPackageMap) -> Vec<Command> {
    let mut commands = Vec::new();
    if let Some(requirements) = &input.requirements {
        let mut bits = pip_action(pip, if input.present { "install" } else { "uninstall" });
        if !input.present {
            bits.push(raw("--yes"));
        } else if input.latest {
            bits.push(raw("--upgrade"));
        }
        bits.extend([raw("-r"), quoted(requirements)]);
        commands.push(shell(bits));
    }

    let requirements = input.packages.iter().map(|package| PythonRequirement::parse(package)).collect::<Vec<_>>();
    let changed = requirements
        .iter()
        .filter(|requirement| requirement.needs_change(input.present, input.latest, installed))
        .map(|requirement| requirement.original)
        .collect::<Vec<_>>();
    if changed.is_empty() {
        return commands;
    }
    let mut bits = pip_action(pip, if input.present { "install" } else { "uninstall" });
    if input.present {
        bits.extend(input.extra_install_arguments.iter().map(|argument| quoted(argument)));
        if input.latest {
            bits.push(raw("--upgrade"));
        }
    } else {
        bits.push(raw("--yes"));
    }
    bits.extend(changed.into_iter().map(quoted));
    commands.push(shell(bits));
    commands
}

struct PythonRequirement<'a> {
    original: &'a str,
    name: String,
    version: Option<String>,
}

impl<'a> PythonRequirement<'a> {
    fn parse(original: &'a str) -> Self {
        let parsed = Requirement::<VerbatimUrl>::from_str(original).ok();
        let name =
            parsed.as_ref().map_or_else(|| original.to_ascii_lowercase(), |requirement| requirement.name.to_string());
        let version = parsed.as_ref().and_then(|requirement| match &requirement.version_or_url {
            Some(VersionOrUrl::VersionSpecifier(specifiers)) if specifiers.len() == 1 => {
                Some(specifiers[0].version().to_string())
            }
            _ => None,
        });
        Self { original, name: name.to_ascii_lowercase(), version }
    }

    fn needs_change(&self, present: bool, latest: bool, installed: &PipPackageMap) -> bool {
        let versions = installed.get(&self.name);
        if !present {
            return versions
                .is_some_and(|versions| self.version.as_ref().is_none_or(|version| versions.contains(version)));
        }
        versions.is_none_or(|versions| self.version.as_ref().is_some_and(|version| !versions.contains(version)))
            || (latest && self.version.is_none())
    }
}

fn pip_action(pip: &str, action: &str) -> Vec<CommandBit> {
    vec![quoted(pip), raw(action)]
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
mod tests {
    use indexmap::IndexMap;

    use super::{PackagesInput, VirtualenvInput, package_commands, virtualenv_commands};
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
    fn virtualenv_creation_and_removal_match_reference_options() {
        let mut input = VirtualenvInput::new("/some/virtual env");
        input.python = Some("python 3".to_owned());
        input.site_packages = true;
        input.always_copy = true;
        assert_eq!(
            descriptions(&virtualenv_commands(&input, false)),
            ["virtualenv -p 'python 3' --system-site-packages --always-copy '/some/virtual env'"]
        );
        input.present = false;
        assert_eq!(descriptions(&virtualenv_commands(&input, true)), ["rm -rf '/some/virtual env'"]);
    }

    #[test]
    fn packages_converge_pep508_names_versions_and_requirements() {
        let installed = IndexMap::from([
            ("django".to_owned(), vec!["5.1".to_owned()]),
            ("requests".to_owned(), vec!["2.0".to_owned()]),
        ]);
        let mut input = PackagesInput::new(["Django==5.1", "requests==2.1", "new package"]);
        input.requirements = Some("requirements file.txt".to_owned());
        assert_eq!(
            descriptions(&package_commands(&input, "/env path/bin/pip", &installed)),
            [
                "'/env path/bin/pip' install -r 'requirements file.txt'",
                "'/env path/bin/pip' install requests==2.1 'new package'"
            ]
        );

        input.present = false;
        input.requirements = None;
        assert_eq!(descriptions(&package_commands(&input, "pip", &installed)), ["pip uninstall --yes Django==5.1"]);
    }
}
