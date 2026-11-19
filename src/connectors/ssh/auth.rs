use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::keys::{PrivateKeyWithHashAlg, load_openssh_certificate, load_secret_key};

use super::config::HOME_ENVIRONMENT_VARIABLE;
use super::session::{SshConnectorError, SshHandle};
use crate::api::HiddenValue;

pub(super) async fn authenticate_key(
    session: &mut SshHandle,
    user: &str,
    path: &Path,
    password: Option<&HiddenValue>,
) -> Result<bool, SshConnectorError> {
    let key = load_secret_key(path, password.map(HiddenValue::unmask))?;
    let mut certificate_path = path.as_os_str().to_os_string();
    certificate_path.push("-cert.pub");
    let certificate_path = PathBuf::from(certificate_path);
    if certificate_path.is_file() {
        let certificate = load_openssh_certificate(certificate_path)?;
        return Ok(session.authenticate_openssh_cert(user, Arc::new(key), certificate).await?.success());
    }
    let hash = session.best_supported_rsa_hash().await?.flatten();
    Ok(session.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await?.success())
}

pub(super) async fn authenticate_config_keys(
    session: &mut SshHandle,
    user: &str,
    paths: &[PathBuf],
    password: Option<&HiddenValue>,
) -> Result<bool, SshConnectorError> {
    for path in paths.iter().filter(|path| path.is_file()) {
        match authenticate_key(session, user, path, password).await {
            Ok(true) => return Ok(true),
            Ok(false) | Err(SshConnectorError::Key(_) | SshConnectorError::Certificate(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

#[cfg(unix)]
pub(super) async fn authenticate_agent(session: &mut SshHandle, user: &str) -> Result<bool, SshConnectorError> {
    use russh::keys::agent::AgentIdentity;
    use russh::keys::agent::client::AgentClient;

    let Ok(mut agent) = AgentClient::connect_env().await else {
        return Ok(false);
    };
    if let Some(binding) = session.agent_session_binding() {
        agent.bind_session(binding, false).await.map_err(|error| SshConnectorError::Agent(error.to_string()))?;
    }
    let identities = agent.request_identities().await.map_err(|error| SshConnectorError::Agent(error.to_string()))?;
    for identity in identities {
        let result = match identity {
            AgentIdentity::PublicKey { key, .. } => session
                .authenticate_publickey_with(user, key, None, &mut agent)
                .await
                .map_err(|error| SshConnectorError::Agent(error.to_string()))?,
            AgentIdentity::Certificate { certificate, .. } => session
                .authenticate_certificate_with(user, certificate, None, &mut agent)
                .await
                .map_err(|error| SshConnectorError::Agent(error.to_string()))?,
        };
        if result.success() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(not(unix))]
pub(super) async fn authenticate_agent(_session: &mut SshHandle, _user: &str) -> Result<bool, SshConnectorError> {
    Ok(false)
}

pub(super) async fn authenticate_default_keys(session: &mut SshHandle, user: &str) -> Result<bool, SshConnectorError> {
    let Some(home) = env::var_os(HOME_ENVIRONMENT_VARIABLE) else {
        return Ok(false);
    };
    let paths = ["id_ed25519", "id_ecdsa", "id_rsa"].map(|name| Path::new(&home).join(".ssh").join(name));
    authenticate_config_keys(session, user, &paths, None).await
}
