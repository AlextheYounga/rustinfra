use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::server::{Sysctl as SysctlFact, SysctlInput as SysctlFactInput, SysctlValue};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::{Line, LineInput};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SysctlInput {
    pub key: String,
    pub value: SysctlValue,
    pub persist: bool,
    pub persist_file: String,
}

impl SysctlInput {
    #[must_use]
    pub fn new(key: impl Into<String>, value: impl Into<SysctlValue>) -> Self {
        Self { key: key.into(), value: value.into(), persist: false, persist_file: "/etc/sysctl.conf".to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sysctl;

impl StatefulOperation for Sysctl {
    type Input = SysctlInput;

    fn name(&self) -> &'static str {
        "server.sysctl"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let values = FactExecutor::new(connector, fact_options)
                .get(&SysctlFact, &SysctlFactInput { keys: Some(vec![input.key.clone()]) })
                .await?;
            let value = input.value.command_value();
            let mut commands = if values.get(&input.key) == Some(&input.value) {
                Vec::new()
            } else {
                vec![sysctl_command(&input.key, &value)]
            };
            if input.persist {
                let mut line =
                    LineInput::new(&input.persist_file, format!("{}[[:space:]]*=[[:space:]]*{value}", input.key));
                line.replace = Some(format!("{} = {value}", input.key));
                commands.extend(Line.commands(&line, connector, fact_options).await?);
            }
            Ok(commands)
        })
    }
}

fn sysctl_command(key: &str, value: &str) -> Command {
    let assignment =
        StringCommand::with_separator([CommandBit::quoted(key), CommandBit::from("="), CommandBit::quoted(value)], "");
    Command::Shell(StringCommand::new([CommandBit::from("sysctl"), assignment.into()]))
}

#[cfg(test)]
mod tests {
    use super::sysctl_command;
    use crate::api::Command;

    #[test]
    fn sysctl_assignment_preserves_single_and_list_values_as_one_argument() {
        assert!(
            matches!(sysctl_command("kern.max-files", "2000"), Command::Shell(command) if command.raw_value() == "sysctl kern.max-files=2000")
        );
        assert!(
            matches!(sysctl_command("net.range", "2000 3000"), Command::Shell(command) if command.raw_value() == "sysctl net.range='2000 3000'")
        );
    }
}
