use crate::api::{Command, CommandBit};
use crate::connectors::Connector;
use crate::facts::files::{
    Directory, File, FindInFile, FindInFileInput, Flags as FlagsFact, Md5File, PathInfo, PathInput, Sha1File,
    Sha256File,
};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};
use std::collections::BTreeSet;
use std::error::Error;

use super::{FilesOperationError, boxed, quoted, raw, shell};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveInput {
    pub source: String,
    pub destination: String,
    pub overwrite: bool,
}

impl MoveInput {
    #[must_use]
    pub fn new(source: impl Into<String>, destination: impl Into<String>) -> Self {
        Self { source: source.into(), destination: destination.into(), overwrite: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Move;

impl StatefulOperation for Move {
    type Input = MoveInput;
    fn name(&self) -> &'static str {
        "files.move"
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
            let facts = FactExecutor::new(connector, options);
            let destination = join_destination(&input.destination, &input.source);
            let directory = facts.get(&Directory, &PathInput::from(input.destination.clone())).await?;
            let target = facts.get(&File, &PathInput::from(destination.clone())).await?;
            move_commands(input, &destination, &directory, &target).map_err(boxed)
        })
    }
}

fn move_commands(
    input: &MoveInput,
    full_destination: &str,
    directory: &PathInfo,
    target: &PathInfo,
) -> Result<Vec<Command>, FilesOperationError> {
    if matches!(directory, PathInfo::DifferentType) {
        return Err(FilesOperationError::DestinationNotDirectory(input.destination.clone()));
    }
    let mut commands = Vec::new();
    if !matches!(target, PathInfo::Missing) {
        if !input.overwrite {
            return Err(FilesOperationError::DestinationExists(full_destination.to_owned()));
        }
        commands.push(shell([raw("rm"), raw("-rf"), quoted(full_destination)]));
    }
    commands.push(shell([raw("mv"), quoted(&input.source), quoted(&input.destination)]));
    Ok(commands)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameInput {
    pub source: String,
    pub destination: String,
    pub overwrite: bool,
}

impl RenameInput {
    #[must_use]
    pub fn new(source: impl Into<String>, destination: impl Into<String>) -> Self {
        Self { source: source.into(), destination: destination.into(), overwrite: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rename;

impl StatefulOperation for Rename {
    type Input = RenameInput;
    fn name(&self) -> &'static str {
        "files.rename"
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
            let facts = FactExecutor::new(connector, options);
            let source = facts.get(&File, &PathInput::from(input.source.clone())).await?;
            let target = facts.get(&File, &PathInput::from(input.destination.clone())).await?;
            rename_commands(input, &source, &target).map_err(boxed)
        })
    }
}

fn rename_commands(
    input: &RenameInput,
    source: &PathInfo,
    target: &PathInfo,
) -> Result<Vec<Command>, FilesOperationError> {
    if matches!(source, PathInfo::Missing) {
        return Err(FilesOperationError::MissingSource(input.source.clone()));
    }
    let mut commands = Vec::new();
    if !matches!(target, PathInfo::Missing) {
        if !input.overwrite {
            return Err(FilesOperationError::DestinationExists(input.destination.clone()));
        }
        commands.push(shell([raw("rm"), raw("-rf"), quoted(&input.destination)]));
    }
    commands.push(shell([raw("mv"), quoted(&input.source), quoted(&input.destination)]));
    Ok(commands)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopyInput {
    pub source: String,
    pub destination: String,
    pub overwrite: bool,
}

impl CopyInput {
    #[must_use]
    pub fn new(source: impl Into<String>, destination: impl Into<String>) -> Self {
        Self { source: source.into(), destination: destination.into(), overwrite: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Copy;

impl StatefulOperation for Copy {
    type Input = CopyInput;
    fn name(&self) -> &'static str {
        "files.copy"
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
            let facts = FactExecutor::new(connector, options);
            let directory = facts.get(&Directory, &PathInput::from(input.destination.clone())).await?;
            let target_path = join_destination(&input.destination, &input.source);
            let target = facts.get(&File, &PathInput::from(target_path.clone())).await?;
            let equal = if !input.overwrite && !matches!(target, PathInfo::Missing) {
                remote_files_equal(&facts, &input.source, &target_path).await?
            } else {
                false
            };
            copy_commands(input, &CopyState { target_path, directory, target, equal }).map_err(boxed)
        })
    }
}

async fn remote_files_equal(
    facts: &FactExecutor<'_>,
    source: &str,
    target: &str,
) -> Result<bool, Box<dyn Error + Send + Sync>> {
    let source_input = PathInput::from(source);
    let target_input = PathInput::from(target);
    let a = facts.get(&Sha1File, &source_input).await?;
    let b = facts.get(&Sha1File, &target_input).await?;
    if a.is_some() && a == b {
        return Ok(true);
    }
    let a = facts.get(&Md5File, &source_input).await?;
    let b = facts.get(&Md5File, &target_input).await?;
    if a.is_some() && a == b {
        return Ok(true);
    }
    let a = facts.get(&Sha256File, &source_input).await?;
    let b = facts.get(&Sha256File, &target_input).await?;
    Ok(a.is_some() && a == b)
}

struct CopyState {
    target_path: String,
    directory: PathInfo,
    target: PathInfo,
    equal: bool,
}

fn copy_commands(input: &CopyInput, state: &CopyState) -> Result<Vec<Command>, FilesOperationError> {
    if matches!(state.directory, PathInfo::DifferentType) {
        return Err(FilesOperationError::DestinationNotDirectory(input.destination.clone()));
    }
    if !input.overwrite && !matches!(state.target, PathInfo::Missing) {
        if state.equal {
            return Ok(Vec::new());
        }
        return Err(FilesOperationError::DestinationDifferent(state.target_path.clone()));
    }
    let mut bits = vec![raw("cp"), raw("-r")];
    if input.overwrite {
        bits.push(raw("-f"));
    }
    bits.extend([quoted(&input.source), quoted(&input.destination)]);
    Ok(vec![shell(bits)])
}

fn join_destination(destination: &str, source: &str) -> String {
    let name = source.trim_end_matches('/').rsplit('/').next().unwrap_or(source);
    format!("{}/{name}", destination.trim_end_matches('/'))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlagsInput {
    pub path: String,
    pub flags: Vec<String>,
    pub present: bool,
}

impl FlagsInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), flags: Vec::new(), present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Flags;

impl StatefulOperation for Flags {
    type Input = FlagsInput;
    fn name(&self) -> &'static str {
        "files.flags"
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
            if input.flags.is_empty() {
                return Ok(Vec::new());
            }
            let current = FactExecutor::new(connector, options).get(&FlagsFact, &input.path).await?;
            Ok(flags_commands(input, &current))
        })
    }
}

