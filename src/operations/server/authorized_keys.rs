use std::fs;
use std::io::Error;

use crate::api::{Command, CommandBit, ContentUploadCommand, StringCommand};
use crate::connectors::Connector;
use crate::facts::server::{AuthorizedKeys, AuthorizedKeysInput, Home, HomeInput};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::{Directory, DirectoryInput, File, FileInput, Line, LineInput};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserAuthorizedKeysInput {
    pub user: String,
    pub public_keys: Vec<String>,
    pub group: Option<String>,
    pub delete_keys: bool,
    pub authorized_key_directory: Option<String>,
    pub authorized_key_filename: Option<String>,
}

impl UserAuthorizedKeysInput {
    #[must_use]
    pub fn new(user: impl Into<String>, public_keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            user: user.into(),
            public_keys: public_keys.into_iter().map(Into::into).collect(),
            group: None,
            delete_keys: false,
            authorized_key_directory: None,
            authorized_key_filename: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UserAuthorizedKeys;

impl StatefulOperation for UserAuthorizedKeys {
    type Input = UserAuthorizedKeysInput;

    fn name(&self) -> &'static str {
        "server.user_authorized_keys"
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
            let home_input = HomeInput::try_from(input.user.as_str())?;
            let directory = match &input.authorized_key_directory {
                Some(directory) => directory.clone(),
                None => format!(
                    "{}/.ssh",
                    facts.get(&Home, &home_input).await?.ok_or_else(|| format!("home not found for {}", input.user))?
                ),
            };
            let filename = input.authorized_key_filename.as_deref().unwrap_or("authorized_keys");
            let path = format!("{}/{filename}", directory.trim_end_matches('/'));
            let group = input.group.as_deref().unwrap_or(&input.user);
            let public_keys = read_public_keys(&input.public_keys)?;

            let mut directory_input = DirectoryInput::new(&directory);
            directory_input.user = Some(input.user.clone());
            directory_input.group = Some(group.to_owned());
            directory_input.mode = Some(700);
            let mut commands = Directory.commands(&directory_input, connector, options).await?;

            let current =
                facts.get(&AuthorizedKeys, &AuthorizedKeysInput { user: home_input, path: Some(path.clone()) }).await?;
            if input.delete_keys && current != public_keys {
                let content = format!("{}\n", public_keys.join("\n"));
                commands.push(Command::UploadContent(ContentUploadCommand {
                    content: content.into_bytes(),
                    destination: path.clone(),
                    remote_temporary_filename: None,
                }));
                commands.extend(metadata_commands(&path, &input.user, group));
                return Ok(commands);
            }

            let mut file_input = FileInput::new(&path);
            file_input.user = Some(input.user.clone());
            file_input.group = Some(group.to_owned());
            file_input.mode = Some(600);
            commands.extend(File.commands(&file_input, connector, options).await?);
            if !input.delete_keys {
                for key in public_keys.iter().filter(|key| !current.contains(key)) {
                    let mut line_input = LineInput::new(&path, key);
                    line_input.ensure_newline = true;
                    commands.extend(Line.commands(&line_input, connector, options).await?);
                }
            }
            Ok(commands)
        })
    }
}

fn read_public_keys(values: &[String]) -> Result<Vec<String>, Error> {
    let mut keys = Vec::new();
    for value in values {
        match fs::metadata(value) {
            Ok(_) => keys.extend(fs::read_to_string(value)?.lines().map(str::trim).map(str::to_owned)),
            Err(_) => keys.push(value.trim().to_owned()),
        }
    }
    Ok(keys)
}

fn metadata_commands(path: &str, user: &str, group: &str) -> [Command; 2] {
    [
        shell([raw("chown"), quoted(format!("{user}:{group}")), quoted(path)]),
        shell([raw("chmod"), raw("600"), quoted(path)]),
    ]
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: impl Into<String>) -> CommandBit {
    CommandBit::quoted(value.into())
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use super::read_public_keys;

    #[test]
    fn inline_keys_are_trimmed_without_treating_them_as_required_files() {
        assert_eq!(read_public_keys(&["  ssh-ed25519 AAA test  ".to_owned()]).unwrap(), ["ssh-ed25519 AAA test"]);
    }
}
