use std::error::Error;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

/// Returns the system architecture according to `uname`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Arch;

impl Fact for Arch {
    type Input = ();
    type Output = Option<String>;

    fn name(&self) -> &'static str {
        "server.Arch"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new(["uname -m"])
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(Some(output.join("\n")))
    }
}

/// Returns raw output from a caller-supplied command.
#[derive(Clone, Copy, Debug, Default)]
pub struct Command;

impl Fact for Command {
    type Input = String;
    type Output = Option<String>;

    fn name(&self) -> &'static str {
        "server.Command"
    }

    fn command(&self, command: &Self::Input) -> StringCommand {
        StringCommand::new([command.clone()])
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(Some(output.join("\n")))
    }
}

/// Returns the path to a command, or `None` when it is unavailable.
#[derive(Clone, Copy, Debug, Default)]
pub struct Which;

impl Fact for Which {
    type Input = String;
    type Output = Option<String>;

    fn name(&self) -> &'static str {
        "server.Which"
    }

    fn command(&self, command: &Self::Input) -> StringCommand {
        StringCommand::new(vec![
            CommandBit::from("command -v"),
            CommandBit::quoted(command.clone()),
            CommandBit::from("|| true"),
        ])
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().cloned())
    }
}

/// Returns all available locale names, normalizing `.utf8` to `.UTF-8`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Locales;

impl Fact for Locales {
    type Input = ();
    type Output = Vec<String>;

    fn name(&self) -> &'static str {
        "server.Locales"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new(["locale -a"])
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn requires_command(&self, (): &Self::Input) -> Option<&str> {
        Some("locale")
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let locales = output.iter().map(|locale| locale.replace("utf8", "UTF-8")).collect();
        Ok(locales)
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, Locales};

    #[test]
    fn locales_match_reference_fixture_normalization() {
        let output = ["C", "C.UTF-8", "en_US.utf8", "fr_FR.utf8", "POSIX"].map(str::to_owned);

        let locales = Locales.process(&output, &()).unwrap();

        assert_eq!(locales, ["C", "C.UTF-8", "en_US.UTF-8", "fr_FR.UTF-8", "POSIX"]);
    }
}
