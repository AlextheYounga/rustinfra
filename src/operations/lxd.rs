use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::lxd::LxdContainers;
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerInput {
    pub id: String,
    pub present: bool,
    pub image: String,
}
impl ContainerInput {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), present: true, image: "ubuntu:16.04".to_owned() }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Container;
impl StatefulOperation for Container {
    type Input = ContainerInput;
    fn name(&self) -> &'static str {
        "lxd.container"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let containers = FactExecutor::new(connector, options).get(&LxdContainers, &()).await?;
            let current = containers
                .iter()
                .find(|value| value.get("name").and_then(serde_json::Value::as_str) == Some(&input.id));
            let mut result = Vec::new();
            if !input.present {
                if let Some(value) = current {
                    if value.get("status").and_then(serde_json::Value::as_str) == Some("Running") {
                        result.push(shell([raw("lxc"), raw("stop"), quoted(&input.id)]));
                    }
                    result.push(shell([raw("lxc"), raw("delete"), quoted(&input.id)]));
                }
            } else if current.is_none() {
                result.push(shell([
                    raw("lxc"),
                    raw("launch"),
                    quoted(&input.image),
                    quoted(&input.id),
                    raw("< /dev/null"),
                ]));
            }
            Ok(result)
        })
    }
}
fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: impl Into<String>) -> CommandBit {
    CommandBit::quoted(value.into())
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_defaults_match_reference() {
        let input = ContainerInput::new("my container");
        assert_eq!(input.image, "ubuntu:16.04");
        assert!(input.present);
    }
}
