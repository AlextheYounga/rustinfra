use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

pub type FlatpakPackageInfo = IndexMap<String, String>;

#[derive(Clone, Copy, Debug, Default)]
pub struct FlatpakPackage;

impl Fact for FlatpakPackage {
    type Input = String;
    type Output = FlatpakPackageInfo;

    fn name(&self) -> &'static str {
        "flatpak.FlatpakPackage"
    }

    fn command(&self, package: &Self::Input) -> StringCommand {
        StringCommand::new([CommandBit::from("flatpak info"), CommandBit::quoted(package.as_str())])
    }

    fn default(&self) -> Self::Output {
        FlatpakPackageInfo::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("flatpak")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut info = FlatpakPackageInfo::new();
        for line in output {
            let trimmed = line.trim_start();
            for (prefix, key) in [("ID:", "id"), ("Ref:", "ref"), ("Version:", "version")] {
                if let Some(value) = trimmed.strip_prefix(prefix).map(str::trim) {
                    info.insert(key.to_owned(), value.to_owned());
                    break;
                }
            }
        }
        Ok(info)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FlatpakPackages;

impl Fact for FlatpakPackages {
    type Input = bool;
    type Output = Vec<String>;

    fn name(&self) -> &'static str {
        "flatpak.FlatpakPackages"
    }

    fn command(&self, user: &Self::Input) -> StringCommand {
        StringCommand::new([if *user {
            "flatpak list --user --columns=application"
        } else {
            "flatpak list --columns=application"
        }])
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("flatpak")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().filter(|line| line.as_str() != "Application ID").cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, FlatpakPackage, FlatpakPackages};

    #[test]
    fn parses_reference_info_and_optional_list_header() {
        let package = "org.videolan.VLC".to_owned();
        let info = FlatpakPackage
            .process(
                &[
                    "          ID: org.videolan.VLC".to_owned(),
                    " Ref: app/org.videolan.VLC/x86_64/stable".to_owned(),
                    " Version: 3.0.21".to_owned(),
                ],
                &package,
            )
            .unwrap();
        assert_eq!(info["version"], "3.0.21");
        assert_eq!(FlatpakPackage.command(&package).raw_value(), "flatpak info org.videolan.VLC");
        assert_eq!(
            FlatpakPackages.process(&["Application ID".to_owned(), "org.videolan.VLC".to_owned()], &false).unwrap(),
            ["org.videolan.VLC"]
        );
        assert_eq!(FlatpakPackages.command(&true).raw_value(), "flatpak list --user --columns=application");
    }
}
