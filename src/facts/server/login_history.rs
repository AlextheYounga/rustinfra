use std::error::Error;

use crate::api::StringCommand;
use crate::facts::Fact;

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LastRecord {
    pub user: String,
    pub tty: String,
    pub host: String,
    pub time: String,
}

fn parse_records(output: &[String]) -> Vec<LastRecord> {
    output
        .iter()
        .filter_map(|line| {
            let line = line.trim_end();
            let lower = line.to_ascii_lowercase();
            if line.is_empty()
                || lower.starts_with("wtmp begins")
                || lower.starts_with("btmp begins")
                || lower.starts_with("user ")
                || lower.starts_with("username ")
            {
                return None;
            }
            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 3 {
                return None;
            }
            let (tty, rest) = if parts[0] == "reboot" && parts.get(1..3) == Some(&["system", "boot"]) {
                ("system boot".to_owned(), &parts[3..])
            } else {
                (parts[1].to_owned(), &parts[2..])
            };
            let first = rest.first()?;
            let (host, time) = if WEEKDAYS.contains(first) {
                (String::new(), rest.join(" "))
            } else {
                (first.to_string(), rest.get(1..)?.join(" "))
            };
            Some(LastRecord { user: parts[0].to_owned(), tty, host, time })
        })
        .collect()
}

macro_rules! last_fact {
    ($name:ident, $fact_name:literal, $command:literal, $required:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = ();
            type Output = Vec<LastRecord>;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, _: &()) -> StringCommand {
                StringCommand::new([$command])
            }
            fn default(&self) -> Self::Output {
                Vec::new()
            }
            fn requires_command(&self, _: &()) -> Option<&str> {
                Some($required)
            }
            fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(parse_records(output))
            }
        }
    };
}

last_fact!(Last, "server.Last", "last -Fwi 2>/dev/null || last -w 2>/dev/null || last 2>/dev/null || true", "last");
last_fact!(Lastb, "server.Lastb", "lastb -Fwi 2>/dev/null || lastb 2>/dev/null || true", "lastb");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_reboot_empty_host_and_footer_records() {
        let output = [
            "alice pts/0 192.168.1.5 Thu Apr 17 14:00:00 2026 still logged in",
            "reboot system boot 6.19.10 Thu Apr 17 11:00:00 2026 - Thu Apr 17 12:00:00 2026 (01:00)",
            "carol pts/2 Thu Apr 17 09:00:00 2026 - Thu Apr 17 09:30:00 2026 (00:30)",
            "wtmp begins Thu Apr 17 09:00:00 2026",
        ]
        .map(str::to_owned);
        let records = Last.process(&output, &()).unwrap();
        assert_eq!(records[0].host, "192.168.1.5");
        assert_eq!(records[1].tty, "system boot");
        assert_eq!(records[2].host, "");
        assert_eq!(records.len(), 3);
        assert_eq!(Last.requires_command(&()), Some("last"));
        assert_eq!(Lastb.requires_command(&()), Some("lastb"));
        assert!(Last.process(&[], &()).unwrap().is_empty());
        assert_eq!(Lastb.process(&output, &()).unwrap(), records);
    }
}
