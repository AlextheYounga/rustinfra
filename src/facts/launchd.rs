use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;

#[derive(Clone, Copy, Debug, Default)]
pub struct LaunchdStatus;

impl Fact for LaunchdStatus {
    type Input = ();
    type Output = IndexMap<String, bool>;

    fn name(&self) -> &'static str {
        "launchd.LaunchdStatus"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["launchctl list"])
    }

    fn default(&self) -> Self::Output {
        IndexMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("launchctl")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let pid = fields.next()?;
                if pid == "PID" {
                    return None;
                }
                fields.next()?;
                let label = fields.next()?;
                Some((label.to_owned(), pid.parse::<u32>().is_ok()))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, LaunchdStatus};

    #[test]
    fn parses_running_and_stopped_reference_services() {
        let statuses = LaunchdStatus
            .process(
                &[
                    "PID Status Label".to_owned(),
                    "411 0 com.example.running".to_owned(),
                    "- 0 com.example.stopped".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(statuses["com.example.running"], true);
        assert_eq!(statuses["com.example.stopped"], false);
    }
}
