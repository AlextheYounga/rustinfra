use std::collections::BTreeMap;
use std::error::Error;

use chrono::{Duration, Utc};

use crate::api::{Command, CommandBit, StringCommand, remote_temporary_filename};
use crate::connectors::Connector;
use crate::facts::files::{File, Md5File, PathInfo, PathInput, Sha1File, Sha256File, Sha384File};
use crate::facts::server::{Date, Which, command_available};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

use super::{FilesOperationError, chmod, chown, quoted, raw, shell};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadInput {
    pub source: String,
    pub destination: String,
    pub user: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u16>,
    pub cache_time_seconds: Option<i64>,
    pub force: bool,
    pub sha384: Option<String>,
    pub sha256: Option<String>,
    pub sha1: Option<String>,
    pub md5: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub insecure: bool,
    pub proxy: Option<String>,
    pub limit_rate: Option<String>,
    pub extra_curl_args: BTreeMap<String, String>,
    pub extra_wget_args: BTreeMap<String, String>,
}

impl DownloadInput {
    #[must_use]
    pub fn new(source: impl Into<String>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            user: None,
            group: None,
            mode: None,
            cache_time_seconds: None,
            force: false,
            sha384: None,
            sha256: None,
            sha1: None,
            md5: None,
            headers: BTreeMap::new(),
            insecure: false,
            proxy: None,
            limit_rate: None,
            extra_curl_args: BTreeMap::new(),
            extra_wget_args: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Download;

impl StatefulOperation for Download {
    type Input = DownloadInput;
    fn name(&self) -> &'static str {
        "files.download"
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
            let info = facts.get(&File, &PathInput::from(input.destination.clone())).await?;
            if matches!(info, PathInfo::DifferentType) {
                return Err(Box::new(FilesOperationError::WrongType {
                    path: input.destination.clone(),
                    expected: "file",
                }) as _);
            }
            let mut fetch = input.force || matches!(info, PathInfo::Missing);
            if let PathInfo::Present(current) = &info {
                if let Some(seconds) = input.cache_time_seconds {
                    let now = facts.get(&Date, &()).await.unwrap_or_else(|_| Utc::now().fixed_offset());
                    fetch |=
                        current.modified_at.is_some_and(|mtime| mtime < (now - Duration::seconds(seconds)).timestamp());
                }
                fetch |= checksum_changed(&facts, &input.destination, input).await?;
            }
            if !fetch {
                return Ok(metadata_commands(input, &info));
            }
            let curl = command_available(facts.get(&Which, &"curl".to_owned()).await?.as_deref());
            let wget =
                if curl { false } else { command_available(facts.get(&Which, &"wget".to_owned()).await?.as_deref()) };
            Ok(download_commands(input, &options.run.remote_temp_directory, curl, wget))
        })
    }
}

async fn checksum_changed(
    facts: &FactExecutor<'_>,
    path: &str,
    input: &DownloadInput,
) -> Result<bool, Box<dyn Error + Send + Sync>> {
    let path = PathInput::from(path);
    if let Some(expected) = &input.sha1
        && facts.get(&Sha1File, &path).await?.as_ref() != Some(expected)
    {
        return Ok(true);
    }
    if let Some(expected) = &input.sha256
        && facts.get(&Sha256File, &path).await?.as_ref() != Some(expected)
    {
        return Ok(true);
    }
    if let Some(expected) = &input.sha384
        && facts.get(&Sha384File, &path).await?.as_ref() != Some(expected)
    {
        return Ok(true);
    }
    if let Some(expected) = &input.md5
        && facts.get(&Md5File, &path).await?.as_ref() != Some(expected)
    {
        return Ok(true);
    }
    Ok(false)
}

fn metadata_commands(input: &DownloadInput, info: &PathInfo) -> Vec<Command> {
    let PathInfo::Present(info) = info else { return Vec::new() };
    let mut commands = Vec::new();
    if input.user.as_deref().is_some_and(|value| value != info.user)
        || input.group.as_deref().is_some_and(|value| value != info.group)
    {
        if let Some(command) = chown(&input.destination, input.user.as_deref(), input.group.as_deref(), false) {
            commands.push(command);
        }
    }
    if input.mode.is_some_and(|value| value != info.mode) {
        commands.push(chmod(&input.destination, input.mode, false));
    }
    commands
}

