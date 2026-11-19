use std::env;
use std::path::PathBuf;
use std::time::Duration;

use indexmap::IndexMap;
use serde_json::Value;
use thiserror::Error;

use crate::api::{Data, HiddenValue, SSH_HOSTNAME_DATA_KEY};

use super::config;

/// OpenSSH-compatible handling for unknown server keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostKeyPolicy {
    Ask,
    Accept,
    Strict,
    AcceptNew,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileTransferProtocol {
    Sftp,
    Scp,
}

/// Typed SSH connector data after inventory precedence has been applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshConnectionOptions {
    pub hostname: String,
    pub port: u16,
    pub user: String,
    pub password: Option<HiddenValue>,
    pub key: Option<PathBuf>,
    pub config_identity_files: Vec<PathBuf>,
    pub key_password: Option<HiddenValue>,
    pub allow_agent: bool,
    pub identity_agent: Option<PathBuf>,
    pub look_for_keys: bool,
    pub known_hosts_files: Vec<PathBuf>,
    pub host_key_policy: HostKeyPolicy,
    pub connect_timeout: Duration,
    pub server_alive_interval: Option<Duration>,
    pub connect_retries: usize,
    pub connect_retry_min_delay: Duration,
    pub connect_retry_max_delay: Duration,
    pub file_transfer_protocol: FileTransferProtocol,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum SshOptionsError {
    #[error("invalid SSH data `{key}`: expected {expected}")]
    InvalidType { key: &'static str, expected: &'static str },
    #[error("invalid SSH port: {0}")]
    InvalidPort(u64),
    #[error("invalid SSH strict host key checking value: {0}")]
    InvalidHostKeyPolicy(String),
    #[error("invalid SSH retry delay range: minimum {minimum} exceeds maximum {maximum}")]
    InvalidRetryDelayRange { minimum: f64, maximum: f64 },
    #[error("unsupported SSH file transfer protocol: {0}")]
    InvalidFileTransferProtocol(String),
    #[error("no SSH user configured and USER is not set")]
    MissingUser,
    #[error("SSH config error: {0}")]
    Config(String),
}

