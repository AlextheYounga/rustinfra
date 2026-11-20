use std::error::Error;

use indexmap::{IndexMap, IndexSet};

use crate::api::StringCommand;

use super::Fact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunitInput {
    pub service: Option<String>,
    pub service_directory: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RunitStatus;

impl Fact for RunitStatus {
    type Input = RunitInput;
    type Output = IndexMap<String, Option<bool>>;
    fn name(&self) -> &'static str {
        "runit.RunitStatus"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        match &input.service {
            Some(service) => StringCommand::new([format!(
                "SVDIR={} sv status {}",
                double_quoted(&input.service_directory),
                double_quoted(service)
            )]),
            None => StringCommand::new([format!(
                "export SVDIR={} && cd \"$SVDIR\" && find * -maxdepth 0 -exec sv status {{}} + 2>/dev/null",
                double_quoted(&input.service_directory)
            )]),
        }
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("sv")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let (status, remainder) = line.split_once(": ")?;
                let (service, _) = remainder.split_once(": ")?;
                Some((
                    service.to_owned(),
                    match status {
                        "run" => Some(true),
                        "down" => Some(false),
                        _ => None,
                    },
                ))
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RunitManaged;

impl Fact for RunitManaged {
    type Input = RunitInput;
    type Output = IndexSet<String>;
    fn name(&self) -> &'static str {
        "runit.RunitManaged"
    }
    fn command(&self, input: &Self::Input) -> StringCommand {
        match &input.service {
            Some(service) => StringCommand::new([format!(
                "cd {} && test -h {} && echo {} || true",
                double_quoted(&input.service_directory),
                double_quoted(service),
                double_quoted(service)
            )]),
            None => StringCommand::new([format!(
                "[ -d {} ] && cd {} && find -mindepth 1 -maxdepth 1 -type l -printf \"%f\\n\" || true",
                double_quoted(&input.service_directory),
                double_quoted(&input.service_directory)
            )]),
        }
    }
    fn default(&self) -> Self::Output {
        IndexSet::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("sv")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().cloned().collect())
    }
}

fn double_quoted(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::{Fact, RunitInput, RunitManaged, RunitStatus};

    #[test]
    fn commands_quote_directories_and_statuses_preserve_fail_state() {
        let input =
            RunitInput { service: Some("agetty-tty1".to_owned()), service_directory: "/var/service".to_owned() };
        assert_eq!(RunitStatus.command(&input).raw_value(), "SVDIR=\"/var/service\" sv status \"agetty-tty1\"");
        let statuses = RunitStatus
            .process(&["run: agetty-tty1: (pid 1)".to_owned(), "fail: bar: runsv not running".to_owned()], &input)
            .unwrap();
        assert_eq!(statuses["agetty-tty1"], Some(true));
        assert_eq!(statuses["bar"], None);
        assert_eq!(RunitManaged.process(&["agetty-tty1".to_owned()], &input).unwrap().len(), 1);
    }
}
