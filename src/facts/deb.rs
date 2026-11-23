use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

pub type InstalledPackages = IndexMap<String, Vec<String>>;

#[derive(Clone, Copy, Debug, Default)]
pub struct DebPackages;

impl Fact for DebPackages {
    type Input = ();
    type Output = InstalledPackages;

    fn name(&self) -> &'static str {
        "deb.DebPackages"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new(["dpkg -l"])
    }

    fn default(&self) -> Self::Output {
        InstalledPackages::new()
    }

    fn requires_command(&self, (): &Self::Input) -> Option<&str> {
        Some("dpkg")
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = InstalledPackages::new();
        for line in output {
            let mut fields = line.split_whitespace();
            let Some(status) = fields.next() else {
                continue;
            };
            if status != "ii" && status != "hi" {
                continue;
            }
            let (Some(name), Some(version)) = (fields.next(), fields.next()) else {
                continue;
            };
            let name = name.split_once(':').map_or(name, |(name, _)| name);
            packages.entry(name.to_owned()).or_default().push(version.to_owned());
        }
        Ok(packages)
    }
}

#[cfg(test)]
mod parity_tests {
    use super::{DebPackage, Fact};
    #[test]
    fn package_file_query_quotes_input_and_parses_indented_control_fields() {
        let package = "/tmp/my package; whoami.deb".to_owned();
        assert!(DebPackage.command(&package).raw_value().contains("test -f '/tmp/my package; whoami.deb'"));
        let info =
            DebPackage.process(&[" Package:\tmy-package".to_owned(), " Version:\t1.0".to_owned()], &package).unwrap();
        assert_eq!(info.name.as_deref(), Some("my-package"));
        assert_eq!(info.version.as_deref(), Some("1.0"));
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DebPackageInfo {
    pub name: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DebPackage;
impl Fact for DebPackage {
    type Input = String;
    type Output = DebPackageInfo;
    fn name(&self) -> &'static str {
        "deb.DebPackage"
    }
    fn command(&self, package: &String) -> StringCommand {
        let package = StringCommand::new([CommandBit::quoted(package.clone())]).raw_value();
        StringCommand::new([format!(
            "test -f {package} && case {package} in *.deb) dpkg -I {package} ;; *) dpkg -s {package} 2>/dev/null || true ;; esac || dpkg -s {package} 2>/dev/null || true"
        )])
    }
    fn default(&self) -> Self::Output {
        DebPackageInfo::default()
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("dpkg")
    }
    fn process(&self, output: &[String], _: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut info = DebPackageInfo::default();
        for line in output {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("Package:") {
                info.name = Some(value.trim().to_owned());
            }
            if let Some(value) = line.strip_prefix("Version:") {
                info.version = Some(value.trim().to_owned());
            }
        }
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::{DebPackages, Fact};

    #[test]
    fn parses_installed_and_held_dpkg_packages_with_architecture_suffixes() {
        let packages = DebPackages
            .process(
                &[
                    "ii  curl:amd64  8.1.2-1  amd64  command line tool".to_owned(),
                    "hi  curl:i386  8.1.2-1  i386  command line tool".to_owned(),
                    "rc  old  1.0  amd64  removed".to_owned(),
                ],
                &(),
            )
            .unwrap();

        assert_eq!(packages["curl"], ["8.1.2-1", "8.1.2-1"]);
        assert!(!packages.contains_key("old"));
        assert_eq!(DebPackages.requires_command(&()), Some("dpkg"));
    }
}