impl SshConnectionOptions {
    pub fn from_data(host_name: &str, data: &Data, connect_timeout: Duration) -> Result<Self, SshOptionsError> {
        let alias = optional_string(data, SSH_HOSTNAME_DATA_KEY)?.unwrap_or(host_name);
        let config_file = optional_string(data, "ssh_config_file")?;
        let resolved = config::resolve(alias, config_file).map_err(SshOptionsError::Config)?;
        let hostname = resolved.as_ref().and_then(|params| params.host_name.as_deref()).unwrap_or(alias).to_owned();
        let port_value = optional_u64(data, "ssh_port")?
            .or_else(|| resolved.as_ref().and_then(|params| params.port.map(u64::from)))
            .unwrap_or(22);
        let port = u16::try_from(port_value).map_err(|_| SshOptionsError::InvalidPort(port_value))?;
        let user = match optional_string(data, "ssh_user")?.or_else(|| resolved.as_ref()?.user.as_deref()) {
            Some(user) if !user.is_empty() => user.to_owned(),
            _ => env::var("USER").map_err(|_| SshOptionsError::MissingUser)?,
        };
        let policy = optional_string(data, "ssh_strict_host_key_checking")?
            .or_else(|| unsupported_value(resolved.as_ref(), "stricthostkeychecking"))
            .unwrap_or("accept-new");
        let host_key_policy = match policy {
            "ask" => HostKeyPolicy::Ask,
            "no" | "off" => HostKeyPolicy::Accept,
            "yes" => HostKeyPolicy::Strict,
            "accept-new" => HostKeyPolicy::AcceptNew,
            value => return Err(SshOptionsError::InvalidHostKeyPolicy(value.to_owned())),
        };

        let retry_min = optional_f64(data, "ssh_connect_retry_min_delay")?.unwrap_or(0.1);
        let retry_max = optional_f64(data, "ssh_connect_retry_max_delay")?.unwrap_or(0.5);
        if retry_min > retry_max {
            return Err(SshOptionsError::InvalidRetryDelayRange { minimum: retry_min, maximum: retry_max });
        }
        let retry_min = duration_from_seconds("ssh_connect_retry_min_delay", retry_min)?;
        let retry_max = duration_from_seconds("ssh_connect_retry_max_delay", retry_max)?;
        let file_transfer_protocol = match optional_string(data, "ssh_file_transfer_protocol")?.unwrap_or("sftp") {
            "sftp" => FileTransferProtocol::Sftp,
            "scp" => FileTransferProtocol::Scp,
            value => return Err(SshOptionsError::InvalidFileTransferProtocol(value.to_owned())),
        };

        let key = optional_string(data, "ssh_key")?.filter(|value| !value.is_empty()).map(PathBuf::from);
        let config_identity_files = if key.is_none() {
            resolved.as_ref().and_then(|params| params.identity_file.clone()).unwrap_or_default()
        } else {
            Vec::new()
        };
        let known_hosts_files = match optional_string(data, "ssh_known_hosts_file")?.filter(|value| !value.is_empty()) {
            Some(path) => vec![config::expand_user(path)],
            None => unsupported_values(resolved.as_ref(), "userknownhostsfile")
                .into_iter()
                .map(config::expand_user)
                .collect(),
        };
        let identity_agent = unsupported_value(resolved.as_ref(), "identityagent")
            .filter(|path| !path.eq_ignore_ascii_case("none"))
            .map(config::expand_user);

        Ok(Self {
            hostname,
            port,
            user,
            password: optional_secret(data, "ssh_password")?,
            key,
            config_identity_files,
            key_password: optional_secret(data, "ssh_key_password")?,
            allow_agent: optional_bool(data, "ssh_allow_agent")?.unwrap_or(true),
            identity_agent,
            look_for_keys: optional_bool(data, "ssh_look_for_keys")?.unwrap_or(true),
            known_hosts_files,
            host_key_policy,
            connect_timeout,
            server_alive_interval: resolved.as_ref().and_then(|params| params.server_alive_interval),
            connect_retries: usize::try_from(optional_u64(data, "ssh_connect_retries")?.unwrap_or(0)).map_err(
                |_| SshOptionsError::InvalidType {
                    key: "ssh_connect_retries",
                    expected: "a non-negative integer supported by this platform",
                },
            )?,
            connect_retry_min_delay: retry_min,
            connect_retry_max_delay: retry_max,
            file_transfer_protocol,
        })
    }
}

fn optional_string<'a>(
    data: &'a IndexMap<String, Value>,
    key: &'static str,
) -> Result<Option<&'a str>, SshOptionsError> {
    match data.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(SshOptionsError::InvalidType { key, expected: "a string" }),
    }
}

fn optional_bool(data: &IndexMap<String, Value>, key: &'static str) -> Result<Option<bool>, SshOptionsError> {
    match data.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(SshOptionsError::InvalidType { key, expected: "a boolean" }),
    }
}

fn optional_u64(data: &IndexMap<String, Value>, key: &'static str) -> Result<Option<u64>, SshOptionsError> {
    match data.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => {
            value.as_u64().map(Some).ok_or(SshOptionsError::InvalidType { key, expected: "a non-negative integer" })
        }
        Some(_) => Err(SshOptionsError::InvalidType { key, expected: "a non-negative integer" }),
    }
}

fn optional_f64(data: &IndexMap<String, Value>, key: &'static str) -> Result<Option<f64>, SshOptionsError> {
    match data.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => match value.as_f64() {
            Some(value) if value.is_finite() && value >= 0.0 => Ok(Some(value)),
            _ => Err(SshOptionsError::InvalidType { key, expected: "a non-negative finite number" }),
        },
        Some(_) => Err(SshOptionsError::InvalidType { key, expected: "a non-negative finite number" }),
    }
}

fn optional_secret(data: &IndexMap<String, Value>, key: &'static str) -> Result<Option<HiddenValue>, SshOptionsError> {
    Ok(optional_string(data, key)?.filter(|value| !value.is_empty()).map(HiddenValue::new))
}

