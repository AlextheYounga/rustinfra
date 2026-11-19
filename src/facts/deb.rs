use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

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
