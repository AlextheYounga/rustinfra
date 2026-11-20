use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;
use super::sysvinit::{ServiceStatusMap, parse_status_lines};

const RCD_STATUS_COMMAND: &str = r#"
            for SERVICE in `find /etc/rc.d /usr/local/etc/rc.d -type f`; do
                $SERVICE status 2> /dev/null || $SERVICE check 2> /dev/null
                echo "`basename $SERVICE`=$?"
            done
        "#;

#[derive(Clone, Copy, Debug, Default)]
pub struct RcdStatus;

impl Fact for RcdStatus {
    type Input = ();
    type Output = ServiceStatusMap;
    fn name(&self) -> &'static str {
        "bsdinit.RcdStatus"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new([RCD_STATUS_COMMAND])
    }
    fn default(&self) -> Self::Output {
        ServiceStatusMap::new()
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_status_lines(output))
    }
}

pub type EnabledServices = IndexMap<String, bool>;

macro_rules! enabled_fact {
    ($name:ident, $fact_name:literal, $command:literal, $required:literal, $parser:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = ();
            type Output = EnabledServices;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, _input: &Self::Input) -> StringCommand {
                StringCommand::new([$command])
            }
            fn default(&self) -> Self::Output {
                EnabledServices::new()
            }
            fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
                Some($required)
            }
            fn process(
                &self,
                output: &[String],
                _input: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(output.iter().map(|line| (($parser)(line), true)).collect())
            }
        }
    };
}

enabled_fact!(RcdEnabled, "bsdinit.RcdEnabled", "rcctl ls on", "rcctl", |line: &String| line.clone());
enabled_fact!(ServiceEnabled, "bsdinit.ServiceEnabled", "service -e", "service", |line: &String| line
    .rsplit('/')
    .next()
    .unwrap_or(line)
    .to_owned());

#[cfg(test)]
mod tests {
    use super::{Fact, RcdEnabled, ServiceEnabled};

    #[test]
    fn parses_openbsd_and_freebsd_enabled_services() {
        assert_eq!(RcdEnabled.process(&["cron".to_owned()], &()).unwrap()["cron"], true);
        assert_eq!(ServiceEnabled.process(&["/usr/local/etc/rc.d/rad".to_owned()], &()).unwrap()["rad"], true);
    }
}
