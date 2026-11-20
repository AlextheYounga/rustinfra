use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct XbpsPackages;

impl Fact for XbpsPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "xbps.XbpsPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["xbps-query -l"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("xbps-query")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            let Some(package) = line.get(3..).and_then(|line| line.split_whitespace().next()) else {
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

#[derive(Clone, Copy, Debug, Default)]
pub struct XbpsSelfUpdate;

impl Fact for XbpsSelfUpdate {
    type Input = ();
    type Output = Vec<String>;

    fn name(&self) -> &'static str {
        "xbps.XbpsSelfUpdate"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["xbps-install -n -u xbps"])
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("xbps-install")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let has_xbps_update = output.iter().any(|line| {
            line.split_whitespace()
                .next()
                .and_then(|package| package.strip_prefix("xbps-"))
                .is_some_and(|version| !version.contains('-') && line.contains(" update "))
        });
        Ok(if has_xbps_update {
            output.iter().filter_map(|line| line.split_whitespace().next().map(str::to_owned)).collect()
        } else {
            Vec::new()
        })
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'.'))
}

fn valid_version(value: &str) -> bool {
    let Some((version, revision)) = value.rsplit_once('_') else {
        return false;
    };
    !version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.')
        && !revision.is_empty()
        && revision.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use std::slice;

    use super::{Fact, XbpsPackages, XbpsSelfUpdate};

    #[test]
    fn parses_package_names_with_dots() {
        let packages = XbpsPackages
            .process(
                &["ii python3-3.13.2_1 Python".to_owned(), "ii python3-jaraco.classes-3.4.0_2 Utility".to_owned()],
                &(),
            )
            .unwrap();
        assert_eq!(packages["python3"], ["3.13.2_1"]);
        assert_eq!(packages["python3-jaraco.classes"], ["3.4.0_2"]);
    }

    #[test]
    fn self_update_requires_an_xbps_package_not_only_triggers() {
        let triggers = "xbps-triggers-0.128_1 update noarch repo 13612 5228".to_owned();
        assert!(XbpsSelfUpdate.process(slice::from_ref(&triggers), &()).unwrap().is_empty());
        let xbps = "xbps-0.60.5_1 update x86_64 repo 1858672 607744".to_owned();
        assert_eq!(XbpsSelfUpdate.process(&[triggers, xbps], &()).unwrap(), ["xbps-triggers-0.128_1", "xbps-0.60.5_1"]);
    }
}
