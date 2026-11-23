use std::path::PathBuf;

use crate::api::{Command, RsyncCommand};
use crate::operations::Operation;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RsyncInput {
    pub source: PathBuf,
    pub destination: String,
    pub flags: Vec<String>,
}

impl RsyncInput {
    #[must_use]
    pub fn new(source: impl Into<PathBuf>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            flags: vec!["-ax".to_owned(), "--delete".to_owned()],
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rsync;

impl Operation for Rsync {
    type Input = RsyncInput;
    fn name(&self) -> &'static str {
        "files.rsync"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![Command::Rsync(RsyncCommand {
            source: input.source.clone(),
            destination: input.destination.clone(),
            flags: input.flags.clone(),
        })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_reference_default_flags_and_typed_command() {
        let commands = Rsync.commands(&RsyncInput::new("local dir", "/remote dir"));
        assert!(matches!(&commands[0], Command::Rsync(value) if value.flags == ["-ax", "--delete"]));
    }
}