fn duration_from_seconds(key: &'static str, seconds: f64) -> Result<Duration, SshOptionsError> {
    Duration::try_from_secs_f64(seconds)
        .map_err(|_| SshOptionsError::InvalidType { key, expected: "a duration supported by this platform" })
}

fn unsupported_values<'a>(params: Option<&'a ssh2_config::HostParams>, key: &str) -> Vec<&'a str> {
    params
        .and_then(|params| params.unsupported_fields.get(key))
        .map(|values| values.iter().map(String::as_str).collect())
        .unwrap_or_default()
}

fn unsupported_value<'a>(params: Option<&'a ssh2_config::HostParams>, key: &str) -> Option<&'a str> {
    unsupported_values(params, key).into_iter().next()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{HostKeyPolicy, SshConnectionOptions, SshOptionsError};
    use crate::api::Data;

    #[test]
    fn inventory_data_overrides_ssh_defaults_and_secrets_stay_masked() {
        let data = [
            ("ssh_hostname".to_owned(), json!("target")),
            ("ssh_port".to_owned(), json!(2202)),
            ("ssh_user".to_owned(), json!("deploy")),
            ("ssh_password".to_owned(), json!("secret")),
            ("ssh_allow_agent".to_owned(), json!(false)),
            ("ssh_strict_host_key_checking".to_owned(), json!("yes")),
        ]
        .into_iter()
        .collect::<Data>();

        let options = SshConnectionOptions::from_data("alias", &data, Duration::from_secs(7)).unwrap();

        assert_eq!(options.hostname, "target");
        assert_eq!(options.port, 2202);
        assert_eq!(options.user, "deploy");
        assert_eq!(options.password.as_ref().unwrap().unmask(), "secret");
        assert_eq!(format!("{:?}", options.password), "Some(HiddenValue(\"*MASKED*\"))");
        assert!(!options.allow_agent);
        assert_eq!(options.host_key_policy, HostKeyPolicy::Strict);
        assert_eq!(options.connect_timeout, Duration::from_secs(7));
    }

    #[test]
    fn invalid_policy_is_rejected_before_connecting() {
        let data =
            [("ssh_user".to_owned(), json!("deploy")), ("ssh_strict_host_key_checking".to_owned(), json!("sometimes"))]
                .into_iter()
                .collect::<Data>();

        let error = SshConnectionOptions::from_data("host", &data, Duration::from_secs(10)).unwrap_err();

        assert_eq!(error, SshOptionsError::InvalidHostKeyPolicy("sometimes".to_owned()));
    }

    #[test]
    fn ssh_config_resolves_alias_and_inventory_values_override_connection_fields() {
        let directory = tempdir().unwrap();
        let config = directory.path().join("config");
        fs::write(
            &config,
            "Host alias\n  HostName config-host\n  User config-user\n  Port 2202\n  IdentityFile /config/key-one\n  IdentityFile /config/key-two\n  ServerAliveInterval 15\n  StrictHostKeyChecking yes\n  UserKnownHostsFile ~/known-one ~/known-two\n  IdentityAgent ~/agent.sock\n",
        )
        .unwrap();
        let data = [
            ("ssh_config_file".to_owned(), json!(config)),
            ("ssh_user".to_owned(), json!("inventory-user")),
            ("ssh_port".to_owned(), json!(2222)),
            ("ssh_key".to_owned(), json!("/inventory/key")),
        ]
        .into_iter()
        .collect::<Data>();

        let options = SshConnectionOptions::from_data("alias", &data, Duration::from_secs(7)).unwrap();

        assert_eq!(options.hostname, "config-host");
        assert_eq!(options.user, "inventory-user");
        assert_eq!(options.port, 2222);
        assert_eq!(options.key, Some("/inventory/key".into()));
        assert!(options.config_identity_files.is_empty());
        assert_eq!(options.server_alive_interval, Some(Duration::from_secs(15)));
        assert_eq!(options.host_key_policy, HostKeyPolicy::Strict);
        assert_eq!(options.known_hosts_files.len(), 2);
        assert!(options.known_hosts_files[0].ends_with("known-one"));
        assert!(options.identity_agent.as_ref().is_some_and(|path| path.ends_with("agent.sock")));
    }
}
