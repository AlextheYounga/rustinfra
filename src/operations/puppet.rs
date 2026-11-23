use crate::api::{Command, CommandBit, StringCommand};

use super::Operation;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentInput {
    pub server: Option<String>,
    pub port: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Agent;

impl Operation for Agent {
    type Input = AgentInput;

    fn name(&self) -> &'static str {
        "puppet.agent"
    }

    fn is_idempotent(&self) -> bool {
        false
    }

    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let mut bits = vec![CommandBit::from("puppet agent -t")];
        if let Some(server) = &input.server {
            bits.push(CommandBit::quoted(format!("--server={server}")));
        }
        if let Some(port) = input.port {
            bits.push(CommandBit::quoted(format!("--masterport={port}")));
        }
        vec![Command::Shell(StringCommand::new(bits))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_match_reference_and_quote_injection_values() {
        let input = AgentInput { server: Some("x; reboot".to_owned()), port: Some(3006) };
        let Command::Shell(command) = &Agent.commands(&input)[0] else {
            return;
        };
        assert_eq!(command.raw_value(), "puppet agent -t '--server=x; reboot' --masterport=3006");
    }
}
