use indexmap::IndexMap;
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
            if FactExecutor::new(connector, options).get(&OpenvzContainers, &()).await?.contains_key(&id) {
                return Err(format!("An OpenVZ container with CTID {} already exists", input.ctid).into());
            }
            let extra = input
                .template
                .as_ref()
                .map(|value| vec![CommandBit::from("--ostemplate"), CommandBit::quoted(value.as_str())])
                .unwrap_or_default();
            Ok(commands("create", &input.ctid, extra))
        })
    }
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
    #[test]
    fn commands_quote_ctids_templates_and_settings() {
        let mut input = SetInput::new("100; reboot");
        input.settings.insert("hostname".to_owned(), SettingValue::One("my host".to_owned()));
        assert!(
            matches!(&Set.commands(&input)[0],Command::Shell(value)if value.raw_value()=="vzctl set '100; reboot' --save --hostname 'my host'")
        );
        let mut start = StartInput::new("100");
        start.force = true;
        assert!(
            matches!(&Restart.commands(&start)[1],Command::Shell(value)if value.raw_value()=="vzctl start 100 --force")
        );
    }
}
