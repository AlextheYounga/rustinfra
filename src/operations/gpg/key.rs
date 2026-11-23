use std::collections::BTreeSet;
use std::error::Error;

use indexmap::IndexMap;
use thiserror::Error;

use crate::api::{Command, StringCommand, remote_temporary_filename};
use crate::connectors::Connector;
use crate::facts::files::{File as FileFact, PathInfo, PathInput};
use crate::facts::gpg::{GpgKeyInfo, GpgKeyringInfo, GpgKeyrings};
use crate::facts::{FactExecutor, FactOptions};

use super::{ensure_directory, ensure_file, parent, quoted, raw, shell};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyInput {
    pub source: Option<String>,
    pub destination: Option<String>,
    pub keyserver: Option<String>,
    pub key_ids: Vec<String>,
    pub dearmor: bool,
    pub mode: u16,
    pub present: bool,
    pub working_directories: Vec<String>,
}

impl Default for KeyInput {
    fn default() -> Self {
        Self {
            source: None,
            destination: None,
            keyserver: None,
            key_ids: Vec::new(),
            dearmor: true,
            mode: 644,
            present: true,
            working_directories: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Key;

impl StatefulOperation for Key {
    type Input = KeyInput;
    fn name(&self) -> &'static str {
        "gpg.key"
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
            validate(input)?;
            if input.present {
                install_commands(input, connector, options).await
            } else {
                removal_commands(input, connector, options).await
            }
        })
    }
}

fn validate(input: &KeyInput) -> Result<(), GpgOperationError> {
    if input.present {
        if input.source.is_none() && input.keyserver.is_none() {
            return Err(GpgOperationError::MissingSource);
        }
        if input.keyserver.is_some() && input.key_ids.is_empty() {
            return Err(GpgOperationError::MissingKeyId);
        }
        if input.destination.is_none() {
            return Err(GpgOperationError::MissingDestination);
        }
    } else if input.destination.is_none() && (input.key_ids.is_empty() || input.working_directories.is_empty()) {
        return Err(GpgOperationError::InvalidRemoval);
    }
    Ok(())
}

async fn install_commands(
    input: &KeyInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let destination = input.destination.as_deref().ok_or(GpgOperationError::MissingDestination)?;
    let facts = FactExecutor::new(connector, options);
    if !input.key_ids.is_empty() {
        let keyrings = facts.get(&GpgKeyrings, &vec![parent(destination).to_owned()]).await?;
        if keyrings.get(destination).is_some_and(|keyring| {
            input.key_ids.iter().all(|key| keyring.keys.keys().any(|existing| key_matches(existing, key)))
        }) {
            return ensure_file(destination, input.mode, connector, options).await;
        }
    } else if matches!(facts.get(&FileFact, &PathInput::from(destination)).await?, PathInfo::Present(_)) {
        return ensure_file(destination, input.mode, connector, options).await;
    }

    let mut commands = ensure_directory(parent(destination), 755, connector, options).await?;
    if let Some(source) = &input.source {
        commands.extend(source_commands(input, source, destination, options));
    }
    if input.keyserver.is_some() {
        commands.extend(keyserver_commands(input, destination, options, connector).await?);
    }
    commands.extend(ensure_file(destination, input.mode, connector, options).await?);
    Ok(commands)
}

fn source_commands(input: &KeyInput, source: &str, destination: &str, options: &FactOptions) -> Vec<Command> {
    let is_url = source.starts_with("http://") || source.starts_with("https://");
    let temporary = remote_temporary_filename(&options.run.remote_temp_directory, source.as_bytes());
    let source_path = if is_url { temporary.as_str() } else { source };
    let mut commands = Vec::new();
    if is_url {
        let download = format!("{temporary}.download");
        commands.extend([
            shell([raw("curl"), raw("-sSLf"), quoted(source), raw("-o"), quoted(&download)]),
            shell([raw("mv"), quoted(download), quoted(&temporary)]),
        ]);
    }
    commands.push(if input.dearmor {
        shell([raw(format!(
            "if grep -q \"BEGIN PGP PUBLIC KEY BLOCK\" {}; then gpg --batch --dearmor -o {} {}; else cp {} {}; fi",
            double(source_path),
            double(destination),
            double(source_path),
            double(source_path),
            double(destination)
        ))])
    } else {
        shell([raw(format!("cp {} {}", double(source_path), double(destination)))])
    });
    if is_url {
        commands.push(shell([raw("rm"), raw("-f"), quoted(temporary)]));
    }
    commands
}

