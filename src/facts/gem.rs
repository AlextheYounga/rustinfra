use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct GemPackages;

impl Fact for GemPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "gem.GemPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["gem", "list", "--local"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("gem")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            let Some((name, version)) = line.strip_suffix(')').and_then(|line| line.split_once(" (")) else {
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
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'+' | b'_'))
}

fn valid_version(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
}

#[cfg(test)]
mod tests {
    use super::{Fact, GemPackages};

    #[test]
    fn parses_reference_package_listing() {
        let packages = GemPackages.process(&["pydash (3.48)".to_owned()], &()).unwrap();

        assert_eq!(packages["pydash"], ["3.48"]);
        assert_eq!(GemPackages.command(&()).raw_value(), "gem list --local");
    }
}
