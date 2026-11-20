use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct PipxPackages;

impl Fact for PipxPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "pipx.PipxPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["pipx list --short"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("pipx")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            let mut fields = line.split_whitespace();
            let (Some(name), Some(version), None) = (fields.next(), fields.next(), fields.next()) else {
                continue;
            };
            if name.chars().all(|character| character.is_ascii_alphanumeric() || "_-+.".contains(character))
                && version.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || ".-".contains(character)
                })
                && version.chars().next().is_some_and(|character| character.is_ascii_digit())
            {
                insert_package(&mut packages, name, version);
            }
        }
        Ok(packages)
    }
}

pub type PipxEnvironmentMap = IndexMap<String, String>;

#[derive(Clone, Copy, Debug, Default)]
pub struct PipxEnvironment;

impl Fact for PipxEnvironment {
    type Input = ();
    type Output = PipxEnvironmentMap;

    fn name(&self) -> &'static str {
        "pipx.PipxEnvironment"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["pipx environment"])
    }

    fn default(&self) -> Self::Output {
        PipxEnvironmentMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("pipx")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let (key, value) = line.split_once('=')?;
                (!key.is_empty() && key.chars().all(|character| character.is_ascii_uppercase() || character == '_'))
                    .then(|| (key.to_owned(), value.to_owned()))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, PipxEnvironment, PipxPackages};

    #[test]
    fn parses_reference_package_and_environment_output() {
        let packages = PipxPackages.process(&["copier 9.0.1".to_owned(), "invoke 2.2.0".to_owned()], &()).unwrap();
        assert_eq!(packages["copier"], ["9.0.1"]);
        assert_eq!(PipxPackages.command(&()).raw_value(), "pipx list --short");

        let environment = PipxEnvironment
            .process(&["PIPX_HOME=/home/me/.local/pipx".to_owned(), "invalid=value".to_owned()], &())
            .unwrap();
        assert_eq!(environment["PIPX_HOME"], "/home/me/.local/pipx");
        assert_eq!(environment.len(), 1);
    }
}
