use std::error::Error;
use std::fmt;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

mod linux_distribution;
mod sysctl;
mod system;
mod users;
pub use linux_distribution::{LinuxDistribution, LinuxDistributionInfo};
pub use sysctl::{Sysctl, SysctlInput, SysctlValue};
pub use system::*;
pub use users::{UserInfo, Users};

const TEMP_DIRECTORY_COMMAND: &str = "if [ -n \"$TMPDIR\" ] && [ -d \"$TMPDIR\" ] && [ -w \"$TMPDIR\" ]; then\n    echo \"$TMPDIR\";\nelif [ -n \"$TMP\" ] && [ -d \"$TMP\" ] && [ -w \"$TMP\" ]; then\n    echo \"$TMP\";\nelif [ -n \"$TEMP\" ] && [ -d \"$TEMP\" ] && [ -w \"$TEMP\" ]; then\n    echo \"$TEMP\";\nelse\n    echo \"\";\nfi";

macro_rules! string_fact {
    ($name:ident, $fact_name:literal, $command:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl Fact for $name {
            type Input = ();
            type Output = Option<String>;

            fn name(&self) -> &'static str {
                $fact_name
            }

            fn command(&self, (): &Self::Input) -> StringCommand {
                StringCommand::new([$command])
            }

            fn default(&self) -> Self::Output {
                None
            }

            fn process(
                &self,
                output: &[String],
                (): &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(output.first().cloned())
            }
        }
    };
}

string_fact!(User, "server.User", "echo $USER");
string_fact!(Path, "server.Path", "echo $PATH");
string_fact!(TmpDir, "server.TmpDir", TEMP_DIRECTORY_COMMAND);
string_fact!(Hostname, "server.Hostname", "uname -n");
string_fact!(Kernel, "server.Kernel", "uname -s");
string_fact!(KernelVersion, "server.KernelVersion", "uname -r");
string_fact!(Os, "server.Os", "uname -s");
string_fact!(OsVersion, "server.OsVersion", "uname -r");
string_fact!(Timezone, "server.Timezone", "readlink -f /etc/localtime | sed 's|.*/zoneinfo/||'");

pub(crate) fn is_bsd(os: Option<&str>) -> bool {
    os.is_some_and(|name| name.contains("BSD"))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HomeInput(pub(crate) String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsafeHomeUser(String);

impl fmt::Display for UnsafeHomeUser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Unsafe username for shell tilde expansion: {:?}", self.0)
    }
}

impl Error for UnsafeHomeUser {}

impl TryFrom<&str> for HomeInput {
    type Error = UnsafeHomeUser;

    fn try_from(user: &str) -> Result<Self, Self::Error> {
        let mut characters = user.chars();
        let safe_first = characters
            .next()
            .is_none_or(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_'));
        let safe_rest =
            characters.all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'));
        if safe_first && safe_rest { Ok(Self(user.to_owned())) } else { Err(UnsafeHomeUser(user.to_owned())) }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Home;

impl Fact for Home {
    type Input = HomeInput;
    type Output = Option<String>;

    fn name(&self) -> &'static str {
        "server.Home"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::with_separator([CommandBit::from("echo ~"), CommandBit::from(input.0.clone())], "")
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().cloned())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MacosVersion;

impl Fact for MacosVersion {
    type Input = ();
    type Output = Option<String>;

    fn name(&self) -> &'static str {
        "server.MacosVersion"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new(["sw_vers -productVersion"])
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn requires_command(&self, (): &Self::Input) -> Option<&str> {
        Some("sw_vers")
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().cloned())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Groups;

impl Fact for Groups {
    type Input = ();
    type Output = Vec<String>;

    fn name(&self) -> &'static str {
        "server.Groups"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new(["cat /etc/group"])
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().filter_map(|line| line.split_once(':').map(|(name, _)| name.to_owned())).collect())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadAverages {
    pub one_minute: f64,
    pub five_minutes: f64,
    pub fifteen_minutes: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LoadAverage;

impl Fact for LoadAverage {
    type Input = ();
    type Output = Option<LoadAverages>;

    fn name(&self) -> &'static str {
        "server.LoadAverage"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new(["cat /proc/loadavg 2>/dev/null || uptime"])
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().find_map(|line| parse_load_averages(line)))
    }
}

fn parse_load_averages(line: &str) -> Option<LoadAverages> {
    let trimmed = line.trim();
    let values = trimmed
        .split_once("load averages:")
        .or_else(|| trimmed.split_once("load average:"))
        .map_or(trimmed, |(_, values)| values);
    let parsed = values
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|value| !value.is_empty())
        .take(3)
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    match parsed.as_slice() {
        [one_minute, five_minutes, fifteen_minutes] => Some(LoadAverages {
            one_minute: *one_minute,
            five_minutes: *five_minutes,
            fifteen_minutes: *fifteen_minutes,
        }),
        _ => None,
    }
}

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
    use super::{
        Fact, Home, HomeInput, Hostname, LoadAverage, LoadAverages, Locales, MacosVersion, TmpDir, UnsafeHomeUser,
    };

    #[test]
    fn locales_match_reference_fixture_normalization() {
        let output = ["C", "C.UTF-8", "en_US.utf8", "fr_FR.utf8", "POSIX"].map(str::to_owned);

        let locales = Locales.process(&output, &()).unwrap();

        assert_eq!(locales, ["C", "C.UTF-8", "en_US.UTF-8", "fr_FR.UTF-8", "POSIX"]);
    }

    #[test]
    fn simple_server_facts_match_reference_commands_and_first_line_results() {
        assert_eq!(Hostname.command(&()).raw_value(), "uname -n");
        assert_eq!(Hostname.process(&["my-host.net".to_owned()], &()).unwrap().as_deref(), Some("my-host.net"));
        assert_eq!(TmpDir.command(&()).raw_value(), super::TEMP_DIRECTORY_COMMAND);
        assert_eq!(MacosVersion.requires_command(&()), Some("sw_vers"));
    }

    #[test]
    fn home_preserves_safe_tilde_expansion_and_rejects_shell_input() {
        let current = HomeInput::default();
        let user = HomeInput::try_from("some_user").unwrap();

        assert_eq!(Home.command(&current).raw_value(), "echo ~");
        assert_eq!(Home.command(&user).raw_value(), "echo ~some_user");
        assert_eq!(HomeInput::try_from("root; id"), Err(UnsafeHomeUser("root; id".to_owned())));
    }

    #[test]
    fn load_average_parses_procfs_and_bsd_uptime_formats() {
        let procfs = LoadAverage.process(&["0.12 0.21 0.22 1/145 71536".to_owned()], &()).unwrap();
        let freebsd = LoadAverage
            .process(&["12:03PM up 16 days, 2 users, load averages: 0.12, 0.21, 0.22".to_owned()], &())
            .unwrap();
        let expected = Some(LoadAverages { one_minute: 0.12, five_minutes: 0.21, fifteen_minutes: 0.22 });

        assert_eq!(procfs, expected);
        assert_eq!(freebsd, expected);
    }
}
