use crate::api::{Command, CommandBit, StringCommand, remote_temporary_filename};
use crate::connectors::Connector;
use crate::facts::crontab::{Crontab as CrontabFact, CrontabFile, CrontabInput as CrontabFactInput, CrontabItem};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::escape_sed;

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronField {
    Value(String),
    List(Vec<String>),
}

impl CronField {
    fn display(&self) -> String {
        match self {
            Self::Value(value) => value.clone(),
            Self::List(values) => values.join(","),
        }
    }
}

impl From<&str> for CronField {
    fn from(value: &str) -> Self {
        Self::Value(value.to_owned())
    }
}

impl From<String> for CronField {
    fn from(value: String) -> Self {
        Self::Value(value)
    }
}

impl From<u8> for CronField {
    fn from(value: u8) -> Self {
        Self::Value(value.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrontabInput {
    pub command: String,
    pub present: bool,
    pub user: Option<String>,
    pub cron_name: Option<String>,
    pub minute: Option<CronField>,
    pub hour: Option<CronField>,
    pub month: Option<CronField>,
    pub day_of_week: Option<CronField>,
    pub day_of_month: Option<CronField>,
    pub special_time: Option<String>,
    pub interpolate_variables: bool,
}

impl CrontabInput {
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            present: true,
            user: None,
            cron_name: None,
            minute: None,
            hour: None,
            month: None,
            day_of_week: None,
            day_of_month: None,
            special_time: None,
            interpolate_variables: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Crontab;

impl StatefulOperation for Crontab {
    type Input = CrontabInput;

    fn name(&self) -> &'static str {
        "crontab.crontab"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let state = FactExecutor::new(connector, fact_options)
                .get(&CrontabFact, &CrontabFactInput { user: input.user.clone() })
                .await?;
            let temporary = remote_temporary_filename(
                &fact_options.run.remote_temp_directory,
                format!("crontab:{}", input.user.as_deref().unwrap_or_default()).as_bytes(),
            );
            Ok(crontab_commands(input, &state, &temporary))
        })
    }
}

fn crontab_commands(input: &CrontabInput, state: &CrontabFile, temporary: &str) -> Vec<Command> {
    let existing =
        state.command(input.cron_name.is_none().then_some(input.command.as_str()), input.cron_name.as_deref());
    let edit = required_edit(input, existing);
    let Some(edit) = edit else {
        return Vec::new();
    };

    let mut commands = Vec::new();
    if !state.is_empty() {
        let mut bits = vec![raw("crontab -l")];
        add_user(&mut bits, input.user.as_deref());
        bits.extend([raw(">"), quoted(temporary)]);
        commands.push(shell(bits));
    }
    match edit {
        CrontabEdit::Add(line) => {
            if !state.is_empty() {
                commands.push(shell([raw("echo '' >>"), quoted(temporary)]));
            }
            if let Some(name) = &input.cron_name {
                commands.push(append_line(&format!("# pyinfra-name={name}"), temporary));
            }
            commands.push(append_line(&line, temporary));
        }
        CrontabEdit::Remove(existing_command) => {
            commands.push(sed_edit(input, temporary, &existing_command, None));
        }
        CrontabEdit::Replace { existing_command, line } => {
            commands.push(sed_edit(input, temporary, &existing_command, Some(&line)));
        }
    }
    let mut bits = vec![raw("crontab")];
    add_user(&mut bits, input.user.as_deref());
    bits.push(quoted(temporary));
    commands.push(shell(bits));
    commands
}

enum CrontabEdit {
    Add(String),
    Remove(String),
    Replace { existing_command: String, line: String },
}

fn required_edit(input: &CrontabInput, existing: Option<&CrontabItem>) -> Option<CrontabEdit> {
    match (input.present, existing) {
        (false, None) => None,
        (true, Some(existing)) if entry_matches(input, existing) => None,
        (false, Some(existing)) => existing.command.clone().map(CrontabEdit::Remove),
        (true, None) => Some(CrontabEdit::Add(crontab_line(input))),
        (true, Some(existing)) => existing
            .command
            .clone()
            .map(|existing_command| CrontabEdit::Replace { existing_command, line: replacement_line(input, existing) }),
    }
}

fn entry_matches(input: &CrontabInput, existing: &CrontabItem) -> bool {
    if !input.present {
        return false;
    }
    let name_comment = input.cron_name.as_ref().map(|name| format!("# pyinfra-name={name}"));
    existing.command.as_deref() == Some(&input.command)
        && existing.special_time == input.special_time
        && name_comment.as_ref().is_none_or(|comment| existing.comments.contains(comment))
        && (input.special_time.is_some()
            || (existing.minute.as_deref() == Some(field(input.minute.as_ref()).as_str())
                && existing.hour.as_deref() == Some(field(input.hour.as_ref()).as_str())
                && existing.day_of_month.as_deref() == Some(field(input.day_of_month.as_ref()).as_str())
                && existing.month.as_deref() == Some(field(input.month.as_ref()).as_str())
                && existing.day_of_week.as_deref() == Some(field(input.day_of_week.as_ref()).as_str())))
}

fn replacement_line(input: &CrontabInput, existing: &CrontabItem) -> String {
    let line = crontab_line(input);
    let expected_comment = input.cron_name.as_ref().map(|name| format!("# pyinfra-name={name}"));
    if expected_comment.as_ref().is_some_and(|comment| !existing.comments.contains(comment)) {
        format!("{}\\n{line}", expected_comment.unwrap_or_default())
    } else {
        line
    }
}

fn crontab_line(input: &CrontabInput) -> String {
    input.special_time.as_ref().map_or_else(
        || {
            format!(
                "{} {} {} {} {} {}",
                field(input.minute.as_ref()),
                field(input.hour.as_ref()),
                field(input.day_of_month.as_ref()),
                field(input.month.as_ref()),
                field(input.day_of_week.as_ref()),
                input.command
            )
        },
        |special_time| format!("{special_time} {}", input.command),
    )
}

fn field(value: Option<&CronField>) -> String {
    value.map_or_else(|| "*".to_owned(), CronField::display)
}

fn sed_edit(input: &CrontabInput, temporary: &str, existing: &str, replacement: Option<&str>) -> Command {
    let pattern = format!(".*{}.*", escape_basic_regex(existing));
    let script = replacement.map_or_else(
        || format!("/{}/d", escape_sed_delimiter(&pattern)),
        |replacement| format!("s/{}/{}/", escape_sed_delimiter(&pattern), escape_sed_replacement(replacement)),
    );
    let script =
        if input.interpolate_variables { raw(format!("\"{}\"", script.replace('"', "\\\""))) } else { quoted(&script) };
    shell([
        raw("sed"),
        raw("-i.rustinfra-crontab-backup"),
        script,
        quoted(temporary),
        raw("&&"),
        raw("rm"),
        raw("-f"),
        quoted(&format!("{temporary}.rustinfra-crontab-backup")),
    ])
}

fn escape_basic_regex(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if matches!(character, '\\' | '.' | '*' | '[' | ']') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn escape_sed_delimiter(value: &str) -> String {
    escape_sed(value)
}

fn escape_sed_replacement(value: &str) -> String {
    escape_sed_delimiter(value).replace('&', "\\&")
}

fn append_line(line: &str, temporary: &str) -> Command {
    shell([raw("echo"), quoted(line), raw(">>"), quoted(temporary)])
}

fn add_user(bits: &mut Vec<CommandBit>, user: Option<&str>) {
    if let Some(user) = user {
        bits.extend([raw("-u"), quoted(user)]);
    }
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}

fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}

fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use super::{CronField, CrontabInput, crontab_commands};
    use crate::api::Command;
    use crate::facts::crontab::{CrontabFile, CrontabItem};

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some(command.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn adds_named_user_cron_with_list_schedule_and_safe_quoting() {
        let mut input = CrontabInput::new("/bin/run 'all'");
        input.user = Some("app user".to_owned());
        input.cron_name = Some("nightly".to_owned());
        input.minute = Some(CronField::List(vec!["0".to_owned(), "30".to_owned()]));
        let commands = descriptions(&crontab_commands(&input, &CrontabFile::default(), "/tmp/cron file"));

        assert_eq!(
            commands,
            [
                "echo '# pyinfra-name=nightly' >> '/tmp/cron file'",
                "echo '0,30 * * * * /bin/run '\"'\"'all'\"'\"'' >> '/tmp/cron file'",
                "crontab -u 'app user' '/tmp/cron file'"
            ]
        );
    }

    #[test]
    fn unchanged_special_time_is_noop_and_changed_command_is_replaced() {
        let state = CrontabFile {
            items: vec![CrontabItem {
                command: Some("/bin/old".to_owned()),
                comments: vec!["# pyinfra-name=start".to_owned()],
                special_time: Some("@reboot".to_owned()),
                ..CrontabItem::default()
            }],
        };
        let mut input = CrontabInput::new("/bin/old");
        input.cron_name = Some("start".to_owned());
        input.special_time = Some("@reboot".to_owned());
        assert!(crontab_commands(&input, &state, "/tmp/cron").is_empty());

        input.command = "/bin/new".to_owned();
        let commands = descriptions(&crontab_commands(&input, &state, "/tmp/cron"));
        assert_eq!(commands.len(), 3);
        assert!(commands[1].contains("s/.*\\/bin\\/old.*/@reboot \\/bin\\/new/"));
    }
}
