use std::error::Error;

use indexmap::IndexMap;
use serde_json::Value;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BrewItemKind {
    Cask,
    Command,
    Formula,
    Tap,
}

impl BrewItemKind {
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Cask => "casks",
            Self::Command => "commands",
            Self::Formula => "formulae",
            Self::Tap => "taps",
        }
    }

    #[must_use]
    pub fn option(self) -> &'static str {
        match self {
            Self::Cask => "--cask",
            Self::Command => "--command",
            Self::Formula => "--formula",
            Self::Tap => "--tap",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BrewVersion;

impl Fact for BrewVersion {
    type Input = ();
    type Output = [u32; 3];

    fn name(&self) -> &'static str {
        "brew.BrewVersion"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["brew --version"])
    }

    fn default(&self) -> Self::Output {
        [0, 0, 0]
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("brew")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let Some(version) = output.first().and_then(|line| line.strip_prefix("Homebrew ")) else {
            return Ok(self.default());
        };
        let mut numbers = version
            .split('.')
            .take(3)
            .map(|part| part.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u32>().ok());
        let parsed = [numbers.next().flatten(), numbers.next().flatten(), numbers.next().flatten()];
        Ok(match parsed {
            [Some(major), Some(minor), Some(patch)] => [major, minor, patch],
            _ => self.default(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BrewPackages;

impl Fact for BrewPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "brew.BrewPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new([
            r#"if brew --version | grep -q -E "Homebrew 0\."; then brew list --versions; else brew info --json --installed; fi"#,
        ])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("brew")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_brew_packages(output, true))
    }
}

fn parse_brew_packages(output: &[String], json: bool) -> PackageMap {
    let body = output.join("\n").trim().to_owned();
    if json && body.starts_with('[') {
        let Ok(Value::Array(formulae)) = serde_json::from_str::<Value>(&body) else {
            return PackageMap::new();
        };
        let mut packages = PackageMap::new();
        for formula in formulae {
            let versions = formula
                .get("installed")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("version").and_then(Value::as_str))
                .collect::<Vec<_>>();
            for key in ["name", "full_name"] {
                if let Some(name) = formula.get(key).and_then(Value::as_str) {
                    for version in &versions {
                        if !packages.get(name).is_some_and(|values| values.iter().any(|value| value == version)) {
                            insert_package(&mut packages, name, version);
                        }
                    }
                }
            }
        }
        return packages;
    }
    let mut packages = PackageMap::new();
    for line in output {
        let mut fields = line.split_whitespace();
        if let (Some(name), Some(version)) = (fields.next(), fields.next()) {
            insert_package(&mut packages, name, version);
        }
    }
    packages
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BrewCasks;

impl Fact for BrewCasks {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "brew.BrewCasks"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new([
            r#"if brew --version | grep -q -e "Homebrew\ +(1\.|2\.[0-5]).*" 1>/dev/null;then brew cask list --versions; else brew list --cask --versions; fi"#,
        ])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("brew")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_brew_packages(output, false))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BrewTaps;

impl Fact for BrewTaps {
    type Input = ();
    type Output = Vec<String>;

    fn name(&self) -> &'static str {
        "brew.BrewTaps"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["brew tap"])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("brew")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.to_vec())
    }
}

pub type BrewTrustMap = IndexMap<String, Vec<String>>;

#[derive(Clone, Copy, Debug, Default)]
pub struct BrewTrusted;

impl Fact for BrewTrusted {
    type Input = ();
    type Output = BrewTrustMap;

    fn name(&self) -> &'static str {
        "brew.BrewTrusted"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["brew trust --json=v1"])
    }
    fn default(&self) -> Self::Output {
        empty_trust_map()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("brew")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let Ok(Value::Object(value)) = serde_json::from_str::<Value>(&output.join("\n")) else {
            return Ok(empty_trust_map());
        };
        let mut result = empty_trust_map();
        for kind in [BrewItemKind::Cask, BrewItemKind::Command, BrewItemKind::Formula, BrewItemKind::Tap] {
            let Some(items) = value.get(kind.key()).and_then(Value::as_array) else {
                return Ok(empty_trust_map());
            };
            result.insert(kind.key().to_owned(), items.iter().filter_map(Value::as_str).map(str::to_owned).collect());
        }
        Ok(result)
    }
}

fn empty_trust_map() -> BrewTrustMap {
    [BrewItemKind::Cask, BrewItemKind::Command, BrewItemKind::Formula, BrewItemKind::Tap]
        .into_iter()
        .map(|kind| (kind.key().to_owned(), Vec::new()))
        .collect()
}

#[must_use]
pub fn new_cask_cli(version: [u32; 3]) -> bool {
    version[0] >= 3 || version[0] >= 2 && version[1] >= 6
}

#[cfg(test)]
mod tests {
    use super::{BrewPackages, BrewTrusted, BrewVersion, Fact, new_cask_cli};

    #[test]
    fn parses_versions_json_packages_and_trust_defaults() {
        assert_eq!(BrewVersion.process(&["Homebrew 3.1.7-62-g9".to_owned()], &()).unwrap(), [3, 1, 7]);
        assert!(new_cask_cli([2, 6, 0]));
        let packages = BrewPackages.process(&[r#"[{"name":"python@3.13","full_name":"homebrew/core/python@3.13","installed":[{"version":"3.13.2"}]}]"#.to_owned()], &()).unwrap();
        assert_eq!(packages["python@3.13"], ["3.13.2"]);
        assert_eq!(packages["homebrew/core/python@3.13"], ["3.13.2"]);
        assert!(BrewTrusted.process(&["{}".to_owned()], &()).unwrap()["taps"].is_empty());
    }
}
