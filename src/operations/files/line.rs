use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{FindInFile, FindInFileInput};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LineBackup {
    #[default]
    Disabled,
    Enabled,
    Extension(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineInput {
    pub path: String,
    pub line: String,
    pub present: bool,
    pub replace: Option<String>,
    pub flags: Vec<String>,
    pub backup: LineBackup,
    pub interpolate_variables: bool,
    pub escape_regex_characters: bool,
    pub ensure_newline: bool,
    pub extended_regex: bool,
}

impl LineInput {
    #[must_use]
    pub fn new(path: impl Into<String>, line: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            line: line.into(),
            present: true,
            replace: None,
            flags: Vec::new(),
            backup: LineBackup::Disabled,
            interpolate_variables: false,
            escape_regex_characters: false,
            ensure_newline: false,
            extended_regex: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Line;

impl StatefulOperation for Line {
    type Input = LineInput;

    fn name(&self) -> &'static str {
        "files.line"
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
            let pattern = adjust_regex(&input.line, input.escape_regex_characters);
            let facts = FactExecutor::new(connector, fact_options);
            let matches = facts.get(&FindInFile, &find_input(input, pattern.clone())).await?;
            let replacement = input.replace.as_deref().filter(|value| !value.is_empty());
            let replacement_exists = if matches.as_ref().is_none_or(Vec::is_empty)
                && input.present
                && let Some(replacement) = replacement
            {
                let replacement_pattern = adjust_regex(replacement, false);
                facts
                    .get(&FindInFile, &find_input(input, replacement_pattern))
                    .await?
                    .is_some_and(|lines| !lines.is_empty())
            } else {
                false
            };
            Ok(line_commands(input, &pattern, matches.as_deref(), replacement_exists))
        })
    }
}

fn find_input(input: &LineInput, pattern: String) -> FindInFileInput {
    FindInFileInput {
        path: input.path.clone(),
        pattern,
        interpolate_variables: input.interpolate_variables,
        extended_regex: input.extended_regex,
    }
}

fn line_commands(
    input: &LineInput,
    pattern: &str,
    matches: Option<&[String]>,
    replacement_exists: bool,
) -> Vec<Command> {
    let has_matches = matches.is_some_and(|lines| !lines.is_empty());
    let replacement = input.replace.as_deref().filter(|value| !value.is_empty());
    if !has_matches && input.present && !replacement_exists {
        let value = replacement.map_or_else(|| strip_regex_anchors(&input.line), str::to_owned);
        return vec![append_command(input, &value)];
    }
    if has_matches && !input.present {
        return vec![sed_command(input, pattern, None)];
    }
    if has_matches
        && input.present
        && let Some(replacement) = replacement
        && matches.is_some_and(|lines| lines.iter().any(|line| line != replacement))
    {
        return vec![sed_command(input, pattern, Some(replacement))];
    }
    Vec::new()
}

fn append_command(input: &LineInput, value: &str) -> Command {
    let line = interpolated_or_quoted(value, input.interpolate_variables);
    let append = if input.ensure_newline {
        StringCommand::new(vec![
            raw("( [ $(tail -c1"),
            quoted(&input.path),
            raw("| wc -l) -eq 0 ] && echo ; echo"),
            line,
            raw(") >>"),
            quoted(&input.path),
        ])
    } else {
        StringCommand::new(vec![raw("echo"), line, raw(">>"), quoted(&input.path)])
    };
    if matches!(input.backup, LineBackup::Disabled) {
        Command::Shell(append)
    } else {
        let backup = format!("{}.{}", input.path, backup_extension(&input.backup));
        Command::Shell(StringCommand::new(vec![
            raw("cp"),
            quoted(&input.path),
            quoted(&backup),
            raw("&&"),
            append.into(),
        ]))
    }
}

fn sed_command(input: &LineInput, pattern: &str, replacement: Option<&str>) -> Command {
    let pattern = escape_sed(pattern);
    let flags = input.flags.join("");
    let script = replacement.map_or_else(
        || {
            let insensitive = flags.contains('i') || flags.contains('I');
            format!("/{pattern}/{}d", if insensitive { "I" } else { "" })
        },
        |replacement| format!("s/{pattern}/{}/{flags}", escape_sed_replacement(replacement)),
    );
    let extension = backup_extension(&input.backup);
    let backup = format!("{}.{}", input.path, extension);
    let mut bits = vec![raw("sed")];
    if input.extended_regex {
        bits.push(raw("-E"));
    }
    bits.extend([
        raw(format!("-i.{extension}")),
        interpolated_or_quoted(&script, input.interpolate_variables),
        quoted(&input.path),
    ]);
    if matches!(input.backup, LineBackup::Disabled) {
        bits.extend([raw("&&"), raw("rm"), raw("-f"), quoted(&backup)]);
    }
    Command::Shell(StringCommand::new(bits))
}

fn adjust_regex(line: &str, escape: bool) -> String {
    let mut line = if escape { escape_regex(line) } else { line.to_owned() };
    if !line.starts_with('^') {
        line.insert_str(0, "^.*");
    }
    if !line.ends_with('$') {
        line.push_str(".*$");
    }
    line
}

fn escape_regex(value: &str) -> String {
    const SPECIAL: &str = ".\\+*?[^]$(){}-";
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if SPECIAL.contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn strip_regex_anchors(value: &str) -> String {
    let value = value.strip_prefix('^').unwrap_or(value);
    if value.ends_with('$') && !value.ends_with("\\$") { value[..value.len() - 1].to_owned() } else { value.to_owned() }
}

pub(crate) fn escape_sed(value: &str) -> String {
    value.replace('/', "\\/")
}

fn escape_sed_replacement(value: &str) -> String {
    escape_sed(value).replace('&', "\\&")
}

fn backup_extension(backup: &LineBackup) -> String {
    match backup {
        LineBackup::Disabled => "rustinfra-line-backup".to_owned(),
        LineBackup::Extension(extension) => extension.clone(),
        LineBackup::Enabled => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or_else(|_| "rustinfra-line-backup".to_owned(), |duration| duration.as_secs().to_string()),
    }
}

fn interpolated_or_quoted(value: &str, interpolate: bool) -> CommandBit {
    if interpolate { raw(format!("\"{}\"", value.replace('"', "\\\""))) } else { quoted(value) }
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}

fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Line, LineBackup, LineInput, adjust_regex, line_commands};
    use crate::api::Command;
    use crate::connectors::{LocalConnector, RunOptions};
    use crate::facts::FactOptions;
    use crate::operations::{CommandExecutor, StatefulOperation};

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
    fn appends_missing_lines_without_leaking_regex_anchors() {
        let input = LineInput::new("some file", "^hello world$");
        let pattern = adjust_regex(&input.line, false);
        assert_eq!(
            descriptions(&line_commands(&input, &pattern, Some(&[]), false)),
            ["echo 'hello world' >> 'some file'"]
        );
    }

    #[test]
    fn replaces_and_removes_matching_lines_with_safe_sed_commands() {
        let mut input = LineInput::new("somefile", "match/line");
        input.replace = Some("replace&line".to_owned());
        input.flags = vec!["g".to_owned()];
        input.backup = LineBackup::Extension("backup".to_owned());
        let pattern = adjust_regex(&input.line, false);
        assert_eq!(
            descriptions(&line_commands(&input, &pattern, Some(&["match/line".to_owned()]), false)),
            ["sed -i.backup 's/^.*match\\/line.*$/replace\\&line/g' somefile"]
        );

        input.present = false;
        assert_eq!(
            descriptions(&line_commands(&input, &pattern, Some(&["match/line".to_owned()]), false)),
            ["sed -i.backup '/^.*match\\/line.*$/d' somefile"]
        );
    }

    #[tokio::test]
    async fn line_converges_a_local_file_and_rechecks_before_each_attempt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config");
        let input = LineInput::new(path.to_string_lossy(), "enabled=true");
        let facts = FactOptions::default();

        let first = Line.commands(&input, &LocalConnector, &facts).await.unwrap();
        CommandExecutor.execute(&LocalConnector, &first, &RunOptions::default()).await.unwrap();
        let second = Line.commands(&input, &LocalConnector, &facts).await.unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), "enabled=true\n");
        assert!(second.is_empty());
    }
}
