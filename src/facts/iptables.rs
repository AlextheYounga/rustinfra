use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpVersion {
    V4,
    V6,
}

#[cfg(test)]
mod ipv6_tests {
    use super::{Fact, Ip6tablesChains, Ip6tablesRules};
    #[test]
    fn ipv6_facts_fix_the_command_family() {
        let table = "filter".to_owned();
        assert_eq!(Ip6tablesRules.command(&table).raw_value(), "ip6tables-save -t filter");
        assert_eq!(
            Ip6tablesRules.process(&["-A INPUT -j ACCEPT".to_owned()], &table).unwrap()[0].chain.as_deref(),
            Some("INPUT")
        );
        assert_eq!(Ip6tablesChains.process(&[":INPUT ACCEPT [0:0]".to_owned()], &table).unwrap()["INPUT"], "ACCEPT");
        assert_eq!(Ip6tablesChains.command(&"nat".to_owned()).raw_value(), "ip6tables-save -t nat");
    }
}

impl IpVersion {
    fn save_command(self) -> &'static str {
        match self {
            Self::V4 => "iptables-save",
            Self::V6 => "ip6tables-save",
        }
    }

    pub(crate) fn command(self) -> &'static str {
        match self {
            Self::V4 => "iptables",
            Self::V6 => "ip6tables",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IptablesInput {
    pub table: String,
    pub version: IpVersion,
}

impl Default for IptablesInput {
    fn default() -> Self {
        Self { table: "filter".to_owned(), version: IpVersion::V4 }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuleDefinition {
    pub chain: Option<String>,
    pub jump: Option<String>,
    pub protocol: Option<String>,
    pub source: Option<String>,
    pub destination: Option<String>,
    pub in_interface: Option<String>,
    pub out_interface: Option<String>,
    pub not_protocol: Option<String>,
    pub not_source: Option<String>,
    pub not_destination: Option<String>,
    pub not_in_interface: Option<String>,
    pub not_out_interface: Option<String>,
    pub log_prefix: Option<String>,
    pub to_destination: Option<String>,
    pub to_source: Option<String>,
    pub to_ports: Option<String>,
    pub extras: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IptablesRules;

impl Fact for IptablesRules {
    type Input = IptablesInput;
    type Output = Vec<RuleDefinition>;

    fn name(&self) -> &'static str {
        "iptables.IptablesRules"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        save_command(input)
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn requires_command(&self, input: &Self::Input) -> Option<&str> {
        Some(input.version.save_command())
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().filter(|line| line.starts_with('-')).filter_map(|line| parse_rule(line)).collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IptablesChains;

impl Fact for IptablesChains {
    type Input = IptablesInput;
    type Output = IndexMap<String, String>;

    fn name(&self) -> &'static str {
        "iptables.IptablesChains"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        save_command(input)
    }

    fn default(&self) -> Self::Output {
        IndexMap::new()
    }

    fn requires_command(&self, input: &Self::Input) -> Option<&str> {
        Some(input.version.save_command())
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let chains = output
            .iter()
            .filter_map(|line| {
                let fields = line.strip_prefix(':')?.split_whitespace().collect::<Vec<_>>();
                (fields.len() >= 2).then(|| (fields[0].to_owned(), fields[1].to_owned()))
            })
            .collect();
        Ok(chains)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ip6tablesRules;
impl Fact for Ip6tablesRules {
    type Input = String;
    type Output = Vec<RuleDefinition>;
    fn name(&self) -> &'static str {
        "iptables.Ip6tablesRules"
    }
    fn command(&self, table: &String) -> StringCommand {
        IptablesRules.command(&IptablesInput { table: table.clone(), version: IpVersion::V6 })
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("ip6tables-save")
    }
    fn process(&self, output: &[String], table: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        IptablesRules.process(output, &IptablesInput { table: table.clone(), version: IpVersion::V6 })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ip6tablesChains;
impl Fact for Ip6tablesChains {
    type Input = String;
    type Output = IndexMap<String, String>;
    fn name(&self) -> &'static str {
        "iptables.Ip6tablesChains"
    }
    fn command(&self, table: &String) -> StringCommand {
        IptablesChains.command(&IptablesInput { table: table.clone(), version: IpVersion::V6 })
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("ip6tables-save")
    }
    fn process(&self, output: &[String], table: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        IptablesChains.process(output, &IptablesInput { table: table.clone(), version: IpVersion::V6 })
    }
}

pub fn parse_extras(tokens: &[String]) -> Vec<(String, String)> {
    let mut extras = Vec::new();
    let mut current_flag = None;
    let mut current_values = Vec::new();
    for token in tokens {
        if token.starts_with('-') {
            if let Some(flag) = current_flag.take() {
                extras.push((flag, current_values.join(" ")));
                current_values.clear();
            }
            current_flag = Some(token.clone());
        } else {
            current_values.push(token.clone());
        }
    }
    if let Some(flag) = current_flag {
        extras.push((flag, current_values.join(" ")));
    }
    extras.sort();
    extras
}

fn save_command(input: &IptablesInput) -> StringCommand {
    StringCommand::new([
        CommandBit::from(input.version.save_command()),
        CommandBit::from("-t"),
        CommandBit::quoted(input.table.clone()),
    ])
}

fn parse_rule(line: &str) -> Option<RuleDefinition> {
    let tokens = shlex::split(line)?;
    let mut definition = RuleDefinition::default();
    let mut extras = Vec::new();
    let mut index = 0;
    let mut negated = false;
    while index < tokens.len() {
        if tokens[index] == "!" {
            negated = true;
            index += 1;
            continue;
        }
        let flag = &tokens[index];
        if !flag.starts_with('-') {
            index += 1;
            continue;
        }
        let value = tokens.get(index + 1).filter(|value| !value.starts_with('-')).cloned().unwrap_or_default();
        if !value.is_empty() {
            index += 1;
        }
        if !set_known_field(&mut definition, flag, &value, negated) {
            extras.push(flag.clone());
            if !value.is_empty() {
                extras.push(value);
            }
        }
        negated = false;
        index += 1;
    }
    definition.extras = parse_extras(&extras);
    Some(definition)
}

fn set_known_field(definition: &mut RuleDefinition, flag: &str, value: &str, negated: bool) -> bool {
    let target = match (flag, negated) {
        ("-A", _) => &mut definition.chain,
        ("-j", _) => &mut definition.jump,
        ("-p", false) => &mut definition.protocol,
        ("-s", false) => &mut definition.source,
        ("-d", false) => &mut definition.destination,
        ("-i", false) => &mut definition.in_interface,
        ("-o", false) => &mut definition.out_interface,
        ("-p", true) => &mut definition.not_protocol,
        ("-s", true) => &mut definition.not_source,
        ("-d", true) => &mut definition.not_destination,
        ("-i", true) => &mut definition.not_in_interface,
        ("-o", true) => &mut definition.not_out_interface,
        ("--log-prefix", _) => &mut definition.log_prefix,
        ("--to-destination", _) => &mut definition.to_destination,
        ("--to-source", _) => &mut definition.to_source,
        ("--to-ports", _) => &mut definition.to_ports,
        _ => return false,
    };
    *target = Some(value.to_owned());
    true
}

#[cfg(test)]
mod tests {
    use super::{Fact, IpVersion, IptablesChains, IptablesInput, IptablesRules};

    #[test]
    fn parses_negation_quotes_nat_and_extension_arguments() {
        let rules = IptablesRules
            .process(
                &["-A INPUT -p tcp ! -s 10.0.0.0/8 -m comment --comment 'allow web app' --dport 443 -j ACCEPT"
                    .to_owned()],
                &IptablesInput::default(),
            )
            .unwrap();
        let rule = &rules[0];
        assert_eq!(rule.chain.as_deref(), Some("INPUT"));
        assert_eq!(rule.not_source.as_deref(), Some("10.0.0.0/8"));
        assert!(rule.extras.contains(&("--comment".to_owned(), "allow web app".to_owned())));
        assert!(rule.extras.contains(&("--dport".to_owned(), "443".to_owned())));
        assert_eq!(
            IptablesRules.command(&IptablesInput { table: "nat table".to_owned(), version: IpVersion::V6 }).raw_value(),
            "ip6tables-save -t 'nat table'"
        );
        let chains = IptablesChains
            .process(&[":INPUT ACCEPT [1:2]".to_owned(), ":FORWARD DROP [0:0]".to_owned()], &IptablesInput::default())
            .unwrap();
        assert_eq!(chains["INPUT"], "ACCEPT");
        assert_eq!(chains["FORWARD"], "DROP");
        assert_eq!(
            IptablesChains
                .command(&IptablesInput { table: "filter; reboot".to_owned(), version: IpVersion::V4 })
                .raw_value(),
            "iptables-save -t 'filter; reboot'"
        );
    }
}
