use std::error::Error;
use std::mem;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CrontabInput {
    pub user: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CrontabFile {
    pub items: Vec<CrontabItem>,
}

impl CrontabFile {
    #[must_use]
    pub fn command(&self, command: Option<&str>, name: Option<&str>) -> Option<&CrontabItem> {
        let name_comment = name.map(|name| format!("# pyinfra-name={name}"));
        self.items.iter().find(|item| {
            item.command.as_deref() == command
                || name_comment.as_ref().is_some_and(|comment| item.comments.contains(comment))
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CrontabItem {
    pub command: Option<String>,
    pub environment: Option<String>,
    pub minute: Option<String>,
    pub hour: Option<String>,
    pub month: Option<String>,
    pub day_of_week: Option<String>,
    pub day_of_month: Option<String>,
    pub comments: Vec<String>,
    pub special_time: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Crontab;

impl Fact for Crontab {
    type Input = CrontabInput;
    type Output = CrontabFile;

    fn name(&self) -> &'static str {
        "crontab.Crontab"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        let mut bits = vec![CommandBit::from("crontab -l")];
        if let Some(user) = &input.user {
            bits.extend([CommandBit::from("-u"), CommandBit::quoted(user.clone())]);
        }
        bits.push(CommandBit::from("|| true"));
        StringCommand::new(bits)
    }

    fn default(&self) -> Self::Output {
        CrontabFile::default()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("crontab")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut file = CrontabFile::default();
        let mut comments = Vec::new();
        for raw_line in output {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                comments.push(line.to_owned());
                continue;
            }
            let mut item = CrontabItem { comments: mem::take(&mut comments), ..CrontabItem::default() };
            if line.starts_with('@') {
                if let Some((special_time, command)) = line.split_once(char::is_whitespace) {
                    item.special_time = Some(special_time.to_owned());
                    item.command = Some(command.trim_start().to_owned());
                    file.items.push(item);
                }
            } else if is_environment_line(line) {
                item.environment = Some(line.to_owned());
                file.items.push(item);
            } else {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                if fields.len() >= 6 {
                    item.minute = Some(fields[0].to_owned());
                    item.hour = Some(fields[1].to_owned());
                    item.day_of_month = Some(fields[2].to_owned());
                    item.month = Some(fields[3].to_owned());
                    item.day_of_week = Some(fields[4].to_owned());
                    item.command = Some(fields[5..].join(" "));
                    file.items.push(item);
                }
            }
        }
        Ok(file)
    }
}

fn is_environment_line(line: &str) -> bool {
    let Some((name, _)) = line.split_once('=') else {
        return false;
    };
    let name = name.trim();
    if (name.starts_with('"') && name.ends_with('"')) || (name.starts_with('\'') && name.ends_with('\'')) {
        return name.len() >= 2;
    }
    let mut chars = name.chars();
    chars.next().is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use super::{Crontab, CrontabInput, Fact};

    #[test]
    fn parses_comments_environment_schedules_and_special_times() {
        let output = [
            "# pyinfra-name=backup".to_owned(),
            "0 1 * * 0 /bin/backup --all".to_owned(),
            "CRON_TZ = UTC".to_owned(),
            "@reboot /bin/start".to_owned(),
        ];
        let file = Crontab.process(&output, &CrontabInput::default()).unwrap();

        assert_eq!(file.items.len(), 3);
        assert_eq!(
            file.command(None, Some("backup")).and_then(|item| item.command.as_deref()),
            Some("/bin/backup --all")
        );
        assert_eq!(file.items[1].environment.as_deref(), Some("CRON_TZ = UTC"));
        assert_eq!(file.items[2].special_time.as_deref(), Some("@reboot"));
        assert_eq!(
            Crontab.command(&CrontabInput { user: Some("app user".to_owned()) }).raw_value(),
            "crontab -l -u 'app user' || true"
        );
    }
}
