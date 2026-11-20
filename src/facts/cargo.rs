use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct CargoPackages;

impl Fact for CargoPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "cargo.CargoPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["cargo", "install", "--list"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("cargo")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            if line.starts_with(char::is_whitespace) {
                continue;
            }
            let mut fields = line.split_whitespace();
            let (Some(name), Some(version)) = (fields.next(), fields.next()) else {
                continue;
            };
            let Some(version) = version.strip_prefix('v').map(|version| version.trim_end_matches(':')) else {
                continue;
            };
            if valid_name(name) && valid_version(version) {
                insert_package(&mut packages, name, version);
            }
        }
        Ok(packages)
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_version(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
}

#[cfg(test)]
mod tests {
    use super::{CargoPackages, Fact};

    #[test]
    fn parses_reference_package_listing() {
        let packages = CargoPackages
            .process(
                &[
                    "vigil-server v1.25.2:".to_owned(),
                    "    vigil".to_owned(),
                    "zee v0.4.0 (https://github.com/zee-editor/zee#08c9f31d):".to_owned(),
                ],
                &(),
            )
            .unwrap();

        assert_eq!(packages["vigil-server"], ["1.25.2"]);
        assert_eq!(packages["zee"], ["0.4.0"]);
        assert_eq!(CargoPackages.command(&()).raw_value(), "cargo install --list");
    }
}
