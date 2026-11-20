use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

#[derive(Clone, Copy, Debug, Default)]
pub struct SEBoolean;

impl Fact for SEBoolean {
    type Input = String;
    type Output = String;

    fn name(&self) -> &'static str {
        "selinux.SEBoolean"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::new([CommandBit::from("getsebool"), CommandBit::quoted(input.as_str())])
    }
    fn default(&self) -> Self::Output {
        String::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("getsebool")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .first()
            .and_then(|line| line.split_once(" --> "))
            .map_or_else(String::new, |(_, value)| value.to_owned()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileContextInfo {
    pub user: String,
    pub role: String,
    pub se_type: String,
    pub level: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FileContext;

impl Fact for FileContext {
    type Input = String;
    type Output = Option<FileContextInfo>;

    fn name(&self) -> &'static str {
        "selinux.FileContext"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::new([
            CommandBit::from("stat -c %C"),
            CommandBit::quoted(input.as_str()),
            CommandBit::from("|| exit 0"),
        ])
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().and_then(|line| parse_context(line)))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FileContextMapping;

impl Fact for FileContextMapping {
    type Input = String;
    type Output = Option<FileContextInfo>;

    fn name(&self) -> &'static str {
        "selinux.FileContextMapping"
    }
    fn command(&self, target: &Self::Input) -> StringCommand {
        StringCommand::new([
            CommandBit::from("set -o pipefail && semanage fcontext -n -l | (grep"),
            CommandBit::quoted(format!("^{target}")),
            CommandBit::from("|| true)"),
        ])
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("semanage")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok((output.len() == 1)
            .then(|| output.first())
            .flatten()
            .and_then(|line| line.split_whitespace().rev().find_map(parse_context)))
    }
}

fn parse_context(value: &str) -> Option<FileContextInfo> {
    let mut fields = value.split(':');
    let context = FileContextInfo {
        user: fields.next()?.to_owned(),
        role: fields.next()?.to_owned(),
        se_type: fields.next()?.to_owned(),
        level: fields.next()?.to_owned(),
    };
    Some(context)
}

pub type SEPortMap = IndexMap<String, IndexMap<u16, String>>;

#[derive(Clone, Copy, Debug, Default)]
pub struct SEPorts;

impl Fact for SEPorts {
    type Input = ();
    type Output = SEPortMap;

    fn name(&self) -> &'static str {
        "selinux.SEPorts"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["semanage port -ln"])
    }
    fn default(&self) -> Self::Output {
        SEPortMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("semanage")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut ports = SEPortMap::new();
        for line in output {
            let mut fields = line.split_whitespace();
            let (Some(se_type), Some(protocol)) = (fields.next(), fields.next()) else { continue };
            if se_type == "unreserved_port_t" {
                continue;
            }
            let values = fields.collect::<Vec<_>>().join(" ");
            for item in values.split(',').map(str::trim) {
                let Some((start, end)) = parse_port_range(item) else { continue };
                for port in start..=end {
                    ports.entry(protocol.to_owned()).or_default().insert(port, se_type.to_owned());
                }
            }
        }
        Ok(ports)
    }
}

fn parse_port_range(value: &str) -> Option<(u16, u16)> {
    if let Some((start, end)) = value.split_once('-') {
        Some((start.parse().ok()?, end.parse().ok()?))
    } else {
        let port = value.parse().ok()?;
        Some((port, port))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SEPortInput {
    pub protocol: String,
    pub port: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SEPort;

impl Fact for SEPort {
    type Input = SEPortInput;
    type Output = String;

    fn name(&self) -> &'static str {
        "selinux.SEPort"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::new([
            CommandBit::from("(sepolicy network -p"),
            CommandBit::quoted(input.port.as_str()),
            CommandBit::from("2>/dev/null || true) | grep"),
            CommandBit::quoted(input.protocol.as_str()),
        ])
    }
    fn default(&self) -> Self::Output {
        String::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("sepolicy")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        if output.len() <= 1 {
            return Ok(String::new());
        }
        Ok(output.first().and_then(|line| line.split_whitespace().nth(2)).unwrap_or_default().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, FileContext, FileContextMapping, SEPort, SEPortInput, SEPorts};

    #[test]
    fn contexts_parse_structured_values_and_quote_targets() {
        let path = "/etc/hosts".to_owned();
        let context = FileContext.process(&["system_u:object_r:net_conf_t:s0".to_owned()], &path).unwrap().unwrap();
        assert_eq!(context.se_type, "net_conf_t");
        assert_eq!(FileContext.process(&["?".to_owned()], &path).unwrap(), None);
        let target = "/etc/zebra(/.*)?".to_owned();
        assert_eq!(
            FileContextMapping.command(&target).raw_value(),
            "set -o pipefail && semanage fcontext -n -l | (grep '^/etc/zebra(/.*)?' || true)"
        );
    }

    #[test]
    fn ports_expand_ranges_skip_unreserved_ranges_and_select_specific_types() {
        let ports = SEPorts
            .process(&["example_port_t tcp 93, 50-51".to_owned(), "unreserved_port_t tcp 1024-65535".to_owned()], &())
            .unwrap();
        assert_eq!(ports["tcp"][&50], "example_port_t");
        assert_eq!(ports["tcp"][&93], "example_port_t");
        let input = SEPortInput { protocol: "tcp".to_owned(), port: "22".to_owned() };
        assert_eq!(
            SEPort
                .process(&["22: tcp ssh_port_t 22".to_owned(), "22: tcp reserved_port_t 1-511".to_owned()], &input)
                .unwrap(),
            "ssh_port_t"
        );
    }
}