async fn keyserver_commands(
    input: &KeyInput,
    destination: &str,
    options: &FactOptions,
    connector: &dyn Connector,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let keyserver = input.keyserver.as_deref().ok_or(GpgOperationError::MissingSource)?;
    let joined = input.key_ids.join(" ");
    let temporary =
        remote_temporary_filename(&options.run.remote_temp_directory, format!("gpg-keyserver-{joined}").as_bytes());
    let mut commands = ensure_directory(&temporary, 700, connector, options).await?;
    let key_ids =
        input.key_ids.iter().map(|key| StringCommand::new([quoted(key)]).raw_value()).collect::<Vec<_>>().join(" ");
    commands.extend([
        shell([raw(format!(
            "export GNUPGHOME={} && gpg --batch --keyserver {} --recv-keys {key_ids}",
            double(&temporary),
            double(keyserver)
        ))]),
        shell([raw(format!(
            "export GNUPGHOME={} && gpg --batch --export {key_ids} > {}",
            double(&temporary),
            double(destination)
        ))]),
    ]);
    Ok(commands)
}

async fn removal_commands(
    input: &KeyInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    if input.key_ids.is_empty() {
        let destination = input.destination.as_deref().ok_or(GpgOperationError::InvalidRemoval)?;
        return match FactExecutor::new(connector, options).get(&FileFact, &PathInput::from(destination)).await? {
            PathInfo::Present(_) => Ok(vec![shell([raw("rm"), raw("-f"), quoted(destination)])]),
            PathInfo::Missing => Ok(Vec::new()),
            PathInfo::DifferentType => Err(GpgOperationError::NotAFile(destination.to_owned()).into()),
        };
    }
    let directories = input
        .destination
        .as_deref()
        .map_or_else(|| input.working_directories.clone(), |destination| vec![parent(destination).to_owned()]);
    let keyrings = FactExecutor::new(connector, options).get(&GpgKeyrings, &directories).await?;
    let selected = input.destination.as_deref().map_or_else(
        || keyrings.iter().collect::<Vec<_>>(),
        |destination| keyrings.get_key_value(destination).into_iter().collect(),
    );
    selected.into_iter().try_fold(Vec::new(), |mut commands, (path, keyring)| {
        commands.extend(remove_from_keyring(path, keyring, &input.key_ids)?);
        Ok(commands)
    })
}

fn remove_from_keyring(
    path: &str,
    keyring: &GpgKeyringInfo,
    key_ids: &[String],
) -> Result<Vec<Command>, GpgOperationError> {
    let fingerprints = matching_fingerprints(&keyring.keys, key_ids);
    if fingerprints.is_empty() {
        return Ok(Vec::new());
    }
    if keyring.format == "asc" {
        if fingerprints.len() < keyring.keys.len() {
            return Err(GpgOperationError::PartialArmoredRemoval(path.to_owned()));
        }
        return Ok(vec![shell([raw("rm"), raw("-f"), quoted(path)])]);
    }
    let joined =
        fingerprints.iter().map(|value| StringCommand::new([quoted(value)]).raw_value()).collect::<Vec<_>>().join(" ");
    Ok(vec![
        shell([raw(format!(
            "gpg --batch --yes --no-default-keyring --keyring {} --delete-keys {joined}",
            double(path)
        ))]),
        shell([raw(format!(
            "gpg --batch --no-default-keyring --keyring {} --list-keys 2>/dev/null | grep -q \"^pub\" || rm -f {}",
            double(path),
            double(path)
        ))]),
    ])
}

