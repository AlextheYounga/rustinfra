use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, HiddenValue, StringCommand};

use super::Fact;
use super::UNKNOWN_STATE;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SupervisorctlOptions {
    pub config_file: Option<String>,
    pub server_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<HiddenValue>,
}

pub(crate) fn command(options: &SupervisorctlOptions) -> StringCommand {
    let mut bits = vec![CommandBit::from("supervisorctl")];
    if let Some(value) = &options.config_file {
        bits.extend([CommandBit::from("--configuration"), CommandBit::quoted(value.as_str())]);
    }
    if let Some(value) = &options.server_url {
        bits.extend([CommandBit::from("--serverurl"), CommandBit::quoted(value.as_str())]);
    }
    if let Some(value) = &options.username {
        bits.extend([CommandBit::from("--username"), CommandBit::quoted(value.as_str())]);
    }
    if let Some(value) = &options.password {
        bits.extend([CommandBit::from("--password"), CommandBit::quoted(value.clone())]);
    }
    StringCommand::new(bits)
}

pub type SupervisorctlStatusMap = IndexMap<String, Option<bool>>;

#[derive(Clone, Copy, Debug, Default)]
pub struct SupervisorctlStatus;

impl Fact for SupervisorctlStatus {
    type Input = SupervisorctlOptions;
    type Output = SupervisorctlStatusMap;

    fn name(&self) -> &'static str {
        "supervisorctl.SupervisorctlStatus"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::new([
            CommandBit::from(command(input)),
            CommandBit::from("status"),
            CommandBit::from("||"),
            CommandBit::from("true"),
        ])
    }

    fn default(&self) -> Self::Output {
        SupervisorctlStatusMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("supervisorctl")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let name = fields.next()?;
                let state = fields.next()?;
                let running = match state {
                    "RUNNING" | "STARTING" => Some(Some(true)),
                    "STOPPED" | "STOPPING" | "EXITED" => Some(Some(false)),
                    "BACKOFF" | "FATAL" | UNKNOWN_STATE => Some(None),
                    _ => None,
                }?;
                Some((name.to_owned(), running))
            })
            .collect())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SupervisorctlPidInput {
    pub process: Option<String>,
    pub options: SupervisorctlOptions,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SupervisorctlPid;

impl Fact for SupervisorctlPid {
    type Input = SupervisorctlPidInput;
    type Output = Option<u32>;

    fn name(&self) -> &'static str {
        "supervisorctl.SupervisorctlPid"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        let mut bits = vec![CommandBit::from(command(&input.options)), CommandBit::from("pid")];
        if let Some(process) = &input.process {
            bits.push(CommandBit::quoted(process.as_str()));
        }
        bits.extend([CommandBit::from("||"), CommandBit::from("true")]);
        StringCommand::new(bits)
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("supervisorctl")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().find_map(|line| line.trim().parse().ok()))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SupervisorctlPids;

impl Fact for SupervisorctlPids {
    type Input = SupervisorctlOptions;
    type Output = Vec<u32>;

    fn name(&self) -> &'static str {
        "supervisorctl.SupervisorctlPids"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::new([
            CommandBit::from(command(input)),
            CommandBit::from("pid"),
            CommandBit::from("all"),
            CommandBit::from("||"),
            CommandBit::from("true"),
        ])
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("supervisorctl")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().flat_map(|line| line.split_whitespace()).filter_map(|part| part.parse().ok()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Fact, SupervisorctlOptions, SupervisorctlPid, SupervisorctlPidInput, SupervisorctlPids, SupervisorctlStatus,
    };
    use crate::api::HiddenValue;

    #[test]
    fn status_options_mask_password_and_parse_all_known_states() {
        let input = SupervisorctlOptions {
            config_file: Some("/etc/supervisord.conf".to_owned()),
            server_url: Some("http://localhost:9001".to_owned()),
            username: Some("admin".to_owned()),
            password: Some(HiddenValue::new("s3cret")),
        };
        let command = SupervisorctlStatus.command(&input);
        assert!(command.raw_value().contains("--password s3cret status || true"));
        assert!(command.masked_value().contains("--password '*MASKED*' status || true"));
        let statuses = SupervisorctlStatus
            .process(
                &["web RUNNING pid 1".to_owned(), "worker EXITED Jan 1".to_owned(), "broken FATAL".to_owned()],
                &input,
            )
            .unwrap();
        assert_eq!(statuses["web"], Some(true));
        assert_eq!(statuses["worker"], Some(false));
        assert_eq!(statuses["broken"], None);
    }

    #[test]
    fn pid_facts_preserve_zero_and_ignore_non_numeric_output() {
        let input = SupervisorctlPidInput { process: Some("web".to_owned()), options: SupervisorctlOptions::default() };
        assert_eq!(SupervisorctlPid.command(&input).raw_value(), "supervisorctl pid web || true");
        assert_eq!(SupervisorctlPid.process(&["0".to_owned()], &input).unwrap(), Some(0));
        assert_eq!(SupervisorctlPid.process(&["ERROR".to_owned()], &input).unwrap(), None);
        assert_eq!(
            SupervisorctlPids.process(&["1001 1002 0".to_owned()], &SupervisorctlOptions::default()).unwrap(),
            [1001, 1002, 0]
        );
    }
}
