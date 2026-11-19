use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

const UNIT_SUFFIXES: &[&str] =
    &["service", "socket", "device", "mount", "automount", "swap", "target", "path", "timer", "slice", "scope"];

pub type UnitStates = IndexMap<String, bool>;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemdInput {
    pub user_mode: bool,
    pub machine: Option<String>,
    pub user_name: Option<String>,
    pub services: Vec<String>,
}

impl SystemdInput {
    #[must_use]
    pub fn for_service(service: impl Into<String>) -> Self {
        Self { services: vec![service.into()], ..Self::default() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemdStatus;

impl Fact for SystemdStatus {
    type Input = SystemdInput;
    type Output = UnitStates;

    fn name(&self) -> &'static str {
        "systemd.SystemdStatus"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        fact_command(input, "SubState")
    }

    fn default(&self) -> Self::Output {
        UnitStates::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("systemctl")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_states(
            output,
            "SubState",
            &["running", "waiting", "exited", "listening", "mounted", "active", "plugged"],
        ))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemdEnabled;

impl Fact for SystemdEnabled {
    type Input = SystemdInput;
    type Output = UnitStates;

    fn name(&self) -> &'static str {
        "systemd.SystemdEnabled"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        fact_command(input, "UnitFileState")
    }

    fn default(&self) -> Self::Output {
        UnitStates::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("systemctl")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_states(output, "UnitFileState", &["enabled", "static"]))
    }
}

pub(crate) fn systemctl_command(input: &SystemdInput) -> StringCommand {
    let mut bits = vec![CommandBit::from("systemctl")];
    if input.user_mode {
        bits.push(CommandBit::from("--user"));
    }
    let machine = match (&input.machine, &input.user_name) {
        (Some(machine), Some(user)) => Some(format!("--machine={user}@{machine}")),
        (Some(machine), None) => Some(format!("--machine={machine}")),
        (None, Some(user)) => Some(format!("--machine={user}@.host")),
        (None, None) => None,
    };
    if let Some(machine) = machine {
        bits.push(CommandBit::quoted(machine));
    }
    StringCommand::new(bits)
}

fn fact_command(input: &SystemdInput, property: &str) -> StringCommand {
    let services = if input.services.is_empty() { vec!["*".to_owned()] } else { input.services.clone() };
    let mut bits = vec![
        systemctl_command(input).into(),
        CommandBit::from("show"),
        CommandBit::from("--all"),
        CommandBit::from("--property"),
        CommandBit::from("Id"),
        CommandBit::from("--property"),
        CommandBit::from(property),
    ];
    bits.extend(services.into_iter().map(CommandBit::quoted));
    let command = StringCommand::new(bits);
    if input.user_mode {
        StringCommand::new([CommandBit::from("("), command.into(), CommandBit::from("2>/dev/null || true )")])
    } else {
        command
    }
}

fn parse_states(output: &[String], property: &str, active_values: &[&str]) -> UnitStates {
    let mut states = UnitStates::new();
    let mut current_unit = None;
    for line in output {
        let Some((key, value)) = line.trim().split_once('=') else {
            current_unit = None;
            continue;
        };
        if key == "Id" {
            current_unit = valid_unit_name(value).then(|| value.to_owned());
        } else if key == property
            && let Some(unit) = current_unit.take()
        {
            states.insert(unit, active_values.contains(&value));
        }
    }
    states
}

fn valid_unit_name(value: &str) -> bool {
    if value.len() > 256 {
        return false;
    }
    let Some((prefix, suffix)) = value.rsplit_once('.') else {
        return false;
    };
    !prefix.is_empty()
        && UNIT_SUFFIXES.contains(&suffix)
        && prefix.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '-' | '_' | '.' | '\\' | '@')
        })
}

#[cfg(test)]
mod tests {
    use super::{Fact, SystemdEnabled, SystemdInput, SystemdStatus};

    #[test]
    fn status_fact_matches_reference_command_and_state_parsing() {
        let input = SystemdInput::for_service("ssh.service");
        let output = [
            "Id=ssh.service".to_owned(),
            "SubState=running".to_owned(),
            String::new(),
            "Id=apt-daily.timer".to_owned(),
            "SubState=dead".to_owned(),
            "Id=invalid/unit.service".to_owned(),
            "SubState=running".to_owned(),
        ];

        assert_eq!(
            SystemdStatus.command(&input).raw_value(),
            "systemctl show --all --property Id --property SubState ssh.service"
        );
        let states = SystemdStatus.process(&output, &input).unwrap();
        assert_eq!(states.get("ssh.service"), Some(&true));
        assert_eq!(states.get("apt-daily.timer"), Some(&false));
        assert!(!states.contains_key("invalid/unit.service"));
    }

    #[test]
    fn enabled_fact_supports_user_machine_scope_and_static_units() {
        let input = SystemdInput {
            user_mode: true,
            machine: Some("container one".to_owned()),
            user_name: Some("app user".to_owned()),
            services: vec!["worker.service".to_owned()],
        };
        assert_eq!(
            SystemdEnabled.command(&input).raw_value(),
            "( systemctl --user '--machine=app user@container one' show --all --property Id --property UnitFileState worker.service 2>/dev/null || true )"
        );
        let states = SystemdEnabled
            .process(&["Id=worker.service".to_owned(), "UnitFileState=static".to_owned()], &input)
            .unwrap();
        assert_eq!(states.get("worker.service"), Some(&true));
    }
}
