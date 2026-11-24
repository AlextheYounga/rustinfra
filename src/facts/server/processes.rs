use std::error::Error;
use std::fmt;

use indexmap::IndexMap;

use crate::api::{CommandBit, PROCESS_ID_COLUMN_HEADER, StringCommand};
use crate::facts::Fact;

const DARWIN_MARKER: &str = "===PYINFRA-PROCESSES-COMM===";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProcessPlatform {
    #[default]
    Linux,
    Bsd,
    BusyBox,
    Darwin,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessesInput {
    pub pid: Option<u64>,
    pub platform: ProcessPlatform,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessInfo {
    pub user: String,
    pub state: String,
    pub cpu_percent: f64,
    pub mem_percent: f64,
    pub command: String,
    pub args: String,
}

#[derive(Debug)]
struct MissingDarwinMarker;
impl fmt::Display for MissingDarwinMarker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("server.Processes: missing Darwin command marker")
    }
}
impl Error for MissingDarwinMarker {}

#[derive(Clone, Copy, Debug, Default)]
pub struct Processes;
impl Fact for Processes {
    type Input = ProcessesInput;
    type Output = IndexMap<u64, ProcessInfo>;
    fn name(&self) -> &'static str {
        "server.Processes"
    }
    fn command(&self, input: &ProcessesInput) -> StringCommand {
        match (input.platform, input.pid) {
            (ProcessPlatform::Darwin, Some(pid)) => StringCommand::new([
                CommandBit::from("LANG=C ps -p"),
                CommandBit::quoted(pid),
                CommandBit::from(
                    "-o pid,user,stat,%cpu,%mem,args -ww && echo ===PYINFRA-PROCESSES-COMM=== && LANG=C ps -p",
                ),
                CommandBit::quoted(pid),
                CommandBit::from("-o pid,comm -ww"),
            ]),
            (ProcessPlatform::Darwin, None) => StringCommand::new([
                "LANG=C ps -eo pid,user,stat,%cpu,%mem,args -ww && echo ===PYINFRA-PROCESSES-COMM=== && LANG=C ps -eo pid,comm -ww",
            ]),
            (ProcessPlatform::BusyBox, Some(pid)) => StringCommand::new([
                CommandBit::from("LANG=C ps -o pid,user,vsz,stat,args | awk"),
                CommandBit::quoted(format!("NR==1 || $1=={pid}")),
            ]),
            (ProcessPlatform::BusyBox, None) => StringCommand::new(["LANG=C ps -o pid,user,vsz,stat,args"]),
            (ProcessPlatform::Linux | ProcessPlatform::Bsd, Some(pid)) => StringCommand::new([
                CommandBit::from("LANG=C ps -p"),
                CommandBit::quoted(pid),
                CommandBit::from("-o pid,user,stat,%cpu,%mem,comm,args"),
            ]),
            (ProcessPlatform::Bsd, None) => StringCommand::new(["LANG=C ps -eo pid,user,stat,%cpu,%mem,comm,args"]),
            (ProcessPlatform::Linux, None) => {
                StringCommand::new(["LANG=C ps -eo pid,user,stat,%cpu,%mem,comm,args --no-headers"])
            }
        }
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn process(&self, output: &[String], input: &ProcessesInput) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        if input.platform == ProcessPlatform::Darwin {
            parse_darwin(output)
        } else {
            Ok(parse_standard(output, input.platform))
        }
    }
}

fn fields(line: &str, count: usize) -> Vec<&str> {
    let mut values = Vec::new();
    let mut rest = line.trim();
    while values.len() + 1 < count {
        let Some(index) = rest.find(char::is_whitespace) else {
            break;
        };
        values.push(&rest[..index]);
        rest = rest[index..].trim_start();
    }
    if !rest.is_empty() {
        values.push(rest);
    }
    values
}

fn parse_standard(output: &[String], platform: ProcessPlatform) -> IndexMap<u64, ProcessInfo> {
    output
        .iter()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with(PROCESS_ID_COLUMN_HEADER) {
                return None;
            }
            if platform == ProcessPlatform::BusyBox {
                let values = fields(line, 5);
                let args = values.get(4)?.to_string();
                let command = args.split_whitespace().next()?.rsplit('/').next()?.to_owned();
                Some((
                    values.first()?.parse().ok()?,
                    ProcessInfo {
                        user: values.get(1)?.to_string(),
                        state: values.get(3)?.to_string(),
                        cpu_percent: 0.0,
                        mem_percent: 0.0,
                        command,
                        args,
                    },
                ))
            } else {
                let values = fields(line, 7);
                Some((
                    values.first()?.parse().ok()?,
                    ProcessInfo {
                        user: values.get(1)?.to_string(),
                        state: values.get(2)?.to_string(),
                        cpu_percent: values.get(3)?.parse().ok()?,
                        mem_percent: values.get(4)?.parse().ok()?,
                        command: values.get(5)?.to_string(),
                        args: values.get(6).unwrap_or(values.get(5)?).to_string(),
                    },
                ))
            }
        })
        .collect()
}

