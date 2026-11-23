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
mod parity_tests {
    use super::{Fact, RpmPackage, RpmPackageProvides};

    #[test]
    fn individual_package_and_provider_quote_inputs() {
        let package = "/tmp/my package; whoami.rpm".to_owned();
        assert!(RpmPackage.command(&package).raw_value().contains(" -q '/tmp/my package; whoami.rpm'"));
        assert_eq!(RpmPackage.process(&["my-package 1.0-1".to_owned()], &package).unwrap().unwrap().version, "1.0-1");
        let provider = "my;package".to_owned();
        assert!(RpmPackageProvides.command(&provider).raw_value().contains("--whatprovides 'my;package'"));
        assert_eq!(
            RpmPackageProvides.process(&["vim 9.0-1".to_owned()], &provider).unwrap(),
            [("vim".to_owned(), "9.0-1".to_owned())]
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpmPackageInfo {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RpmPackage;
impl Fact for RpmPackage {
    type Input = String;
    type Output = Option<RpmPackageInfo>;
    fn name(&self) -> &'static str {
        "rpm.RpmPackage"
    }
    fn command(&self, package: &String) -> StringCommand {
        let package = StringCommand::new([CommandBit::quoted(package.clone())]).raw_value();
        StringCommand::new([format!(
            "rpm --queryformat '%{{NAME}} %{{VERSION}}-%{{RELEASE}}\\n' -q {package} || ! test -e {package} || rpm --queryformat '%{{NAME}} %{{VERSION}}-%{{RELEASE}}\\n' -qp {package} 2> /dev/null"
        )])
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("rpm")
    }
    fn process(&self, output: &[String], _: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().find_map(|line| {
            line.split_once(' ')
                .map(|(name, version)| RpmPackageInfo { name: name.to_owned(), version: version.to_owned() })
        }))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RpmPackageProvides;
impl Fact for RpmPackageProvides {
    type Input = String;
    type Output = Vec<(String, String)>;
    fn name(&self) -> &'static str {
        "rpm.RpmPackageProvides"
    }
    fn command(&self, package: &String) -> StringCommand {
        StringCommand::new([
            CommandBit::from("dnf repoquery --quiet --queryformat"),
            CommandBit::quoted("%{NAME} %{VERSION}-%{RELEASE}\\n"),
            CommandBit::from("--whatprovides"),
            CommandBit::quoted(package.clone()),
            CommandBit::from("|| true"),
        ])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("dnf")
    }
    fn process(&self, output: &[String], _: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| line.split_once(' ').map(|(name, version)| (name.to_owned(), version.to_owned())))
            .collect())
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
