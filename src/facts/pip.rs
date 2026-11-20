use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

pub type PipPackageMap = IndexMap<String, Vec<String>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipPackagesInput {
    pub pip: String,
}

impl Default for PipPackagesInput {
    fn default() -> Self {
        Self { pip: "pip".to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PipPackages;

impl Fact for PipPackages {
    type Input = PipPackagesInput;
    type Output = PipPackageMap;

    fn name(&self) -> &'static str {
        "pip.PipPackages"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::new([CommandBit::quoted(input.pip.clone()), CommandBit::from("freeze")])
    }

    fn default(&self) -> Self::Output {
        PipPackageMap::new()
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PipPackageMap::new();
        for line in output {
            let Some((name, version)) = line.split_once("==") else {
                continue;
            };
            if valid_package_name(name) && valid_version(version) {
                packages.entry(name.to_ascii_lowercase()).or_default().push(version.to_owned());
            }
        }
        Ok(packages)
    }
}

fn valid_package_name(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'.'))
}

fn valid_version(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{Fact, PipPackages, PipPackagesInput};

    #[test]
    fn parses_freeze_output_and_normalizes_package_names() {
        let packages = PipPackages
            .process(
                &[
                    "Django==5.1.2".to_owned(),
                    "typing_extensions==4.12.0".to_owned(),
                    "editable @ file:///src/editable".to_owned(),
                ],
                &PipPackagesInput::default(),
            )
            .unwrap();

        assert_eq!(packages["django"], ["5.1.2"]);
        assert_eq!(packages["typing_extensions"], ["4.12.0"]);
        assert_eq!(
            PipPackages.command(&PipPackagesInput { pip: "/env path/bin/pip".to_owned() }).raw_value(),
            "'/env path/bin/pip' freeze"
        );
    }
}
