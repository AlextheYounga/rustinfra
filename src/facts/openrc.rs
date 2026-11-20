use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

pub type OpenrcServiceMap = IndexMap<String, bool>;

#[derive(Clone, Copy, Debug, Default)]
pub struct OpenrcStatus;

impl Fact for OpenrcStatus {
    type Input = String;
    type Output = OpenrcServiceMap;
    fn name(&self) -> &'static str {
        "openrc.OpenrcStatus"
    }
    fn command(&self, runlevel: &Self::Input) -> StringCommand {
        StringCommand::new([CommandBit::from("rc-status"), CommandBit::quoted(runlevel.as_str())])
    }
    fn default(&self) -> Self::Output {
        OpenrcServiceMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("rc-status")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let (name, status) = line.split_once('[')?;
                let name = name.trim();
                if name.is_empty() {
                    return None;
                }
                Some((name.to_owned(), status.trim_start().starts_with("started")))
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OpenrcEnabled;

impl Fact for OpenrcEnabled {
    type Input = String;
    type Output = OpenrcServiceMap;
    fn name(&self) -> &'static str {
        "openrc.OpenrcEnabled"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["rc-update show -v"])
    }
    fn default(&self) -> Self::Output {
        OpenrcServiceMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("rc-update")
    }
    fn process(&self, output: &[String], runlevel: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let (name, levels) = line.split_once('|')?;
                Some((name.trim().to_owned(), levels.split_whitespace().any(|level| level == runlevel)))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, OpenrcEnabled, OpenrcStatus};

    #[test]
    fn parses_statuses_and_selected_runlevel() {
        let runlevel = "boot".to_owned();
        let status = OpenrcStatus
            .process(&["  podman [  started 00:34:14 (10) ]".to_owned(), "  guest [  crashed  ]".to_owned()], &runlevel)
            .unwrap();
        assert!(status["podman"]);
        assert!(!status["guest"]);
        let enabled =
            OpenrcEnabled.process(&[" bootmisc | boot".to_owned(), " crond | default".to_owned()], &runlevel).unwrap();
        assert!(enabled["bootmisc"]);
        assert!(!enabled["crond"]);
    }
}
