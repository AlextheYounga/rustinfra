use indexmap::IndexMap;
use std::error::Error;
use std::slice;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::vzctl::OpenvzContainers;
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{Operation, StatefulOperation, StatefulOperationFuture};

fn commands(action: &str, ctid: &str, extra: impl IntoIterator<Item = CommandBit>) -> Vec<Command> {
    let mut bits = vec![CommandBit::from("vzctl"), CommandBit::from(action), CommandBit::quoted(ctid)];
    bits.extend(extra);
    vec![Command::Shell(StringCommand::new(bits))]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartInput {
    pub ctid: String,
    pub force: bool,
}
impl StartInput {
    #[must_use]
    pub fn new(ctid: impl Into<String>) -> Self {
        Self { ctid: ctid.into(), force: false }
    }
}
macro_rules! simple {
    ($name:ident,$input:ty,$operation:literal,$action:literal,$body:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Operation for $name {
            type Input = $input;
            fn name(&self) -> &'static str {
                $operation
            }
            fn is_idempotent(&self) -> bool {
                false
            }
            fn commands(&self, input: &Self::Input) -> Vec<Command> {
                $body(input, $action)
            }
        }
    };
}
simple!(Start, StartInput, "vzctl.start", "start", |input: &StartInput, action| commands(
    action,
    &input.ctid,
    input.force.then(|| CommandBit::from("--force"))
));

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CtidInput(pub String);
fn plain(input: &CtidInput, action: &str) -> Vec<Command> {
    commands(action, &input.0, [])
}
simple!(Stop, CtidInput, "vzctl.stop", "stop", plain);
simple!(Mount, CtidInput, "vzctl.mount", "mount", plain);
simple!(Unmount, CtidInput, "vzctl.unmount", "umount", plain);
simple!(Delete, CtidInput, "vzctl.delete", "delete", plain);

#[derive(Clone, Copy, Debug, Default)]
pub struct Restart;
impl Operation for Restart {
    type Input = StartInput;
    fn name(&self) -> &'static str {
        "vzctl.restart"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let mut result = commands("stop", &input.ctid, []);
        result.extend(Start.commands(input));
        result
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateInput {
    pub ctid: String,
    pub template: Option<String>,
}
impl CreateInput {
    #[must_use]
    pub fn new(ctid: impl Into<String>) -> Self {
        Self { ctid: ctid.into(), template: None }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Create;
impl StatefulOperation for Create {
    type Input = CreateInput;
    fn name(&self) -> &'static str {
        "vzctl.create"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let id = input.ctid.parse::<i64>().map_err(|_| "CTID must be numeric")?;
            let containers = FactExecutor::new(connector, options).get(&OpenvzContainers, &()).await?;
            create_commands(input, containers.contains_key(&id))
        })
    }
}

fn create_commands(input: &CreateInput, exists: bool) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    if exists {
        return Err(format!("An OpenVZ container with CTID {} already exists", input.ctid).into());
    }
    let extra = input
        .template
        .as_ref()
        .map(|value| vec![CommandBit::from("--ostemplate"), CommandBit::quoted(value.as_str())])
        .unwrap_or_default();
    Ok(commands("create", &input.ctid, extra))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingValue {
    One(String),
    Many(Vec<String>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetInput {
    pub ctid: String,
    pub save: bool,
    pub settings: IndexMap<String, SettingValue>,
}
impl SetInput {
    #[must_use]
    pub fn new(ctid: impl Into<String>) -> Self {
        Self { ctid: ctid.into(), save: true, settings: IndexMap::new() }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Set;
impl Operation for Set {
    type Input = SetInput;
    fn name(&self) -> &'static str {
        "vzctl.set"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let mut extra = Vec::new();
        if input.save {
            extra.push(CommandBit::from("--save"));
        }
        for (key, value) in &input.settings {
            let values = match value {
                SettingValue::One(value) => slice::from_ref(value),
                SettingValue::Many(values) => values,
            };
            for value in values {
                extra.extend([CommandBit::quoted(format!("--{key}")), CommandBit::quoted(value.as_str())]);
            }
        }
        commands("set", &input.ctid, extra)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some(command.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn lifecycle_commands_match_reference_fixtures() {
        let mut start = StartInput::new("123");
        assert_eq!(descriptions(&Start.commands(&start)), ["vzctl start 123"]);
        start.force = true;
        assert_eq!(descriptions(&Start.commands(&start)), ["vzctl start 123 --force"]);
        assert_eq!(descriptions(&Restart.commands(&start)), ["vzctl stop 123", "vzctl start 123 --force"]);
        let ctid = CtidInput("123".to_owned());
        assert_eq!(descriptions(&Stop.commands(&ctid)), ["vzctl stop 123"]);
        assert_eq!(descriptions(&Mount.commands(&ctid)), ["vzctl mount 123"]);
        assert_eq!(descriptions(&Unmount.commands(&ctid)), ["vzctl umount 123"]);
        assert_eq!(descriptions(&Delete.commands(&ctid)), ["vzctl delete 123"]);
        assert!(!Start.is_idempotent());
        assert!(!Restart.is_idempotent());
    }

    #[test]
    fn create_commands_match_new_and_existing_container_fixtures() {
        let mut input = CreateInput::new("123");
        input.template = Some("ubuntu-1604".to_owned());
        assert_eq!(
            descriptions(&create_commands(&input, false).unwrap()),
            ["vzctl create 123 --ostemplate ubuntu-1604"]
        );
        assert_eq!(
            create_commands(&input, true).unwrap_err().to_string(),
            "An OpenVZ container with CTID 123 already exists"
        );
        assert!(!Create.is_idempotent());
    }

    #[test]
    fn set_commands_match_scalar_list_save_and_injection_fixtures() {
        let mut input = SetInput::new("123");
        input.settings.insert("hostname".to_owned(), SettingValue::One("my-host.name".to_owned()));
        assert_eq!(descriptions(&Set.commands(&input)), ["vzctl set 123 --save --hostname my-host.name"]);

        input.save = false;
        assert_eq!(descriptions(&Set.commands(&input)), ["vzctl set 123 --hostname my-host.name"]);

        input.save = true;
        input.settings.clear();
        input
            .settings
            .insert("ipadd".to_owned(), SettingValue::Many(vec!["192.168.1.1".to_owned(), "192.168.1.2".to_owned()]));
        assert_eq!(
            descriptions(&Set.commands(&input)),
            ["vzctl set 123 --save --ipadd 192.168.1.1 --ipadd 192.168.1.2"]
        );

        input.settings.clear();
        input.settings.insert(";reboot".to_owned(), SettingValue::One("x; rm -rf /".to_owned()));
        assert_eq!(descriptions(&Set.commands(&input)), ["vzctl set 123 --save '--;reboot' 'x; rm -rf /'"]);
        assert!(!Set.is_idempotent());
    }
}