fn parse_darwin(output: &[String]) -> Result<IndexMap<u64, ProcessInfo>, Box<dyn Error + Send + Sync>> {
    let marker = output.iter().position(|line| line.trim() == DARWIN_MARKER).ok_or(MissingDarwinMarker)?;
    let mut processes = output[..marker]
        .iter()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with(PROCESS_ID_COLUMN_HEADER) {
                return None;
            }
            let values = fields(line, 6);
            Some((
                values.first()?.parse().ok()?,
                ProcessInfo {
                    user: values.get(1)?.to_string(),
                    state: values.get(2)?.to_string(),
                    cpu_percent: values.get(3)?.parse().ok()?,
                    mem_percent: values.get(4)?.parse().ok()?,
                    command: String::new(),
                    args: values.get(5)?.to_string(),
                },
            ))
        })
        .collect::<IndexMap<u64, ProcessInfo>>();
    for line in &output[marker + 1..] {
        let values = fields(line, 2);
        let Some(pid) = values.first().and_then(|pid| pid.parse::<u64>().ok()) else {
            continue;
        };
        if let (Some(process), Some(command)) = (processes.get_mut(&pid), values.get(1)) {
            process.command = (*command).to_owned();
        }
    }
    processes.retain(|_, process| !process.command.is_empty());
    Ok(processes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_busybox_and_darwin_processes_preserve_arguments() {
        assert_eq!(
            Processes.command(&ProcessesInput::default()).raw_value(),
            "LANG=C ps -eo pid,user,stat,%cpu,%mem,comm,args --no-headers"
        );
        assert_eq!(
            Processes.command(&ProcessesInput { pid: Some(1), platform: ProcessPlatform::Linux }).raw_value(),
            "LANG=C ps -p 1 -o pid,user,stat,%cpu,%mem,comm,args"
        );
        let linux = Processes
            .process(&["1 root Ss 0.0 0.1 init /sbin/init --system".to_owned()], &ProcessesInput::default())
            .unwrap();
        assert_eq!(linux[&1].args, "/sbin/init --system");
        let bsd_input = ProcessesInput { pid: None, platform: ProcessPlatform::Bsd };
        assert_eq!(Processes.command(&bsd_input).raw_value(), "LANG=C ps -eo pid,user,stat,%cpu,%mem,comm,args");
        let bsd = Processes
            .process(
                &["PID USER STAT %CPU %MEM COMMAND COMMAND".to_owned(), "1 root Ss 0.0 0.1 init /sbin/init".to_owned()],
                &bsd_input,
            )
            .unwrap();
        assert_eq!(bsd[&1].command, "init");
        assert_eq!(
            Processes.command(&ProcessesInput { pid: Some(1), platform: ProcessPlatform::Bsd }).raw_value(),
            "LANG=C ps -p 1 -o pid,user,stat,%cpu,%mem,comm,args"
        );
        let busy = Processes
            .process(
                &["1 root 2108 S /sbin/init".to_owned()],
                &ProcessesInput { pid: None, platform: ProcessPlatform::BusyBox },
            )
            .unwrap();
        assert_eq!(busy[&1].command, "init");
        assert_eq!(
            Processes.command(&ProcessesInput { pid: None, platform: ProcessPlatform::BusyBox }).raw_value(),
            "LANG=C ps -o pid,user,vsz,stat,args"
        );
        assert_eq!(
            Processes.command(&ProcessesInput { pid: Some(1), platform: ProcessPlatform::BusyBox }).raw_value(),
            "LANG=C ps -o pid,user,vsz,stat,args | awk 'NR==1 || $1==1'"
        );
        assert_eq!(
            Processes.command(&ProcessesInput { pid: None, platform: ProcessPlatform::Darwin }).raw_value(),
            "LANG=C ps -eo pid,user,stat,%cpu,%mem,args -ww && echo ===PYINFRA-PROCESSES-COMM=== && LANG=C ps -eo pid,comm -ww"
        );
        let darwin = Processes
            .process(
                &["1 root Ss 0.0 0.1 /sbin/launchd".to_owned(), DARWIN_MARKER.to_owned(), "1 /sbin/launchd".to_owned()],
                &ProcessesInput { pid: None, platform: ProcessPlatform::Darwin },
            )
            .unwrap();
        assert_eq!(darwin[&1].command, "/sbin/launchd");
        let spaced = Processes
            .process(
                &[
                    "3 root S 0.0 0.0 /Applications/My App --flag\r".to_owned(),
                    format!("{DARWIN_MARKER}\r"),
                    "3 /Applications/My App\r".to_owned(),
                ],
                &ProcessesInput { pid: Some(3), platform: ProcessPlatform::Darwin },
            )
            .unwrap();
        assert_eq!(spaced[&3].command, "/Applications/My App");
        assert_eq!(spaced[&3].args, "/Applications/My App --flag");
        let vanished = Processes
            .process(
                &[
                    "1 root Ss 0.0 0.1 /sbin/launchd".to_owned(),
                    "2 root S 0.0 0.0 /gone".to_owned(),
                    DARWIN_MARKER.to_owned(),
                    "1 /sbin/launchd".to_owned(),
                ],
                &ProcessesInput { pid: None, platform: ProcessPlatform::Darwin },
            )
            .unwrap();
        assert!(!vanished.contains_key(&2));
    }
}
