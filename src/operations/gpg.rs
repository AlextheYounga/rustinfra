use std::error::Error;
use std::path::Path;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::FactOptions;

use super::files::{Directory, DirectoryInput, File, FileInput};
use super::{StatefulOperation, StatefulOperationFuture};

mod key;
pub use key::{Key, KeyInput};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DearmorInput {
    pub source: String,
    pub destination: String,
    pub mode: u16,
}

impl DearmorInput {
    pub fn new(source: impl Into<String>, destination: impl Into<String>) -> Self {
        Self { source: source.into(), destination: destination.into(), mode: 644 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Dearmor;

impl StatefulOperation for Dearmor {
    type Input = DearmorInput;
    fn name(&self) -> &'static str {
        "gpg.dearmor"
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
            let mut commands = ensure_directory(parent(&input.destination), 755, connector, options).await?;
            commands.push(shell([
                raw("gpg"),
                raw("--batch"),
                raw("--dearmor"),
                raw("-o"),
                double_quoted(&input.destination),
                double_quoted(&input.source),
            ]));
            commands.extend(ensure_file(&input.destination, input.mode, connector, options).await?);
            Ok(commands)
        })
    }
}

pub(super) async fn ensure_directory(
    path: &str,
    mode: u16,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let mut input = DirectoryInput::new(path);
    input.mode = Some(mode);
    Directory.commands(&input, connector, options).await
}

pub(super) async fn ensure_file(
    path: &str,
    mode: u16,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let mut input = FileInput::new(path);
    input.mode = Some(mode);
    File.commands(&input, connector, options).await
}

pub(super) fn parent(path: &str) -> &str {
    Path::new(path).parent().and_then(Path::to_str).filter(|value| !value.is_empty()).unwrap_or(".")
}

pub(super) fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
pub(super) fn quoted(value: impl Into<String>) -> CommandBit {
    CommandBit::quoted(value.into())
}
pub(super) fn double_quoted(value: &str) -> CommandBit {
    raw(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace(char::from(96), "\\\u{60}")
    ))
}
pub(super) fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_uses_posix_destination_directory() {
        assert_eq!(parent("/etc/apt/keyrings/key.gpg"), "/etc/apt/keyrings");
        assert_eq!(parent("key.gpg"), ".");
    }
}
