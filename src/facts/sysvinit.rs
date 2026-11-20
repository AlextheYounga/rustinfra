use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;

pub type ServiceStatusMap = IndexMap<String, Option<bool>>;

pub(crate) const INITD_STATUS_COMMAND: &str = r####"
        for SERVICE in `ls /etc/init.d/`; do
            _=`cat /etc/init.d/$SERVICE | grep "### BEGIN INIT INFO"`

            if [ "$?" = "0" ]; then
                STATUS=`/etc/init.d/$SERVICE status`
                echo "$SERVICE=$?"
            fi
        done
    "####;

#[derive(Clone, Copy, Debug, Default)]
pub struct InitdStatus;

impl Fact for InitdStatus {
    type Input = ();
    type Output = ServiceStatusMap;

    fn name(&self) -> &'static str {
        "sysvinit.InitdStatus"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new([INITD_STATUS_COMMAND])
    }
    fn default(&self) -> Self::Output {
        ServiceStatusMap::new()
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_status_lines(output))
    }
}

pub(crate) fn parse_status_lines(output: &[String]) -> ServiceStatusMap {
    output
        .iter()
        .filter_map(|line| {
            let (name, status) = line.rsplit_once('=')?;
            if name.is_empty() || !name.chars().all(|value| value.is_ascii_alphanumeric() || "_-.".contains(value)) {
                return None;
            }
            let code = status.parse::<u8>().ok()?;
            Some((
                name.to_owned(),
                if code == 0 {
                    Some(true)
                } else if code < 4 {
                    Some(false)
                } else {
                    None
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Fact, InitdStatus};

    #[test]
    fn parses_lsb_status_codes() {
        let statuses = InitdStatus
            .process(&["running=0".to_owned(), "stopped=3".to_owned(), "unknown=4".to_owned()], &())
            .unwrap();
        assert_eq!(statuses["running"], Some(true));
        assert_eq!(statuses["stopped"], Some(false));
        assert_eq!(statuses["unknown"], None);
    }
}
