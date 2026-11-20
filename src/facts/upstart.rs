use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;

#[derive(Clone, Copy, Debug, Default)]
pub struct UpstartStatus;

impl Fact for UpstartStatus {
    type Input = ();
    type Output = IndexMap<String, bool>;
    fn name(&self) -> &'static str {
        "upstart.UpstartStatus"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["initctl list"])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("initctl")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let (name, remainder) = line.split_once(' ')?;
                let (_, status) = remainder.split_once('/')?;
                Some((name.to_owned(), status.starts_with("running")))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, UpstartStatus};

    #[test]
    fn parses_running_and_waiting_jobs() {
        let statuses = UpstartStatus
            .process(&["something start/running, process 1493".to_owned(), "shutdown stop/waiting".to_owned()], &())
            .unwrap();
        assert!(statuses["something"]);
        assert!(!statuses["shutdown"]);
    }
}
