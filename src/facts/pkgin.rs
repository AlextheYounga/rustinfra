use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct PkginPackages;

impl Fact for PkginPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "pkgin.PkginPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["pkgin", "list"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("pkgin")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            let Some(package) = line.split_whitespace().next() else {
                continue;
            };
            let Some((name, version)) = package.rsplit_once('-') else {
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
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{Fact, PkginPackages};

    #[test]
    fn parses_reference_package_listing() {
        let packages = PkginPackages
            .process(
                &[
                    "SDL2-2.0.14nb2       Simple DirectMedia Layer".to_owned(),
                    "ca-certificates-20200601nb1 Root CA certificates".to_owned(),
                ],
                &(),
            )
            .unwrap();

        assert_eq!(packages["SDL2"], ["2.0.14nb2"]);
        assert_eq!(packages["ca-certificates"], ["20200601nb1"]);
        assert_eq!(PkginPackages.command(&()).raw_value(), "pkgin list");
    }
}
