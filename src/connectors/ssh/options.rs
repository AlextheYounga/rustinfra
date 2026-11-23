use std::env;
use std::path::PathBuf;
use std::time::Duration;

use indexmap::IndexMap;
use serde_json::Value;
use thiserror::Error;

use crate::api::{Data, HiddenValue, SSH_HOSTNAME_DATA_KEY, USER_NAME_IDENTIFIER};

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
    pub host_alias: String,
    pub hostname: String,
    pub port: u16,
    pub user: String,
    pub password: Option<HiddenValue>,
    pub key: Option<PathBuf>,
    pub config_identity_files: Vec<PathBuf>,
    pub config_certificate_file: Option<PathBuf>,
    pub key_password: Option<HiddenValue>,
    pub allow_agent: bool,
    pub identity_agent: Option<PathBuf>,
    pub look_for_keys: bool,
    pub forward_agent: bool,
    pub known_hosts_files: Vec<PathBuf>,
    pub host_key_policy: HostKeyPolicy,
    pub connect_timeout: Duration,
    pub server_alive_interval: Option<Duration>,
    pub connect_retries: usize,
    pub connect_retry_min_delay: Duration,
    pub connect_retry_max_delay: Duration,
    pub file_transfer_protocol: FileTransferProtocol,
    pub proxy_command: Option<Vec<String>>,
    pub proxy_jump: Vec<Self>,
    pub proxy_jump_channel_timeout: Duration,
    pub config_file: Option<PathBuf>,
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
    #[error("invalid ProxyJump target: {0}")]
    InvalidProxyJump(String),
    #[error("ProxyJump loop detected through: {0}")]
    ProxyJumpLoop(String),
}

impl SshConnectionOptions {
    pub fn from_data(host_name: &str, data: &Data, connect_timeout: Duration) -> Result<Self, SshOptionsError> {
        Self::build(BuildRequest { host_name, data, connect_timeout, use_config_timeout: false }, &mut Vec::new())
    }

    fn build(request: BuildRequest<'_>, stack: &mut Vec<String>) -> Result<Self, SshOptionsError> {
        let data = request.data;
        let alias = optional_string(data, SSH_HOSTNAME_DATA_KEY)?.unwrap_or(request.host_name);
        if stack.iter().any(|host| host == alias) {
            return Err(SshOptionsError::ProxyJumpLoop(alias.to_owned()));
        }
        stack.push(alias.to_owned());
        let config_file = optional_string(data, "ssh_config_file")?;
        let stored_config_file = config_file.filter(|path| !path.is_empty()).map(config::expand_user);
        let resolved = config::resolve(alias, config_file).map_err(SshOptionsError::Config)?;
        let hostname = resolved.as_ref().and_then(|params| params.host_name.as_deref()).unwrap_or(alias).to_owned();
        let port_value = optional_u64(data, "ssh_port")?
            .or_else(|| resolved.as_ref().and_then(|params| params.port.map(u64::from)))
            .unwrap_or(22);
        let port = u16::try_from(port_value).map_err(|_| SshOptionsError::InvalidPort(port_value))?;
        let user = match optional_string(data, "ssh_user")?.or_else(|| resolved.as_ref()?.user.as_deref()) {
            Some(user) if !user.is_empty() => user.to_owned(),
            _ => env::var(USER_NAME_IDENTIFIER).map_err(|_| SshOptionsError::MissingUser)?,
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
        let config_certificate_file =
            if key.is_none() { resolved.as_ref().and_then(|params| params.certificate_file.clone()) } else { None };
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

        let connect_timeout = if request.use_config_timeout {
            resolved.as_ref().and_then(|params| params.connect_timeout).unwrap_or(request.connect_timeout)
        } else {
            request.connect_timeout
        };
        let proxy_command =
            unsupported_values(resolved.as_ref(), "proxycommand").into_iter().map(str::to_owned).collect::<Vec<_>>();
        let proxy_command = (!proxy_command.is_empty()).then_some(proxy_command);
        let proxy_specs = if proxy_command.is_none() {
            resolved.as_ref().and_then(|params| params.proxy_jump.clone()).unwrap_or_default()
        } else {
            Vec::new()
        };

        let mut options = Self {
            host_alias: alias.to_owned(),
            hostname,
            port,
            user,
            password: optional_secret(data, "ssh_password")?,
            key,
            config_identity_files,
            config_certificate_file,
            key_password: optional_secret(data, "ssh_key_password")?,
            allow_agent: optional_bool(data, "ssh_allow_agent")?.unwrap_or(true),
            identity_agent,
            look_for_keys: optional_bool(data, "ssh_look_for_keys")?.unwrap_or(true),
            forward_agent: optional_bool(data, "ssh_forward_agent")?
                .or_else(|| resolved.as_ref().and_then(|params| params.forward_agent))
                .unwrap_or(false),
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
            proxy_command,
            proxy_jump: Vec::new(),
            proxy_jump_channel_timeout: connect_timeout,
            config_file: stored_config_file,
        };
        for spec in proxy_specs {
            if spec.eq_ignore_ascii_case("none") {
                continue;
            }
            let (hop_name, hop_data) = proxy_jump_data(&spec, config_file)?;
            let mut hop = Self::build(
                BuildRequest { host_name: &hop_name, data: &hop_data, connect_timeout, use_config_timeout: true },
                stack,
            )?;
            hop.proxy_jump_channel_timeout = connect_timeout;
            options.proxy_jump.append(&mut hop.proxy_jump);
            options.proxy_jump.push(hop);
        }
        stack.pop();
        Ok(options)
    }
}

#[derive(Clone, Copy)]
struct BuildRequest<'a> {
    host_name: &'a str,
    data: &'a Data,
    connect_timeout: Duration,
    use_config_timeout: bool,
}

fn proxy_jump_data(spec: &str, config_file: Option<&str>) -> Result<(String, Data), SshOptionsError> {
    let (user, host_port) = match spec.rsplit_once('@') {
        Some((user, host)) if !user.is_empty() => (Some(user), host),
        _ => (None, spec),
    };
    let (host, port) = if host_port.matches(':').count() <= 1 {
        match host_port.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() && !port.is_empty() => {
                let port = port.parse::<u64>().map_err(|_| SshOptionsError::InvalidProxyJump(spec.to_owned()))?;
                (host, Some(port))
            }
            _ => (host_port, None),
        }
    } else {
        (host_port, None)
    };
    if host.is_empty() {
        return Err(SshOptionsError::InvalidProxyJump(spec.to_owned()));
    }

    let mut data = Data::new();
    if let Some(config_file) = config_file {
        data.insert("ssh_config_file".to_owned(), Value::String(config_file.to_owned()));
    }
    if let Some(user) = user {
        data.insert("ssh_user".to_owned(), Value::String(user.to_owned()));
    }
    if let Some(port) = port {
        data.insert("ssh_port".to_owned(), Value::Number(port.into()));
    }
    Ok((host.to_owned(), data))
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
mod tests;
