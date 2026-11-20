use std::error::Error;

use crate::api::{CommandBit, StringCommand};

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct PacmanUnpackGroup;

impl Fact for PacmanUnpackGroup {
    type Input = String;
    type Output = Vec<String>;

    fn name(&self) -> &'static str {
        "pacman.PacmanUnpackGroup"
    }

    fn command(&self, package: &Self::Input) -> StringCommand {
        StringCommand::new([
            CommandBit::from("pacman -S --print-format %n"),
            CommandBit::quoted(package.clone()),
            CommandBit::from("|| true"),
        ])
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("pacman")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.to_vec())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PacmanPackages;

impl Fact for PacmanPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "pacman.PacmanPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["pacman -Q"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("pacman")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            if let Some((name, version)) = line.split_once(' ') {
                insert_package(&mut packages, name, version);
            }
        }
        Ok(packages)
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, PacmanPackages, PacmanUnpackGroup};

    #[test]
    fn group_command_quotes_input_and_packages_preserve_versions() {
        assert_eq!(
            PacmanUnpackGroup.command(&"my;package".to_owned()).raw_value(),
            "pacman -S --print-format %n 'my;package' || true"
        );
        let packages = PacmanPackages.process(&["lz4 1:1.9.2-1".to_owned(), "lshw B.02.20-3".to_owned()], &()).unwrap();
        assert_eq!(packages["lz4"], ["1:1.9.2-1"]);
        assert_eq!(packages["lshw"], ["B.02.20-3"]);
    }
}