fn matching_fingerprints(keys: &IndexMap<String, GpgKeyInfo>, requested: &[String]) -> BTreeSet<String> {
    requested
        .iter()
        .flat_map(|requested| {
            keys.iter().filter_map(move |(id, info)| {
                key_matches(id, requested).then(|| info.fingerprint.clone().unwrap_or_else(|| id.clone()))
            })
        })
        .collect()
}

fn key_matches(existing: &str, requested: &str) -> bool {
    let requested = requested.trim_start_matches("0x").trim_start_matches("0X").to_ascii_uppercase();
    let existing = existing.to_ascii_uppercase();
    requested == existing
        || existing.ends_with(&requested)
        || existing.starts_with(&requested)
        || requested.ends_with(&existing)
}

fn double(value: &str) -> String {
    format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace(char::from(96), "\\\u{60}")
    )
}

#[derive(Debug, Error)]
enum GpgOperationError {
    #[error("Either \u{60}src\u{60} or \u{60}keyserver\u{60} must be provided for installation")]
    MissingSource,
    #[error("\u{60}keyid\u{60} must be provided with \u{60}keyserver\u{60}")]
    MissingKeyId,
    #[error("\u{60}dest\u{60} must be provided for installation")]
    MissingDestination,
    #[error(
        "For removal, either \u{60}dest\u{60} or both \u{60}keyid\u{60} and \u{60}working_dirs\u{60} must be provided"
    )]
    InvalidRemoval,
    #[error("destination {0} exists and is not a file")]
    NotAFile(String),
    #[error(
        "Cannot remove individual keys from armored keyring {0:?}: the file contains other keys and cannot be edited in-place. Remove the file explicitly with present=False and no keyid."
    )]
    PartialArmoredRemoval(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fingerprint: &str) -> GpgKeyInfo {
        GpgKeyInfo {
            validity: "-".to_owned(),
            length: 4096,
            fingerprint: Some(fingerprint.to_owned()),
            uid_hash: None,
            uid: None,
            subkeys: IndexMap::new(),
        }
    }

    fn raw_commands(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some(command.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn key_ids_match_short_long_and_full_fingerprint_forms() {
        assert!(key_matches("8D81803C0EBFCD88", "0x0EBFCD88"));
        assert!(key_matches("8D81803C0EBFCD88", "9DC858229FC7DD38854AE2D88D81803C0EBFCD88"));
    }

    #[test]
    fn removal_uses_fingerprints_and_rejects_partial_armored_files() {
        let keys: IndexMap<String, GpgKeyInfo> =
            [("ABCDEF1234567890".to_owned(), key("FULLABCDEF")), ("111111111111AAAA".to_owned(), key("FULL111111"))]
                .into_iter()
                .collect();
        let binary = GpgKeyringInfo { format: "gpg".to_owned(), keys: keys.clone() };
        let commands = remove_from_keyring("/keys/vendor.gpg", &binary, &["0xABCDEF12".to_owned()]);
        assert!(matches!(commands, Ok(commands) if raw_commands(&commands)[0].ends_with("--delete-keys FULLABCDEF")));
        let armored = GpgKeyringInfo { format: "asc".to_owned(), keys };
        assert!(remove_from_keyring("/keys/vendor.asc", &armored, &["0xABCDEF12".to_owned()]).is_err());
    }

    #[test]
    fn source_install_quotes_paths_and_switches_dearmor_behavior() {
        let input = KeyInput {
            source: Some("/tmp/key file.asc".to_owned()),
            destination: Some("/keys/key file.gpg".to_owned()),
            ..KeyInput::default()
        };
        let commands =
            raw_commands(&source_commands(&input, "/tmp/key file.asc", "/keys/key file.gpg", &FactOptions::default()));
        assert!(commands[0].contains("gpg --batch --dearmor -o \"/keys/key file.gpg\" \"/tmp/key file.asc\""));
    }
}
