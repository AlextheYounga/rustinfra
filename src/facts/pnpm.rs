use std::error::Error;

use serde_json::Value;

use crate::api::{CommandBit, StringCommand};

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PnpmPackagesInput {
    pub directory: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PnpmPackages;

impl Fact for PnpmPackages {
    type Input = PnpmPackagesInput;
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "pnpm.PnpmPackages"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        match &input.directory {
            Some(directory) => StringCommand::new([
                CommandBit::from("! test -d"),
                CommandBit::quoted(directory.as_str()),
                CommandBit::from("|| (cd"),
                CommandBit::quoted(directory.as_str()),
                CommandBit::from("&& pnpm list --depth=0 --json)"),
            ]),
            None => StringCommand::new(["pnpm --global list --depth=0 --json"]),
        }
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("pnpm")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let Ok(Value::Array(projects)) = serde_json::from_str::<Value>(&output.join("\n")) else {
            return Ok(PackageMap::new());
        };
        let mut packages = PackageMap::new();
        for project in projects {
            for field in ["dependencies", "devDependencies", "optionalDependencies"] {
                let Some(dependencies) = project.get(field).and_then(Value::as_object) else {
                    continue;
                };
                for (name, package) in dependencies {
                    if let Some(version) = package.get("version").and_then(Value::as_str)
                        && !version.is_empty()
                    {
                        insert_package(&mut packages, name, version);
                    }
                }
            }
        }
        Ok(packages)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PnpmModulesUpToDate;

impl Fact for PnpmModulesUpToDate {
    type Input = String;
    type Output = bool;

    fn name(&self) -> &'static str {
        "pnpm.PnpmModulesUpToDate"
    }

    fn command(&self, directory: &Self::Input) -> StringCommand {
        let directory = directory.trim_end_matches('/');
        StringCommand::new([
            CommandBit::from("if cmp -s"),
            CommandBit::quoted(format!("{directory}/pnpm-lock.yaml")),
            CommandBit::quoted(format!("{directory}/node_modules/.pnpm/lock.yaml")),
            CommandBit::from("; then echo up_to_date; else echo not_up_to_date; fi"),
        ])
    }

    fn default(&self) -> Self::Output {
        false
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().is_some_and(|line| line.trim() == "up_to_date"))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PnpmVersion;

impl Fact for PnpmVersion {
    type Input = ();
    type Output = String;

    fn name(&self) -> &'static str {
        "pnpm.PnpmVersion"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["pnpm --version"])
    }

    fn default(&self) -> Self::Output {
        String::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("pnpm")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.concat().trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, PnpmModulesUpToDate, PnpmPackages, PnpmPackagesInput, PnpmVersion};

    #[test]
    fn matches_reference_commands_and_parses_dependency_groups() {
        let input = PnpmPackagesInput { directory: Some("/app dir; reboot".to_owned()) };
        assert_eq!(
            PnpmPackages.command(&input).raw_value(),
            "! test -d '/app dir; reboot' || (cd '/app dir; reboot' && pnpm list --depth=0 --json)"
        );
        let output = [r#"[{"dependencies":{"a":{"version":"1"}},"devDependencies":{"b":{"version":"link:../b"}},"unsavedDependencies":{"c":{"version":"2"}}}]"#.to_owned()];
        let packages = PnpmPackages.process(&output, &input).unwrap();
        assert_eq!(packages["a"], ["1"]);
        assert_eq!(packages["b"], ["link:../b"]);
        assert!(!packages.contains_key("c"));
        assert!(PnpmPackages.process(&["invalid".to_owned()], &input).unwrap().is_empty());
    }

    #[test]
    fn lockfile_and_version_facts_match_reference_behavior() {
        let directory = "/opt/app/".to_owned();
        assert_eq!(
            PnpmModulesUpToDate.command(&directory).raw_value(),
            "if cmp -s /opt/app/pnpm-lock.yaml /opt/app/node_modules/.pnpm/lock.yaml ; then echo up_to_date; else echo not_up_to_date; fi"
        );
        assert!(PnpmModulesUpToDate.process(&["up_to_date".to_owned()], &directory).unwrap());
        assert_eq!(PnpmVersion.process(&["10.15.1\n".to_owned()], &()).unwrap(), "10.15.1");
    }
}
