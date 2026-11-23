use std::error::Error;

use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::iptables::{IpVersion, IptablesChains, IptablesInput, IptablesRules, RuleDefinition, parse_extras};
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainInput {
    pub chain: String,
    pub present: bool,
    pub table: String,
    pub policy: Option<String>,
    pub version: IpVersion,
}

impl ChainInput {
    #[must_use]
    pub fn new(chain: impl Into<String>) -> Self {
        Self { chain: chain.into(), present: true, table: "filter".to_owned(), policy: None, version: IpVersion::V4 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Chain;

impl StatefulOperation for Chain {
    type Input = ChainInput;

    fn name(&self) -> &'static str {
        "iptables.chain"
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
            let fact_input = IptablesInput { table: input.table.clone(), version: input.version };
            let chains = FactExecutor::new(connector, fact_options).get(&IptablesChains, &fact_input).await?;
            Ok(chain_commands(input, chains.get(&input.chain).map(String::as_str)))
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleInput {
    pub chain: String,
    pub jump: String,
    pub present: bool,
    pub table: String,
    pub append: bool,
    pub version: IpVersion,
    pub protocol: Option<String>,
    pub not_protocol: Option<String>,
    pub source: Option<String>,
    pub not_source: Option<String>,
    pub destination: Option<String>,
    pub not_destination: Option<String>,
    pub in_interface: Option<String>,
    pub not_in_interface: Option<String>,
    pub out_interface: Option<String>,
    pub not_out_interface: Option<String>,
    pub to_destination: Option<String>,
    pub to_source: Option<String>,
    pub to_ports: Option<String>,
    pub log_prefix: Option<String>,
    pub destination_port: Option<String>,
    pub source_port: Option<String>,
    pub comment: Option<String>,
    pub extras: Vec<String>,
}

impl RuleInput {
    #[must_use]
    pub fn new(chain: impl Into<String>, jump: impl Into<String>) -> Self {
        Self {
            chain: chain.into(),
            jump: jump.into(),
            present: true,
            table: "filter".to_owned(),
            append: true,
            version: IpVersion::V4,
            protocol: None,
            not_protocol: None,
            source: None,
            not_source: None,
            destination: None,
            not_destination: None,
            in_interface: None,
            not_in_interface: None,
            out_interface: None,
            not_out_interface: None,
            to_destination: None,
            to_source: None,
            to_ports: None,
            log_prefix: None,
            destination_port: None,
            source_port: None,
            comment: None,
            extras: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rule;

impl StatefulOperation for Rule {
    type Input = RuleInput;

    fn name(&self) -> &'static str {
        "iptables.rule"
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
            let prepared = PreparedRule::new(input).map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)?;
            let fact_input = IptablesInput { table: input.table.clone(), version: input.version };
            let rules = FactExecutor::new(connector, fact_options).get(&IptablesRules, &fact_input).await?;
            Ok(rule_commands(input, &prepared, rules.contains(&prepared.definition)))
        })
    }
}

fn chain_commands(input: &ChainInput, existing_policy: Option<&str>) -> Vec<Command> {
    let mut commands = Vec::new();
    let base = || [raw(input.version.command()), raw("-t"), quoted(&input.table)];
    if !input.present {
        if existing_policy.is_some() {
            let mut bits = Vec::from(base());
            bits.extend([raw("-X"), quoted(&input.chain)]);
            commands.push(shell(bits));
        }
        return commands;
    }
    if existing_policy.is_none() {
        let mut bits = Vec::from(base());
        bits.extend([raw("-N"), quoted(&input.chain)]);
        commands.push(shell(bits));
    }
    if let Some(policy) = &input.policy
        && existing_policy != Some(policy)
    {
        let mut bits = Vec::from(base());
        bits.extend([raw("-P"), quoted(&input.chain), quoted(policy)]);
        commands.push(shell(bits));
    }
    commands
}

struct PreparedRule {
    definition: RuleDefinition,
    extras: Vec<String>,
}

impl PreparedRule {
    fn new(input: &RuleInput) -> Result<Self, IptablesOperationError> {
        validate_rule(input)?;
        let mut extras = input.extras.clone();
        if let Some(port) = &input.destination_port {
            extras.extend(["--dport".to_owned(), port.clone()]);
        }
        if let Some(port) = &input.source_port {
            extras.extend(["--sport".to_owned(), port.clone()]);
        }
        if let Some(comment) = &input.comment {
            extras.extend(["-m".to_owned(), "comment".to_owned(), "--comment".to_owned(), comment.clone()]);
        }
        let mut parsed_extras = parse_extras(&extras);
        if let Some(protocol) = &input.protocol {
            parsed_extras.push(("-m".to_owned(), protocol.to_ascii_lowercase()));
            parsed_extras.sort();
        }
        let definition = RuleDefinition {
            chain: Some(input.chain.clone()),
            jump: Some(input.jump.clone()),
            protocol: input.protocol.as_ref().map(|value| value.to_ascii_lowercase()),
            source: normalized_network(input.source.as_deref()),
            destination: normalized_network(input.destination.as_deref()),
            in_interface: input.in_interface.clone(),
            out_interface: input.out_interface.clone(),
            not_protocol: input.not_protocol.as_ref().map(|value| value.to_ascii_lowercase()),
            not_source: normalized_network(input.not_source.as_deref()),
            not_destination: normalized_network(input.not_destination.as_deref()),
            not_in_interface: input.not_in_interface.clone(),
            not_out_interface: input.not_out_interface.clone(),
            log_prefix: input.log_prefix.clone(),
            to_destination: input.to_destination.clone(),
            to_source: input.to_source.clone(),
            to_ports: input.to_ports.clone(),
            extras: parsed_extras,
        };
        Ok(Self { definition, extras })
    }
}

fn validate_rule(input: &RuleInput) -> Result<(), IptablesOperationError> {
    if input.protocol.is_none() && (input.destination_port.is_some() || input.source_port.is_some()) {
        return Err(IptablesOperationError::PortWithoutProtocol);
    }
    for (value, table, jump, kind) in [
        (&input.to_destination, "nat", "DNAT", "to_destination"),
        (&input.to_source, "nat", "SNAT", "to_source"),
        (&input.to_ports, "nat", "REDIRECT", "to_ports"),
    ] {
        if value.is_some() && (input.table != table || input.jump != jump) {
            return Err(IptablesOperationError::InvalidNatTarget {
                kind,
                table: input.table.clone(),
                jump: input.jump.clone(),
            });
        }
    }
    if input.log_prefix.is_some() && input.jump != "LOG" {
        return Err(IptablesOperationError::InvalidLogTarget { jump: input.jump.clone() });
    }
    Ok(())
}

fn rule_commands(input: &RuleInput, prepared: &PreparedRule, exists: bool) -> Vec<Command> {
    let action = match (input.present, exists, input.append) {
        (true, false, true) => "-A",
        (true, false, false) => "-I",
        (false, true, _) => "-D",
        _ => return Vec::new(),
    };
    let mut bits =
        vec![raw(input.version.command()), raw("-t"), quoted(&input.table), raw(action), quoted(&input.chain)];
    add_optional(&mut bits, "-p", input.protocol.as_deref().map(str::to_ascii_lowercase).as_deref());
    add_optional(&mut bits, "-s", input.source.as_deref());
    add_optional(&mut bits, "-d", input.destination.as_deref());
    add_optional(&mut bits, "-i", input.in_interface.as_deref());
    add_optional(&mut bits, "-o", input.out_interface.as_deref());
    add_negated(&mut bits, "-p", input.not_protocol.as_deref().map(str::to_ascii_lowercase).as_deref());
    add_negated(&mut bits, "-s", input.not_source.as_deref());
    add_negated(&mut bits, "-d", input.not_destination.as_deref());
    add_negated(&mut bits, "-i", input.not_in_interface.as_deref());
    add_negated(&mut bits, "-o", input.not_out_interface.as_deref());
    bits.extend(prepared.extras.iter().map(|value| quoted(value)));
    bits.extend([raw("-j"), quoted(&input.jump)]);
    add_optional(&mut bits, "--log-prefix", input.log_prefix.as_deref());
    add_optional(&mut bits, "--to-destination", input.to_destination.as_deref());
    add_optional(&mut bits, "--to-source", input.to_source.as_deref());
    add_optional(&mut bits, "--to-ports", input.to_ports.as_deref());
    vec![shell(bits)]
}

fn normalized_network(value: Option<&str>) -> Option<String> {
    value.map(|value| if value.contains('/') { value.to_owned() } else { format!("{value}/32") })
}

fn add_optional(bits: &mut Vec<CommandBit>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        bits.extend([raw(flag), quoted(value)]);
    }
}

fn add_negated(bits: &mut Vec<CommandBit>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        bits.extend([raw("!"), raw(flag), quoted(value)]);
    }
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}

fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}

fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IptablesOperationError {
    #[error("iptables cannot filter by destination_port/source_port without a protocol")]
    PortWithoutProtocol,
    #[error("iptables only supports {kind} on the nat table and the required jump (table={table}, jump={jump})")]
    InvalidNatTarget { kind: &'static str, table: String, jump: String },
    #[error("iptables only supports log_prefix with the LOG jump (jump={jump})")]
    InvalidLogTarget { jump: String },
}

#[cfg(test)]
#[path = "iptables_tests.rs"]
mod tests;
