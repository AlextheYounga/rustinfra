use std::io::Error as IoError;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use russh::client;
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts, learn_known_hosts_path};
use russh::keys::ssh_key::{Error as CertificateError, PublicKey};
use russh::keys::{Error as KeyError, PublicKeyOrCertificate, check_known_hosts};
use russh::{Channel, ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpError;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time;

use crate::api::{RsyncCommand, StringCommand};

use super::auth::{authenticate_agent, authenticate_config_keys, authenticate_default_keys, authenticate_key};
use super::options::{HostKeyPolicy, SshConnectionOptions, SshOptionsError};
use super::output::OutputCollector;
use crate::connectors::{
    CommandOutput, CommandResult, Connector, ConnectorError, ConnectorFuture, ConnectorTransferFuture,
    FileTransferOptions, OutputStream, RunOptions, make_unix_command,
};

pub(super) type SshHandle = client::Handle<ServerKeyHandler>;

/// SSH connection, authentication, channel, or transfer failure.
#[derive(Debug, Error)]
pub enum SshConnectorError {
    #[error(transparent)]
    Options(#[from] SshOptionsError),
    #[error("SSH protocol error: {0}")]
    Protocol(#[from] russh::Error),
    #[error("SSH key error: {0}")]
    Key(#[from] KeyError),
    #[error("SSH certificate error: {0}")]
    Certificate(#[from] CertificateError),
    #[error("SSH authentication failed for {user}@{host}")]
    Authentication { user: String, host: String },
    #[error("SSH agent authentication failed: {0}")]
    Agent(String),
    #[error("SFTP error: {0}")]
    Sftp(#[from] SftpError),
    #[error("SSH transfer I/O error: {0}")]
    TransferIo(#[from] IoError),
    #[error("SSH file transfer command failed during {action}: {stderr}")]
    TransferCommand { action: &'static str, stderr: String },
    #[error("SCP transfer failed: {0}")]
    Scp(String),
    #[error("SSH command requested before connecting")]
    NotConnected,
    #[error("SSH command timed out after {0:?}")]
    Timeout(Duration),
    #[error("SSH file transfers are not implemented yet")]
    TransferNotImplemented,
}

/// Persistent SSH connection that opens an independent channel for each command.
pub struct SshConnector {
    pub(super) options: SshConnectionOptions,
    session: RwLock<Option<Arc<SshHandle>>>,
    sftp: RwLock<Option<Arc<SftpSession>>>,
}

impl SshConnector {
    #[must_use]
    pub fn new(options: SshConnectionOptions) -> Self {
        Self { options, session: RwLock::new(None), sftp: RwLock::new(None) }
    }

    async fn connect_session(&self) -> Result<SshHandle, SshConnectorError> {
        time::timeout(self.options.connect_timeout, self.connect_session_without_timeout())
            .await
            .map_err(|_| SshConnectorError::Timeout(self.options.connect_timeout))?
    }

    async fn connect_session_without_timeout(&self) -> Result<SshHandle, SshConnectorError> {
        let config = Arc::new(client::Config {
            keepalive_interval: self.options.server_alive_interval,
            ..client::Config::default()
        });
        let handler = ServerKeyHandler {
            hostname: self.options.hostname.clone(),
            port: self.options.port,
            known_hosts_file: self.options.known_hosts_file.clone(),
            policy: self.options.host_key_policy,
        };
        let address = (self.options.hostname.as_str(), self.options.port);
        let mut session = client::connect(config, address, handler).await.map_err(|error| match error {
            HandlerError::Protocol(error) => SshConnectorError::Protocol(error),
            HandlerError::HostKey(error) => SshConnectorError::Key(error),
        })?;

        if self.authenticate(&mut session).await? {
            Ok(session)
        } else {
            Err(SshConnectorError::Authentication {
                user: self.options.user.clone(),
                host: self.options.hostname.clone(),
            })
        }
    }

    async fn authenticate(&self, session: &mut SshHandle) -> Result<bool, SshConnectorError> {
        if let Some(key_path) = &self.options.key {
            if authenticate_key(session, &self.options.user, key_path, self.options.key_password.as_ref()).await? {
                return Ok(true);
            }
        } else {
            if authenticate_config_keys(
                session,
                &self.options.user,
                &self.options.config_identity_files,
                self.options.key_password.as_ref(),
            )
            .await?
            {
                return Ok(true);
            }
            if self.options.allow_agent && authenticate_agent(session, &self.options.user).await? {
                return Ok(true);
            }
            if self.options.look_for_keys && authenticate_default_keys(session, &self.options.user).await? {
                return Ok(true);
            }
        }

        if let Some(password) = &self.options.password {
            return Ok(session.authenticate_password(&self.options.user, password.unmask()).await?.success());
        }
        Ok(session.authenticate_none(&self.options.user).await?.success())
    }

    pub(super) async fn run(
        &self,
        command: StringCommand,
        options: &RunOptions,
    ) -> Result<CommandResult, SshConnectorError> {
        let session = self.session.read().await.clone().ok_or(SshConnectorError::NotConnected)?;
        let unix_command = make_unix_command(command, &options.unix);
        let execution = execute_channel(&session, unix_command.raw_value(), options);
        let (exit_code, output) = match options.timeout {
            Some(timeout) => {
                time::timeout(timeout, execution).await.map_err(|_| SshConnectorError::Timeout(timeout))??
            }
            None => execution.await?,
        };
        let success = exit_code.is_some_and(|code| options.success_exit_codes.contains(&code));
        Ok(CommandResult { success, exit_code, output })
    }

    pub(super) async fn sftp(&self) -> Result<Arc<SftpSession>, SshConnectorError> {
        if let Some(sftp) = self.sftp.read().await.clone() {
            return Ok(sftp);
        }

        let mut current = self.sftp.write().await;
        if let Some(sftp) = current.clone() {
            return Ok(sftp);
        }
        let session = self.session.read().await.clone().ok_or(SshConnectorError::NotConnected)?;
        let channel = session.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = Arc::new(SftpSession::new(channel.into_stream()).await?);
        *current = Some(Arc::clone(&sftp));
        Ok(sftp)
    }

    pub(super) async fn open_channel(&self) -> Result<Channel<client::Msg>, SshConnectorError> {
        let session = self.session.read().await.clone().ok_or(SshConnectorError::NotConnected)?;
        Ok(session.channel_open_session().await?)
    }
}

impl Connector for SshConnector {
    fn connect(&self) -> ConnectorTransferFuture<'_> {
        Box::pin(async move {
            let mut current = self.session.write().await;
            if current.is_some() {
                return Ok(());
            }
            let mut retries = self.options.connect_retries;
            loop {
                match self.connect_session().await {
                    Ok(session) => {
                        *current = Some(Arc::new(session));
                        return Ok(());
                    }
                    Err(error) if retries > 0 => {
                        retries -= 1;
                        time::sleep(retry_delay(&self.options)).await;
                        tracing::debug!(host = %self.options.hostname, %error, retries, "retrying SSH connection");
                    }
                    Err(error) => return Err(ConnectorError::from(error)),
                }
            }
        })
    }

    fn disconnect(&self) -> ConnectorTransferFuture<'_> {
        Box::pin(async move {
            if let Some(sftp) = self.sftp.write().await.take() {
                sftp.close().await.map_err(SshConnectorError::from)?;
            }
            if let Some(session) = self.session.write().await.take() {
                session.disconnect(Disconnect::ByApplication, "", "English").await.map_err(SshConnectorError::from)?;
            }
            Ok(())
        })
    }

    fn run_shell_command<'a>(&'a self, command: StringCommand, options: &'a RunOptions) -> ConnectorFuture<'a> {
        Box::pin(async move { self.run(command, options).await.map_err(ConnectorError::from) })
    }

    fn put_file<'a>(
        &'a self,
        source: &'a Path,
        destination: &'a str,
        options: FileTransferOptions<'a>,
    ) -> ConnectorTransferFuture<'a> {
        Box::pin(async move { self.put(source, destination, options).await.map_err(ConnectorError::from) })
    }

    fn get_file<'a>(
        &'a self,
        source: &'a str,
        destination: &'a Path,
        options: FileTransferOptions<'a>,
    ) -> ConnectorTransferFuture<'a> {
        Box::pin(async move { self.get(source, destination, options).await.map_err(ConnectorError::from) })
    }

    fn rsync<'a>(&'a self, _command: &'a RsyncCommand, _options: &'a RunOptions) -> ConnectorTransferFuture<'a> {
        Box::pin(async { Err(SshConnectorError::TransferNotImplemented.into()) })
    }
}

