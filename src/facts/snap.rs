use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

pub type SnapPackageInfo = IndexMap<String, String>;

#[derive(Clone, Copy, Debug, Default)]
pub struct SnapPackage;

impl Fact for SnapPackage {
    type Input = String;
    type Output = SnapPackageInfo;

    fn name(&self) -> &'static str {
        "snap.SnapPackage"
    }

    fn command(&self, package: &Self::Input) -> StringCommand {
        StringCommand::new([CommandBit::from("snap info"), CommandBit::quoted(package.as_str())])
    }

    fn default(&self) -> Self::Output {
        SnapPackageInfo::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("snap")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut info = SnapPackageInfo::new();
        for line in output {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key {
                "name" | "publisher" | "snap-id" => {
                    info.insert(key.to_owned(), value.to_owned());
                }
                "tracking" => {
                    if let Some((track, remainder)) = value.split_once('/')
                        && let Some(risk) = remainder.split('/').next()
                    {
                        info.insert("channel".to_owned(), format!("{track}/{risk}"));
                    }
                }
                "installed" => {
                    if let Some(version) = value.split_whitespace().next() {
                        info.insert("version".to_owned(), version.to_owned());
                    }
                }
                _ => {}
            }
        }
        Ok(info)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SnapPackages;

impl Fact for SnapPackages {
    type Input = ();
    type Output = Vec<String>;

    fn name(&self) -> &'static str {
        "snap.SnapPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["snap list"])
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("snap")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().skip(1).filter_map(|line| line.split_whitespace().next().map(str::to_owned)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, SnapPackage, SnapPackages};

    #[test]
    fn parses_reference_package_info_and_list() {
        let package = "lxd".to_owned();
        let info = SnapPackage
            .process(
                &[
                    "name: lxd".to_owned(),
                    "publisher: Canonical✓".to_owned(),
                    "snap-id: abc".to_owned(),
                    "tracking: 4.0/stable/ubuntu-20.04".to_owned(),
                    "installed: 4.0.9-8e2046b (1) 71MB".to_owned(),
                ],
                &package,
            )
            .unwrap();
        assert_eq!(info["channel"], "4.0/stable");
        assert_eq!(info["version"], "4.0.9-8e2046b");
        assert_eq!(SnapPackage.command(&package).raw_value(), "snap info lxd");
        assert_eq!(
            SnapPackages.process(&["Name Version".to_owned(), "core 1".to_owned(), "lxd 2".to_owned()], &()).unwrap(),
            ["core", "lxd"]
        );
    }
}
