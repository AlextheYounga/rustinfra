use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct ApkPackages;

impl Fact for ApkPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "apk.ApkPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["apk list --installed"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("apk")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            let Some(package) = line.split_whitespace().next() else {
                continue;
            };
            let Some(release) = package.rfind("-r") else {
                continue;
            };
            let Some(version_start) = package[..release].rfind('-') else {
                continue;
            };
            let (name, version) = (&package[..version_start], &package[version_start + 1..]);
            if !name.is_empty() && !version.is_empty() {
                insert_package(&mut packages, name, version);
            }
        }
        Ok(packages)
    }
}

#[cfg(test)]
mod tests {
    use super::{ApkPackages, Fact};

    #[test]
    fn parses_names_with_hyphens_and_release_versions() {
        let packages = ApkPackages
            .process(
                &[
                    "WARNING: Ignoring APKINDEX: missing".to_owned(),
                    "acct-openrc-6.6.4-r2 x86_64 {acct} (GPL) [installed]".to_owned(),
                    "linux-firmware-rtl_nic-20220509-r1 x86_64 {linux-firmware} (custom) [installed]".to_owned(),
                ],
                &(),
            )
            .unwrap();

        assert_eq!(packages["acct-openrc"], ["6.6.4-r2"]);
        assert_eq!(packages["linux-firmware-rtl_nic"], ["20220509-r1"]);
        assert_eq!(ApkPackages.command(&()).raw_value(), "apk list --installed");
    }
}