fn flags_commands(input: &FlagsInput, current: &[String]) -> Vec<Command> {
    let requested: BTreeSet<_> = input.flags.iter().cloned().collect();
    let current: BTreeSet<_> = current.iter().cloned().collect();
    let changes: Vec<_> = if input.present {
        requested.difference(&current).cloned().collect()
    } else {
        requested.intersection(&current).cloned().collect()
    };
    if changes.is_empty() {
        return Vec::new();
    }
    let value = changes
        .into_iter()
        .map(|flag| if input.present { flag } else { format!("no{flag}") })
        .collect::<Vec<_>>()
        .join(",");
    vec![shell([raw("chflags"), raw(value), quoted(&input.path)])]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceInput {
    pub path: String,
    pub text: String,
    pub replacement: String,
    pub flags: Vec<String>,
    pub backup_extension: Option<String>,
    pub interpolate_variables: bool,
    pub extended_regex: bool,
}

impl ReplaceInput {
    #[must_use]
    pub fn new(path: impl Into<String>, text: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
            replacement: replacement.into(),
            flags: Vec::new(),
            backup_extension: None,
            interpolate_variables: false,
            extended_regex: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Replace;

impl StatefulOperation for Replace {
    type Input = ReplaceInput;
    fn name(&self) -> &'static str {
        "files.replace"
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
            let find = FindInFileInput {
                path: input.path.clone(),
                pattern: input.text.clone(),
                interpolate_variables: input.interpolate_variables,
                extended_regex: input.extended_regex,
            };
            let matches = FactExecutor::new(connector, options).get(&FindInFile, &find).await?;
            Ok(if matches.as_ref().is_none_or(Vec::is_empty) { Vec::new() } else { vec![replace_command(input)] })
        })
    }
}

fn replace_command(input: &ReplaceInput) -> Command {
    let extension = input.backup_extension.as_deref().unwrap_or("rustinfra-replace-backup");
    let script = format!(
        "s/{}/{}/{}",
        super::escape_sed(&input.text),
        super::escape_sed(&input.replacement).replace('&', "\\&"),
        input.flags.join("")
    );
    let script = if input.interpolate_variables {
        CommandBit::from(format!("\"{}\"", script.replace('"', "\\\"")))
    } else {
        CommandBit::quoted(script)
    };
    let mut bits = vec![raw("sed")];
    if input.extended_regex {
        bits.push(raw("-E"));
    }
    bits.extend([raw(format!("-i.{extension}")), script, quoted(&input.path)]);
    if input.backup_extension.is_none() {
        bits.extend([raw("&&"), raw("rm"), raw("-f"), quoted(&format!("{}.{extension}", input.path))]);
    }
    shell(bits)
}

#[cfg(test)]
#[path = "misc_tests.rs"]
mod tests;
