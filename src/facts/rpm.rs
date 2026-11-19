use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

pub type InstalledPackages = IndexMap<String, Vec<String>>;

#[derive(Clone, Copy, Debug, Default)]
pub struct RpmPackages;

impl Fact for RpmPackages {
    type Input = ();
    type Output = InstalledPackages;

    fn name(&self) -> &'static str {
        "rpm.RpmPackages"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new(vec![
            CommandBit::from("rpm --queryformat"),
            CommandBit::quoted("%{NAME} %{VERSION}-%{RELEASE}\\n"),
            CommandBit::from("-qa"),
        ])
    }

    fn default(&self) -> Self::Output {
        InstalledPackages::new()
    }

    fn requires_command(&self, (): &Self::Input) -> Option<&str> {
        Some("rpm")
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = InstalledPackages::new();
        for line in output {
            let mut fields = line.split_whitespace();
            let (Some(name), Some(version), None) = (fields.next(), fields.next(), fields.next()) else {
                continue;
            };
            packages.entry(name.to_owned()).or_default().push(version.to_owned());
        }
        Ok(packages)
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, RpmPackages};

    #[test]
    fn command_and_parser_match_rpm_package_shape() {
        let packages =
            RpmPackages.process(&["bash 5.2.26-1".to_owned(), "invalid extra fields here".to_owned()], &()).unwrap();

        assert_eq!(RpmPackages.command(&()).raw_value(), "rpm --queryformat '%{NAME} %{VERSION}-%{RELEASE}\\n' -qa");
        assert_eq!(packages["bash"], ["5.2.26-1"]);
    }
}
