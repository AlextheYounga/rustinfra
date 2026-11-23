use std::error::Error;

use crate::api::{CommandBit, StringCommand, USER_NAME_IDENTIFIER};
use crate::facts::Fact;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkTool {
    Ss,
    Netstat,
    Sockstat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkProtocol {
    Tcp,
    Udp,
}

impl NetworkProtocol {
    fn name(self) -> &'static str {
        if self == Self::Tcp { "tcp" } else { "udp" }
    }
    fn flag(self) -> &'static str {
        if self == Self::Tcp { "t" } else { "u" }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortInput {
    pub port: u16,
    pub protocol: NetworkProtocol,
    pub tool: NetworkTool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortProcess {
    pub process: String,
    pub pid: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Port;
impl Fact for Port {
    type Input = PortInput;
    type Output = Option<PortProcess>;
    fn name(&self) -> &'static str {
        "server.Port"
    }
    fn command(&self, input: &PortInput) -> StringCommand {
        match input.tool {
            NetworkTool::Sockstat => StringCommand::new([
                CommandBit::from("sockstat -l -p"),
                CommandBit::quoted(input.port),
                CommandBit::from("-P"),
                CommandBit::quoted(input.protocol.name()),
            ]),
            NetworkTool::Ss => StringCommand::new([
                CommandBit::from(format!("ss -lp{}n | grep", input.protocol.flag())),
                CommandBit::quoted(format!(":{} ", input.port)),
                CommandBit::from("|| true"),
            ]),
            NetworkTool::Netstat => StringCommand::new([
                CommandBit::from(format!("netstat -{}lnp 2>/dev/null | awk", input.protocol.flag())),
                CommandBit::quoted(format!("$4 ~ /:{}$/", input.port)),
            ]),
        }
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn process(&self, output: &[String], input: &PortInput) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(match input.tool {
            NetworkTool::Ss => output.iter().find_map(|line| {
                let process = line.split('"').nth(1)?.to_owned();
                let pid = line.split_once("pid=")?.1.split(',').next()?.parse().ok()?;
                Some(PortProcess { process, pid })
            }),
            NetworkTool::Netstat => output.iter().flat_map(|line| line.split_whitespace()).find_map(|field| {
                let (pid, process) = field.split_once('/')?;
                Some(PortProcess { process: process.to_owned(), pid: pid.parse().ok()? })
            }),
            NetworkTool::Sockstat => {
                output.iter().filter(|line| !line.trim_start().starts_with(USER_NAME_IDENTIFIER)).find_map(|line| {
                    let fields = line.split_whitespace().collect::<Vec<_>>();
                    Some(PortProcess { process: fields.get(1)?.to_string(), pid: fields.get(2)?.parse().ok()? })
                })
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortsInput {
    pub tool: NetworkTool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListeningPort {
    pub port: u16,
    pub ip: String,
    pub protocol: String,
    pub process: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ports;
impl Fact for Ports {
    type Input = PortsInput;
    type Output = Vec<ListeningPort>;
    fn name(&self) -> &'static str {
        "server.Ports"
    }
    fn command(&self, input: &PortsInput) -> StringCommand {
        StringCommand::new([match input.tool {
            NetworkTool::Ss => "ss -lpuntn || true",
            NetworkTool::Netstat => "netstat -tunp 2>/dev/null || true",
            NetworkTool::Sockstat => "sockstat -l",
        }])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn process(&self, output: &[String], input: &PortsInput) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(match input.tool {
            NetworkTool::Ss => output.iter().filter_map(|line| parse_ss(line)).collect(),
            NetworkTool::Netstat => output.iter().filter_map(|line| parse_netstat(line)).collect(),
            NetworkTool::Sockstat => output.iter().filter_map(|line| parse_sockstat(line)).collect(),
        })
    }
}

fn address(value: &str) -> Option<(String, u16)> {
    let (ip, port) = value.rsplit_once(':')?;
    let ip = ip.strip_prefix('[').and_then(|value| value.strip_suffix(']')).unwrap_or(ip);
    Some((ip.split('%').next()?.to_owned(), port.parse().ok()?))
}

fn parse_ss(line: &str) -> Option<ListeningPort> {
    let process = line.split('"').nth(1)?.to_owned();
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let prefixed = matches!(fields.first().copied(), Some("tcp" | "udp"));
    let protocol = if prefixed {
        fields[0]
    } else if fields.first().copied() == Some("UNCONN") {
        "udp"
    } else {
        "tcp"
    };
    let local = *fields.get(if prefixed { 4 } else { 3 })?;
    let (ip, port) = address(local)?;
    Some(ListeningPort { port, ip, protocol: protocol.to_owned(), process })
}

fn parse_netstat(line: &str) -> Option<ListeningPort> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let protocol = if fields.first()?.starts_with("tcp") {
        "tcp"
    } else if fields.first()?.starts_with("udp") {
        "udp"
    } else {
        return None;
    };
    let (_, process) = fields.iter().find_map(|field| field.split_once('/'))?;
    let (ip, port) = address(fields.get(3)?)?;
    Some(ListeningPort { port, ip, protocol: protocol.to_owned(), process: process.to_owned() })
}

fn parse_sockstat(line: &str) -> Option<ListeningPort> {
    if line.trim_start().starts_with(USER_NAME_IDENTIFIER) {
        return None;
    }
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let (ip, port) = address(fields.get(5)?)?;
    Some(ListeningPort { port, ip, protocol: fields.get(4)?.to_string(), process: fields.get(1)?.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn port_commands_and_all_parsers_match_reference_formats() {
        let input = PortInput { port: 53, protocol: NetworkProtocol::Udp, tool: NetworkTool::Ss };
        assert_eq!(Port.command(&input).raw_value(), "ss -lpun | grep ':53 ' || true");
        let process = Port
            .process(&[r#"UNCONN 0 0 0.0.0.0:53 0.0.0.0:* users:(("named",pid=1234,fd=1))"#.to_owned()], &input)
            .unwrap()
            .unwrap();
        assert_eq!(process, PortProcess { process: "named".to_owned(), pid: 1234 });
        let ports = Ports
            .process(
                &[r#"udp UNCONN 0 0 127.0.0.53%lo:53 0.0.0.0:* users:(("resolve",pid=1,fd=2))"#.to_owned()],
                &PortsInput { tool: NetworkTool::Ss },
            )
            .unwrap();
        assert_eq!(ports[0].ip, "127.0.0.53");
        assert_eq!(ports[0].protocol, "udp");
        let netstat = Ports
            .process(
                &["tcp 0 0 0.0.0.0:80 0.0.0.0:* LISTEN 5678/nginx".to_owned()],
                &PortsInput { tool: NetworkTool::Netstat },
            )
            .unwrap();
        assert_eq!(netstat[0].port, 80);
        assert_eq!(netstat[0].process, "nginx");
        let sockstat = Ports
            .process(&["root sshd 6674 6 tcp6 *:22 *:*".to_owned()], &PortsInput { tool: NetworkTool::Sockstat })
            .unwrap();
        assert_eq!(sockstat[0].ip, "*");
        assert_eq!(sockstat[0].protocol, "tcp6");
        assert_eq!(address("[::]:22"), Some(("::".to_owned(), 22)));
    }
}