fn download_commands(input: &DownloadInput, temp_directory: &str, has_curl: bool, has_wget: bool) -> Vec<Command> {
    let temporary = remote_temporary_filename(temp_directory, input.destination.as_bytes());
    let curl = transfer_command(input, &temporary, true);
    let wget = transfer_command(input, &temporary, false);
    let transfer = if has_curl {
        curl
    } else if has_wget {
        wget
    } else {
        StringCommand::new([raw("("), curl.into(), raw(") || ("), wget.into(), raw(")")])
    };
    let mut commands =
        vec![Command::Shell(transfer), shell([raw("mv"), quoted(&temporary), quoted(&input.destination)])];
    if let Some(command) = chown(&input.destination, input.user.as_deref(), input.group.as_deref(), false) {
        commands.push(command);
    }
    if input.mode.is_some() {
        commands.push(chmod(&input.destination, input.mode, false));
    }
    for (expected, programs, message) in [
        (input.sha1.as_deref(), "sha1sum {0} 2> /dev/null || shasum {0} || sha1 {0}", "SHA1 did not match!"),
        (
            input.sha256.as_deref(),
            "sha256sum {0} 2> /dev/null || shasum -a 256 {0} || sha256 {0}",
            "SHA256 did not match!",
        ),
        (input.sha384.as_deref(), "sha384sum {0} 2> /dev/null || shasum -a 384 {0}", "SHA384 did not match!"),
        (input.md5.as_deref(), "md5sum {0} 2> /dev/null || md5 {0}", "MD5 did not match!"),
    ] {
        if let Some(expected) = expected {
            let path = StringCommand::new([quoted(&input.destination)]).raw_value();
            let programs = programs.replace("{0}", &path);
            commands.push(Command::Shell(StringCommand::new([format!(
                "(( {programs} ) | grep {} ) || ( echo {} && exit 1 )",
                StringCommand::new([CommandBit::quoted(expected)]).raw_value(),
                StringCommand::new([CommandBit::quoted(message)]).raw_value()
            )])));
        }
    }
    commands
}

fn transfer_command(input: &DownloadInput, temporary: &str, curl: bool) -> StringCommand {
    let mut bits = vec![raw(if curl { "curl" } else { "wget" }), raw(if curl { "-sSLf" } else { "-q" })];
    let extra = if curl { &input.extra_curl_args } else { &input.extra_wget_args };
    for (key, value) in extra {
        bits.extend([raw(key), quoted(value)]);
    }
    if let Some(proxy) = &input.proxy {
        if curl {
            bits.extend([raw("--proxy"), quoted(proxy)]);
        } else {
            bits.extend([raw("-e"), raw("use_proxy=yes"), raw("-e"), quoted(&format!("http_proxy={proxy}"))]);
        }
    }
    if input.insecure {
        bits.push(raw(if curl { "--insecure" } else { "--no-check-certificate" }));
    }
    if let Some(rate) = &input.limit_rate {
        bits.extend([raw("--limit-rate"), quoted(rate)]);
    }
    for (key, value) in &input.headers {
        bits.extend([raw("--header"), quoted(&format!("{key}: {value}"))]);
    }
    bits.extend([quoted(&input.source), raw(if curl { "-o" } else { "-O" }), quoted(temporary)]);
    if !curl {
        bits.extend([raw("|| ( rm -f"), quoted(temporary), raw("; exit 1 )")]);
    }
    StringCommand::new(bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(value) => Some(value.raw_value()),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn download_uses_available_client_and_quotes_all_request_data() {
        let mut input = DownloadInput::new("https://example.test/a file", "/tmp/a file");
        input.headers.insert("Authorization".to_owned(), "Bearer a b".to_owned());
        input.sha256 = Some("abc def".to_owned());
        let commands = text(&download_commands(&input, "/tmp", true, false));
        assert!(
            commands[0]
                .starts_with("curl -sSLf --header 'Authorization: Bearer a b' 'https://example.test/a file' -o ")
        );
        assert!(commands[2].contains("grep 'abc def'"));
    }
}
