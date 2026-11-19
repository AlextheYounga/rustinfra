use crate::api::{Command, StringCommand};

use super::Operation;

/// One or more raw shell commands accepted by `server.shell`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellCommands(Vec<String>);

impl ShellCommands {
    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

impl From<&str> for ShellCommands {
    fn from(value: &str) -> Self {
        Self(vec![value.to_owned()])
    }
}

impl From<String> for ShellCommands {
    fn from(value: String) -> Self {
        Self(vec![value])
    }
}

impl From<Vec<String>> for ShellCommands {
    fn from(value: Vec<String>) -> Self {
        Self(value)
    }
}

impl<const SIZE: usize> From<[&str; SIZE]> for ShellCommands {
    fn from(value: [&str; SIZE]) -> Self {
        Self(value.into_iter().map(str::to_owned).collect())
    }
}

/// Runs raw shell code on a target.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shell;

impl Operation for Shell {
    type Input = ShellCommands;

    fn name(&self) -> &'static str {
        "server.shell"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        input.as_slice().iter().cloned().map(|command| Command::Shell(StringCommand::new([command]))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Shell, ShellCommands};
    use crate::api::Command;
    use crate::operations::Operation;

    #[test]
    fn shell_accepts_one_command_or_an_ordered_list() {
        let one = Shell.commands(&ShellCommands::from("echo one"));
        let many = Shell.commands(&ShellCommands::from(["echo one", "echo two"]));

        assert_eq!(one.len(), 1);
        assert_eq!(many.len(), 2);
        assert!(matches!(&many[0], Command::Shell(command) if command.raw_value() == "echo one"));
        assert!(matches!(&many[1], Command::Shell(command) if command.raw_value() == "echo two"));
        assert!(!Shell.is_idempotent());
    }
}
