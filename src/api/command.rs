use std::fmt;
use std::path::PathBuf;

use super::HiddenValue;

/// One composable part of a shell command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandBit {
    String(String),
    Hidden(HiddenValue),
    Command(Box<StringCommand>),
    Quoted(Box<Self>),
}

impl CommandBit {
    /// Quotes a value as one POSIX shell word.
    pub fn quoted(value: impl Into<Self>) -> Self {
        Self::Quoted(Box::new(value.into()))
    }

    fn raw_value(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Hidden(value) => value.unmask().to_owned(),
            Self::Command(command) => command.raw_value(),
            Self::Quoted(value) => quote_shell_word(&value.raw_value()),
        }
    }

    fn masked_value(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Hidden(value) => value.masked_value().to_owned(),
            Self::Command(command) => command.masked_value(),
            Self::Quoted(value) => quote_shell_word(&value.masked_value()),
        }
    }
}

impl From<&str> for CommandBit {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for CommandBit {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<PathBuf> for CommandBit {
    fn from(value: PathBuf) -> Self {
        Self::String(value.display().to_string())
    }
}

impl From<HiddenValue> for CommandBit {
    fn from(value: HiddenValue) -> Self {
        Self::Hidden(value)
    }
}

impl From<StringCommand> for CommandBit {
    fn from(value: StringCommand) -> Self {
        Self::Command(Box::new(value))
    }
}

macro_rules! command_bit_from_integer {
    ($($integer:ty),+ $(,)?) => {
        $(
            impl From<$integer> for CommandBit {
                fn from(value: $integer) -> Self {
                    Self::String(value.to_string())
                }
            }
        )+
    };
}

command_bit_from_integer!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

/// A shell command assembled from values with explicit masking and quoting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StringCommand {
    bits: Vec<CommandBit>,
    separator: String,
}

impl StringCommand {
    /// Creates a command whose bits are separated by spaces.
    #[must_use]
    pub fn new(bits: impl IntoIterator<Item = impl Into<CommandBit>>) -> Self {
        Self::with_separator(bits, " ")
    }

    /// Creates a command with the specified separator between bits.
    #[must_use]
    pub fn with_separator(bits: impl IntoIterator<Item = impl Into<CommandBit>>, separator: impl Into<String>) -> Self {
        Self { bits: bits.into_iter().map(Into::into).collect(), separator: separator.into() }
    }

    /// Returns the command that must be sent to the target.
    #[must_use]
    pub fn raw_value(&self) -> String {
        self.bits.iter().map(CommandBit::raw_value).collect::<Vec<_>>().join(&self.separator)
    }

    /// Returns the command safe to print in output and diagnostics.
    #[must_use]
    pub fn masked_value(&self) -> String {
        self.bits.iter().map(CommandBit::masked_value).collect::<Vec<_>>().join(&self.separator)
    }
}

impl fmt::Display for StringCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.masked_value())
    }
}

/// A command sent through a connector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Shell(StringCommand),
    Upload(FileUploadCommand),
    UploadContent(ContentUploadCommand),
    Download(FileDownloadCommand),
    Rsync(RsyncCommand),
}

/// Uploads generated content to a target path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentUploadCommand {
    pub content: Vec<u8>,
    pub destination: String,
    pub remote_temporary_filename: Option<String>,
}

/// Uploads a local file to a target path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileUploadCommand {
    pub source: PathBuf,
    pub destination: String,
    pub remote_temporary_filename: Option<String>,
}

/// Downloads a target file to a local path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDownloadCommand {
    pub source: String,
    pub destination: PathBuf,
    pub remote_temporary_filename: Option<String>,
}

/// Synchronizes a local path to a target using the local rsync executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RsyncCommand {
    pub source: PathBuf,
    pub destination: String,
    pub flags: Vec<String>,
}

fn quote_shell_word(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }

    let is_safe = value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-')
    });
    if is_safe {
        return value.to_owned();
    }

    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::{CommandBit, StringCommand};
    use crate::api::HiddenValue;

    #[test]
    fn nested_commands_preserve_raw_and_masked_values() {
        let token =
            StringCommand::with_separator(vec![CommandBit::from("--token "), HiddenValue::new("s3cret").into()], "");
        let command = StringCommand::new(vec![CommandBit::from("run"), token.into()]);

        assert_eq!(command.raw_value(), "run --token s3cret");
        assert_eq!(command.masked_value(), "run --token *MASKED*");
        assert_eq!(command.to_string(), "run --token *MASKED*");
    }

    #[test]
    fn quoted_values_match_python_shlex_quote_behavior() {
        let command = StringCommand::new(vec![
            CommandBit::from("echo"),
            CommandBit::quoted("quote me!"),
            CommandBit::quoted("has'quote"),
            CommandBit::quoted(""),
        ]);

        assert_eq!(command.raw_value(), "echo 'quote me!' 'has'\"'\"'quote' ''");
    }

    #[test]
    fn quoted_hidden_values_are_masked_but_execute_with_clear_text() {
        let command =
            StringCommand::new(vec![CommandBit::from("echo"), CommandBit::quoted(HiddenValue::new("abc; id"))]);

        assert_eq!(command.raw_value(), "echo 'abc; id'");
        assert_eq!(command.masked_value(), "echo '*MASKED*'");
    }
}
