use crate::api::{Command, CommandBit, StringCommand};

use super::Operation;

mod stateful;
pub use stateful::{Group, GroupInput, Hostname, HostnameFile, HostnameInput};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KillInput {
    pub pid: i64,
    pub signal: String,
}

impl KillInput {
    #[must_use]
    pub fn new(pid: i64) -> Self {
        Self { pid, signal: "TERM".to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Kill;

impl Operation for Kill {
    type Input = KillInput;

    fn name(&self) -> &'static str {
        "server.kill"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let signal = format!("-{}", input.signal);
        vec![Command::Shell(StringCommand::new(vec![
            CommandBit::from("kill"),
            CommandBit::quoted(signal),
            CommandBit::quoted(input.pid.to_string()),
        ]))]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Wait;

impl Operation for Wait {
    type Input = u16;

    fn name(&self) -> &'static str {
        "server.wait"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, port: &Self::Input) -> Vec<Command> {
        vec![Command::Shell(StringCommand::new([format!(
            r"while ! (netstat -an | grep LISTEN | grep -e \.{port} -e :{port}); do echo 'waiting for port {port}...'; sleep 1; done"
        )]))]
    }
}

#[cfg(test)]
mod tests {
    use super::{Kill, KillInput, Shell, ShellCommands, Wait};
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

    #[test]
    fn kill_quotes_signal_and_pid_like_reference_fixtures() {
        let normal = Kill.commands(&KillInput::new(1234));
        let injected = Kill.commands(&KillInput { pid: 1234, signal: "9; rm -rf /".to_owned() });

        assert!(matches!(&normal[0], Command::Shell(command) if command.raw_value() == "kill -TERM 1234"));
        assert!(matches!(&injected[0], Command::Shell(command) if command.raw_value() == "kill '-9; rm -rf /' 1234"));
        assert!(!Kill.is_idempotent());
    }

    #[test]
    fn wait_generates_port_poll_without_string_injection() {
        let commands = Wait.commands(&443);

        assert!(matches!(&commands[0], Command::Shell(command) if command.raw_value().contains("-e :443")));
        assert!(!Wait.is_idempotent());
    }
}
