use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};
use crate::facts::Fact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SysctlValue {
    Integer(i64),
    String(String),
    List(Vec<SysctlValue>),
}

impl SysctlValue {
    #[must_use]
    pub fn command_value(&self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::String(value) => value.clone(),
            Self::List(values) => values.iter().map(Self::command_value).collect::<Vec<_>>().join(" "),
        }
    }
}

impl From<i64> for SysctlValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<String> for SysctlValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for SysctlValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SysctlInput {
    pub keys: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sysctl;

impl Fact for Sysctl {
    type Input = SysctlInput;
    type Output = IndexMap<String, SysctlValue>;

    fn name(&self) -> &'static str {
        "server.Sysctl"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        match &input.keys {
            None => StringCommand::new(["sysctl -a 2>/dev/null || true"]),
            Some(keys) => {
                let mut bits = vec![CommandBit::from("sysctl")];
                bits.extend(keys.iter().cloned().map(CommandBit::quoted));
                bits.push(CommandBit::from("2>/dev/null || true"));
                StringCommand::new(bits)
            }
        }
    }

    fn default(&self) -> Self::Output {
        IndexMap::new()
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut values = IndexMap::new();
        for line in output {
            let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':')) else {
                continue;
            };
            values.insert(key.trim().to_owned(), parse_value(value.trim()));
        }
        Ok(values)
    }
}

fn parse_value(value: &str) -> SysctlValue {
    let simple = value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ' ' | '\t'));
    if !simple || value.is_empty() {
        return SysctlValue::String(value.to_owned());
    }
    let values = value.split_whitespace().map(parse_scalar).collect::<Vec<_>>();
    match values.as_slice() {
        [] => SysctlValue::String(String::new()),
        [value] => value.clone(),
        _ => SysctlValue::List(values),
    }
}

fn parse_scalar(value: &str) -> SysctlValue {
    value.parse().map_or_else(|_| SysctlValue::String(value.to_owned()), SysctlValue::Integer)
}

#[cfg(test)]
mod tests {
    use super::{Fact, Sysctl, SysctlInput, SysctlValue};

    #[test]
    fn parses_linux_and_bsd_values_with_typed_numbers_and_lists() {
        let values = Sysctl
            .process(
                &[
                    "net.ipv4.tcp_adv_win_scale = -2".to_owned(),
                    "net.ipv4.ip_local_port_range = 1024\t65535".to_owned(),
                    "net.ipv4.ip_local_reserved_ports = ".to_owned(),
                    "kern.hostname: example".to_owned(),
                ],
                &SysctlInput::default(),
            )
            .unwrap();

        assert_eq!(values["net.ipv4.tcp_adv_win_scale"], SysctlValue::Integer(-2));
        assert_eq!(
            values["net.ipv4.ip_local_port_range"],
            SysctlValue::List(vec![SysctlValue::Integer(1024), SysctlValue::Integer(65535)])
        );
        assert_eq!(values["net.ipv4.ip_local_reserved_ports"], SysctlValue::String(String::new()));
        assert_eq!(values["kern.hostname"], SysctlValue::String("example".to_owned()));
    }
}