fn retry_delay(options: &SshConnectionOptions) -> Duration {
    let minimum = options.connect_retry_min_delay.as_secs_f64();
    let maximum = options.connect_retry_max_delay.as_secs_f64();
    Duration::from_secs_f64(rand::random_range(minimum..=maximum))
}

async fn execute_channel(
    session: &SshHandle,
    command: String,
    options: &RunOptions,
) -> Result<(Option<i32>, CommandOutput), SshConnectorError> {
    let mut channel = session.channel_open_session().await?;
    if options.get_pty {
        channel.request_pty(false, "xterm", 80, 24, 0, 0, &[]).await?;
    }
    channel.exec(true, command).await?;
    for line in &options.stdin {
        channel.data(line.as_bytes()).await?;
        if !line.ends_with('\n') {
            channel.data(&b"\n"[..]).await?;
        }
    }
    channel.eof().await?;

    let mut collector = OutputCollector::default();
    let mut exit_code = None;
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => collector.push(OutputStream::Stdout, &data),
            ChannelMsg::ExtendedData { data, ext: 1 } => collector.push(OutputStream::Stderr, &data),
            ChannelMsg::ExitStatus { exit_status } => exit_code = i32::try_from(exit_status).ok(),
            _ => {}
        }
    }
    Ok((exit_code, collector.finish()))
}

