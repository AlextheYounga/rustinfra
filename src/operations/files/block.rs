use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{BLOCK_BEGIN, BLOCK_END, BLOCK_MARKER, Block as BlockFact, BlockInput as BlockFactInput};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockInput {
    pub path: String,
    pub content: Vec<String>,
    pub present: bool,
    pub line: Option<String>,
    pub backup_extension: Option<String>,
    pub escape_regex_characters: bool,
    pub before: bool,
    pub after: bool,
    pub marker: String,
    pub begin: String,
    pub end: String,
    pub preserve_metadata: bool,
}

impl BlockInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: Vec::new(),
            present: true,
            line: None,
            backup_extension: None,
            escape_regex_characters: false,
            before: false,
            after: false,
            marker: BLOCK_MARKER.to_owned(),
            begin: BLOCK_BEGIN.to_owned(),
            end: BLOCK_END.to_owned(),
            preserve_metadata: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Block;

impl StatefulOperation for Block {
    type Input = BlockInput;
    fn name(&self) -> &'static str {
        "files.block"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            validate(input).map_err(|error| Box::new(error) as Box<dyn Error + Send + MarkerSync>)?;
            let fact_input = BlockFactInput {
                path: input.path.clone(),
                marker: input.marker.clone(),
                begin: input.begin.clone(),
                end: input.end.clone(),
            };
            let current = FactExecutor::new(connector, options).get(&BlockFact, &fact_input).await?;
            Ok(block_commands(input, current.as_deref(), &options.run.remote_temp_directory))
        })
    }
}

fn validate(input: &BlockInput) -> Result<(), BlockError> {
    if input.present && input.content.is_empty() {
        return Err(BlockError::MissingContent);
    }
    if input.present && input.line.is_some() && input.before == input.after {
        return Err(BlockError::LineDirection);
    }
    if input.present && input.line.is_none() && input.before != input.after {
        return Err(BlockError::MissingLine);
    }
    Ok(())
}

fn block_commands(input: &BlockInput, current: Option<&[String]>, temp_directory: &str) -> Vec<Command> {
    if input.present && current.is_some_and(|value| value == input.content) {
        return Vec::new();
    }
    if !input.present && current.is_none_or(<[String]>::is_empty) {
        return Vec::new();
    }
    let start = input.marker.replace("{mark}", &input.begin);
    let end = input.marker.replace("{mark}", &input.end);
    let block = [&[start.clone()][..], &input.content, &[end.clone()][..]].concat().join("\n");
    let existing = current.is_some();
    let prefix = output_prefix(input, temp_directory, existing);
    let suffix = output_suffix(input, existing);
    let path = quote(&input.path);
    let body = if !input.present {
        format!("awk {} {path} > \"$OUT\"", quote(&format!("/{start}/,/{end}/ {{next}} 1")))
    } else if current.is_none() || (current.is_some_and(<[String]>::is_empty) && input.before == input.after) {
        let original = if existing { path } else { "/dev/null".to_owned() };
        if input.before {
            format!("{{ printf '%s\\n' {}; cat {original}; }} > \"$OUT\"", quote(&block))
        } else {
            format!("{{ cat {original}; printf '%s\\n' {}; }} > \"$OUT\"", quote(&block))
        }
    } else if current.is_some_and(<[String]>::is_empty) {
        let line = input.line.as_deref().unwrap_or_default();
        let regex = if input.escape_regex_characters { escape_regex(line) } else { line.to_owned() };
        let before = if input.before { "{ print }" } else { "" };
        let after = if input.after { "{ print }" } else { "" };
        let program = format!(
            "BEGIN {{x=ARGV[2]; ARGV[2]=\"\"}} {after} f!=1 && /{regex}/ {{ print x; f=1}} END {{if (f==0) print x }} {before}"
        );
        format!("awk {} {path} {} > \"$OUT\"", quote(&program), quote(&block))
    } else {
        let program = format!(
            "BEGIN {{f=1; x=ARGV[2]; ARGV[2]=\"\"}} /{start}/ {{print; print x; f=0}} /{end}/ {{print; f=1; next}} f"
        );
        format!("awk {} {path} {} > \"$OUT\"", quote(&program), quote(&input.content.join("\n")))
    };
    vec![Command::Shell(StringCommand::new([format!("{prefix}{body}{suffix}")]))]
}

fn output_prefix(input: &BlockInput, temp_directory: &str, existing: bool) -> String {
    let path = quote(&input.path);
    let backup = input.backup_extension.as_ref().filter(|_| existing).map_or_else(String::new, |extension| {
        format!("cp {path} {} && ", quote(&format!("{}.{}", input.path, extension)))
    });
    let temporary = format!("OUT=\"$(TMPDIR={} mktemp -t pyinfra.XXXXXX)\" && ", quote(temp_directory));
    if input.preserve_metadata && existing {
        format!(
            "{backup}{temporary}MODE=\"$(stat -c %a {path} 2>/dev/null || stat -f %Lp {path} 2>/dev/null)\" && OWNER=\"$(stat -c \"%u:%g\" {path} 2>/dev/null || stat -f \"%u:%g\" {path} 2>/dev/null || echo $(id -un):$(id -gn))\" && "
        )
    } else {
        format!("{backup}{temporary}")
    }
}

fn output_suffix(input: &BlockInput, existing: bool) -> String {
    let path = quote(&input.path);
    if input.preserve_metadata && existing {
        format!(" && mv \"$OUT\" {path} && chown \"$OWNER\" {path} && chmod \"$MODE\" {path}")
    } else {
        format!(" && mv \"$OUT\" {path}")
    }
}

fn quote(value: &str) -> String {
    StringCommand::new([CommandBit::quoted(value)]).raw_value()
}

fn escape_regex(value: &str) -> String {
    const SPECIAL: &str = ".\\+*?[^]$(){}-|";
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if SPECIAL.contains(character) {
            output.push('\\');
        }
        output.push(character);
    }
    output
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
enum BlockError {
    #[error("'content' must be supplied when 'present' == true")]
    MissingContent,
    #[error("only one of 'before' or 'after' may be used when 'line' is specified")]
    LineDirection,
    #[error("'line' must be supplied or 'before' and 'after' must be equal")]
    MissingLine,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn add_update_remove_and_validation_match_reference_boundaries() {
        let mut input = BlockInput::new("/tmp/my file");
        input.content = vec!["literal $(touch /tmp/pwn)".to_owned()];
        let add = block_commands(&input, None, "/tmp dir");
        assert!(matches!(&add[0], Command::Shell(command)
            if command.raw_value().contains("printf '%s\\n' '")
                && command.raw_value().contains("$(touch /tmp/pwn)")));
        assert!(block_commands(&input, Some(&input.content), "/tmp").is_empty());
        input.present = false;
        assert!(
            matches!(&block_commands(&input, Some(&["old".to_owned()]), "/tmp")[0], Command::Shell(command) if command.raw_value().contains("{next} 1"))
        );
        input.present = true;
        input.content.clear();
        assert_eq!(validate(&input), Err(BlockError::MissingContent));
    }
}
use std::error::Error;
use std::marker::Sync as MarkerSync;
