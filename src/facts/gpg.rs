use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpgKeyInfo {
    pub validity: String,
    pub length: u32,
    pub fingerprint: Option<String>,
    pub uid_hash: Option<String>,
    pub uid: Option<String>,
    pub subkeys: IndexMap<String, GpgSubkeyInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpgSubkeyInfo {
    pub validity: String,
    pub length: u32,
    pub fingerprint: Option<String>,
    pub uid_hash: Option<String>,
    pub uid: Option<String>,
}

#[derive(Clone, Debug)]
struct PendingKey {
    id: String,
    info: GpgKeyInfo,
}

#[derive(Clone, Debug)]
struct PendingSubkey {
    id: String,
    info: GpgSubkeyInfo,
}

fn parse_keys(
    output: &[String],
    key_type: &str,
    subkey_type: &str,
) -> Result<IndexMap<String, GpgKeyInfo>, Box<dyn Error + Send + Sync>> {
    let mut keys = IndexMap::new();
    let mut current_key = None::<PendingKey>;
    let mut current_subkey = None::<PendingSubkey>;
    for line in output.iter().filter(|line| !line.is_empty()) {
        let fields = line.split(':').collect::<Vec<_>>();
        if fields.first().is_some_and(|record| *record == key_type || *record == subkey_type) && fields.len() > 4 {
            if fields[0] == key_type {
                finish_key(&mut keys, &mut current_key, &mut current_subkey);
                current_key = Some(PendingKey {
                    id: fields[4].to_owned(),
                    info: GpgKeyInfo {
                        validity: fields[1].to_owned(),
                        length: fields[2].parse()?,
                        fingerprint: None,
                        uid_hash: None,
                        uid: None,
                        subkeys: IndexMap::new(),
                    },
                });
            } else if current_key.is_some() {
                finish_subkey(&mut current_key, &mut current_subkey);
                current_subkey = Some(PendingSubkey {
                    id: fields[4].to_owned(),
                    info: GpgSubkeyInfo {
                        validity: fields[1].to_owned(),
                        length: fields[2].parse()?,
                        fingerprint: None,
                        uid_hash: None,
                        uid: None,
                    },
                });
            }
        } else if fields.len() > 9 {
            if fields[0] == "fpr" {
                if let Some(subkey) = &mut current_subkey {
                    subkey.info.fingerprint = Some(fields[9].to_owned());
                } else if let Some(key) = &mut current_key {
                    key.info.fingerprint = Some(fields[9].to_owned());
                }
            } else if fields[0] == "uid" {
                if let Some(subkey) = &mut current_subkey {
                    subkey.info.uid_hash = Some(fields[7].to_owned());
                    subkey.info.uid = Some(fields[9].to_owned());
                } else if let Some(key) = &mut current_key {
                    key.info.uid_hash = Some(fields[7].to_owned());
                    key.info.uid = Some(fields[9].to_owned());
                }
            }
        }
    }
    finish_key(&mut keys, &mut current_key, &mut current_subkey);
    Ok(keys)
}

fn finish_subkey(key: &mut Option<PendingKey>, subkey: &mut Option<PendingSubkey>) {
    if let (Some(key), Some(subkey)) = (key, subkey.take()) {
        key.info.subkeys.insert(subkey.id, subkey.info);
    }
}

fn finish_key(
    keys: &mut IndexMap<String, GpgKeyInfo>,
    key: &mut Option<PendingKey>,
    subkey: &mut Option<PendingSubkey>,
) {
    finish_subkey(key, subkey);
    if let Some(key) = key.take() {
        keys.insert(key.id, key.info);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpgKey;

impl Fact for GpgKey {
    type Input = String;
    type Output = IndexMap<String, GpgKeyInfo>;
    fn name(&self) -> &'static str {
        "gpg.GpgKey"
    }
    fn command(&self, source: &Self::Input) -> StringCommand {
        let source = quote(source);
        if source.split_once(':').is_some_and(|(scheme, remainder)| !scheme.is_empty() && remainder.starts_with("//")) {
            StringCommand::new([format!("(wget -O - {source} || curl -sSLf {source}) | gpg --with-colons")])
        } else {
            StringCommand::new([format!("gpg --with-colons {source}")])
        }
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("gpg")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        parse_keys(output, "pub", "sub")
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpgKeys;

impl Fact for GpgKeys {
    type Input = Option<String>;
    type Output = IndexMap<String, GpgKeyInfo>;
    fn name(&self) -> &'static str {
        "gpg.GpgKeys"
    }
    fn command(&self, keyring: &Self::Input) -> StringCommand {
        list_command("--list-keys", keyring.as_deref())
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("gpg")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        parse_keys(output, "pub", "sub")
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpgSecretKeys;

impl Fact for GpgSecretKeys {
    type Input = Option<String>;
    type Output = IndexMap<String, GpgKeyInfo>;
    fn name(&self) -> &'static str {
        "gpg.GpgSecretKeys"
    }
    fn command(&self, keyring: &Self::Input) -> StringCommand {
        list_command("--list-secret-keys", keyring.as_deref())
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("gpg")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        parse_keys(output, "sec", "ssb")
    }
}

fn list_command(action: &str, keyring: Option<&str>) -> StringCommand {
    keyring.map_or_else(
        || StringCommand::new([format!("gpg {action} --with-colons")]),
        |keyring| {
            StringCommand::new([format!(
                "gpg {action} --with-colons --keyring {} --no-default-keyring",
                quote(keyring)
            )])
        },
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpgKeyringInfo {
    pub format: String,
    pub keys: IndexMap<String, GpgKeyInfo>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpgKeyrings;

impl Fact for GpgKeyrings {
    type Input = Vec<String>;
    type Output = IndexMap<String, GpgKeyringInfo>;
    fn name(&self) -> &'static str {
        "gpg.GpgKeyrings"
    }
    fn command(&self, directories: &Self::Input) -> StringCommand {
        let locations = directories.iter().map(|value| double_quote(value)).collect::<Vec<_>>().join(" ");
        StringCommand::new([format!(
            "find {locations} -type f \\( -name '*.gpg' -o -name '*.kbx' \\) 2>/dev/null -exec sh -c 'echo \"KEYRING:$1\"; gpg --list-keys --with-colons --keyring \"$1\" --no-default-keyring 2>/dev/null || true' _ {{}} \\; ; find {locations} -type f -name '*.asc' 2>/dev/null -exec sh -c 'echo \"KEYRING:$1\"; gpg --with-colons \"$1\" 2>/dev/null || true' _ {{}} \\;"
        )])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("gpg")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut keyrings = IndexMap::new();
        let mut path = None::<String>;
        let mut lines = Vec::new();
        for line in output.iter().map(|line| line.trim()).filter(|line| !line.is_empty()) {
            if let Some(next) = line.strip_prefix("KEYRING:") {
                finish_keyring(&mut keyrings, &mut path, &mut lines)?;
                path = Some(next.to_owned());
            } else {
                lines.push(line.to_owned());
            }
        }
        finish_keyring(&mut keyrings, &mut path, &mut lines)?;
        Ok(keyrings)
    }
}

fn finish_keyring(
    keyrings: &mut IndexMap<String, GpgKeyringInfo>,
    path: &mut Option<String>,
    lines: &mut Vec<String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(path) = path.take().filter(|_| !lines.is_empty()) {
        let format = path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
        keyrings.insert(path, GpgKeyringInfo { format, keys: parse_keys(lines, "pub", "sub")? });
    }
    lines.clear();
    Ok(())
}

fn quote(value: &str) -> String {
    StringCommand::new([CommandBit::quoted(value)]).raw_value()
}
fn double_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`"))
}

#[cfg(test)]
mod tests {
    use super::{Fact, GpgKey, GpgKeyrings, GpgKeys};

    #[test]
    fn parses_primary_keys_subkeys_fingerprints_and_uids() {
        let output = [
            "pub:-:4096:1:ABC::::::".to_owned(),
            "fpr:::::::::FULLABC:".to_owned(),
            "uid:::::::HASH::User <u@example.com>:".to_owned(),
            "sub:-:2048:1:SUB::::::".to_owned(),
        ];
        let keys = GpgKeys.process(&output, &None).unwrap();
        assert_eq!(keys["ABC"].fingerprint.as_deref(), Some("FULLABC"));
        assert_eq!(keys["ABC"].uid.as_deref(), Some("User <u@example.com>"));
        assert_eq!(keys["ABC"].subkeys["SUB"].length, 2048);
    }

    #[test]
    fn commands_quote_sources_keyrings_and_search_directories() {
        assert_eq!(GpgKey.command(&"/some/key/file".to_owned()).raw_value(), "gpg --with-colons /some/key/file");
        assert_eq!(
            GpgKeys.command(&Some("/some/keyring".to_owned())).raw_value(),
            "gpg --list-keys --with-colons --keyring /some/keyring --no-default-keyring"
        );
        assert!(
            GpgKeyrings
                .command(&vec!["/etc/apt/keyrings".to_owned()])
                .raw_value()
                .starts_with("find \"/etc/apt/keyrings\"")
        );
    }
}