#[derive(Clone)]
pub(super) struct ServerKeyHandler {
    hostname: String,
    port: u16,
    known_hosts_file: Option<PathBuf>,
    policy: HostKeyPolicy,
}

#[derive(Debug, Error)]
pub(super) enum HandlerError {
    #[error(transparent)]
    Protocol(#[from] russh::Error),
    #[error(transparent)]
    HostKey(#[from] KeyError),
}

impl client::Handler for ServerKeyHandler {
    type Error = HandlerError;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = server_key.public_key();
        match self.policy {
            HostKeyPolicy::Accept => Ok(true),
            HostKeyPolicy::Ask | HostKeyPolicy::Strict => Ok(self.check_known_host(&key)?),
            HostKeyPolicy::AcceptNew => match self.check_known_host(&key)? {
                true => Ok(true),
                false => {
                    self.learn_known_host(&key)?;
                    Ok(true)
                }
            },
        }
    }
}

impl ServerKeyHandler {
    fn check_known_host(&self, key: &PublicKey) -> Result<bool, KeyError> {
        match &self.known_hosts_file {
            Some(path) => check_known_hosts_path(&self.hostname, self.port, key, path),
            None => check_known_hosts(&self.hostname, self.port, key),
        }
    }

    fn learn_known_host(&self, key: &PublicKey) -> Result<(), KeyError> {
        match &self.known_hosts_file {
            Some(path) => learn_known_hosts_path(&self.hostname, self.port, key, path),
            None => learn_known_hosts(&self.hostname, self.port, key),
        }
    }
}

#[cfg(test)]
mod tests;
