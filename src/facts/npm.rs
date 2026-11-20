use std::error::Error;

use crate::api::{CommandBit, StringCommand};

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NpmPackagesInput {
    pub directory: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NpmPackages;

impl Fact for NpmPackages {
    type Input = NpmPackagesInput;
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "npm.NpmPackages"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        input.directory.as_ref().map_or_else(
            || StringCommand::new([CommandBit::from("npm list -g --depth=0")]),
            |directory| {
                StringCommand::new([
                    CommandBit::from("! test -d"),
                    CommandBit::quoted(directory.clone()),
                    CommandBit::from("|| (cd"),
                    CommandBit::quoted(directory.clone()),
                    CommandBit::from("&& npm list -g --depth=0)"),
                ])
            },
        )
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("npm")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            let Some(package) = line.strip_prefix("├── ").or_else(|| line.strip_prefix("└── ")) else {
                continue;
            };
            let Some((name, version)) = package.rsplit_once('@') else {
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
    use super::{Fact, NpmPackages, NpmPackagesInput};

    #[test]
    fn parses_global_and_quotes_local_package_listing() {
        let packages = NpmPackages
            .process(&["├── pydash@3.48".to_owned(), "└── thing@1.9".to_owned()], &NpmPackagesInput::default())
            .unwrap();
        assert_eq!(packages["pydash"], ["3.48"]);
        assert_eq!(packages["thing"], ["1.9"]);

        let input = NpmPackagesInput { directory: Some("/app dir; id".to_owned()) };
        assert_eq!(
            NpmPackages.command(&input).raw_value(),
            "! test -d '/app dir; id' || (cd '/app dir; id' && npm list -g --depth=0)"
        );
    }
}
