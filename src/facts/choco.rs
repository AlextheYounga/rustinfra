use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct ChocoPackages;

impl Fact for ChocoPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "choco.ChocoPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["choco list"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("choco")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            let mut fields = line.split_whitespace();
            let (Some(name), Some(version), None) = (fields.next(), fields.next(), fields.next()) else {
                continue;
            };
            if name.chars().all(|character| character.is_ascii_alphanumeric() || ".-+_".contains(character))
                && version.chars().all(|character| character.is_ascii_digit() || character == '.')
            {
                insert_package(&mut packages, name, version);
            }
        }
        Ok(packages)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ChocoVersion;

impl Fact for ChocoVersion {
    type Input = ();
    type Output = String;

    fn name(&self) -> &'static str {
        "choco.ChocoVersion"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["choco --version"])
    }

    fn default(&self) -> Self::Output {
        String::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("choco")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.concat().replace('\n', ""))
    }
}

#[cfg(test)]
mod tests {
    use super::{ChocoPackages, ChocoVersion, Fact};

    #[test]
    fn parses_reference_package_and_version_output() {
        let packages = ChocoPackages
            .process(
                &[
                    "Chocolatey v0.10.15".to_owned(),
                    "chocolatey-core.extension 1.3.5.1".to_owned(),
                    "1 packages installed.".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(packages["chocolatey-core.extension"], ["1.3.5.1"]);
        assert_eq!(ChocoVersion.process(&["0.10.15".to_owned(), String::new()], &()).unwrap(), "0.10.15");
    }
}
