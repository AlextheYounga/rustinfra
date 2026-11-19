#[cfg(unix)]
use std::env;
use std::io::Error as IoError;
use std::mem::take;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use russh::client;
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts, learn_known_hosts_path};
use russh::keys::ssh_key::{Error as CertificateError, PublicKey};
use russh::keys::{Error as KeyError, PublicKeyOrCertificate, check_known_hosts};
use russh::{Channel, ChannelMsg, ChannelOpenFailure, Disconnect};
use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpError;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time;

use crate::api::{RsyncCommand, StringCommand};

#[cfg(unix)]
use super::config::SSH_AUTH_SOCK_ENVIRONMENT_VARIABLE;
use super::options::{HostKeyPolicy, SshConnectionOptions, SshOptionsError};
use super::output::OutputCollector;
use super::proxy;
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
    #[error("SSH proxy failed: {0}")]
    Proxy(String),
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
    jump_sessions: RwLock<Vec<Arc<SshHandle>>>,
    sftp: RwLock<Option<Arc<SftpSession>>>,
}

impl SshConnector {
    #[must_use]
    pub fn new(options: SshConnectionOptions) -> Self {
        Self { options, session: RwLock::new(None), jump_sessions: RwLock::new(Vec::new()), sftp: RwLock::new(None) }
    }

    async fn connect_session(&self) -> Result<(SshHandle, Vec<SshHandle>), SshConnectorError> {
        proxy::connect(&self.options).await
    }

    pub(super) async fn run(
        &self,
        command: StringCommand,
        options: &RunOptions,
    ) -> Result<CommandResult, SshConnectorError> {
        let session = self.session.read().await.clone().ok_or(SshConnectorError::NotConnected)?;
        let unix_command = make_unix_command(command, &options.unix);
        let execution = execute_channel(&session, unix_command.raw_value(), options, self.options.forward_agent);
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
                    Ok((session, jumps)) => {
                        *self.jump_sessions.write().await = jumps.into_iter().map(Arc::new).collect();
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
            let mut first_error = None;
            if let Some(sftp) = self.sftp.write().await.take() {
                if let Err(error) = sftp.close().await {
                    first_error = Some(SshConnectorError::from(error));
                }
            }
            if let Some(session) = self.session.write().await.take() {
                if let Err(error) = session.disconnect(Disconnect::ByApplication, "", "English").await
                    && first_error.is_none()
                {
                    first_error = Some(SshConnectorError::from(error));
                }
            }
            let jumps = take(&mut *self.jump_sessions.write().await);
            for session in jumps.into_iter().rev() {
                if let Err(error) = session.disconnect(Disconnect::ByApplication, "", "English").await
                    && first_error.is_none()
                {
                    first_error = Some(SshConnectorError::from(error));
                }
            }
            if let Some(error) = first_error {
                return Err(ConnectorError::from(error));
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
    forward_agent: bool,
) -> Result<(Option<i32>, CommandOutput), SshConnectorError> {
    let mut channel = session.channel_open_session().await?;
    if forward_agent {
        channel.agent_forward(false).await?;
    }
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
    known_hosts_files: Vec<PathBuf>,
    policy: HostKeyPolicy,
    forward_agent_socket: Option<PathBuf>,
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

    #[cfg(unix)]
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        use tokio::io::copy_bidirectional;
        use tokio::net::UnixStream;

        let Some(path) = &self.forward_agent_socket else {
            reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
            return Ok(());
        };
        let Ok(mut agent) = UnixStream::connect(path).await else {
            reply.reject(ChannelOpenFailure::ConnectFailed).await;
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = copy_bidirectional(&mut stream, &mut agent).await;
        });
        Ok(())
    }

    #[cfg(not(unix))]
    async fn server_channel_open_agent_forward(
        &mut self,
        _channel: Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }
}

impl ServerKeyHandler {
    pub(super) fn for_options(options: &SshConnectionOptions) -> Self {
        Self {
            hostname: options.hostname.clone(),
            port: options.port,
            known_hosts_files: options.known_hosts_files.clone(),
            policy: options.host_key_policy,
            forward_agent_socket: forward_agent_socket(options),
        }
    }

    fn check_known_host(&self, key: &PublicKey) -> Result<bool, KeyError> {
        if self.known_hosts_files.is_empty() {
            return check_known_hosts(&self.hostname, self.port, key);
        }
        for path in &self.known_hosts_files {
            if check_known_hosts_path(&self.hostname, self.port, key, path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn learn_known_host(&self, key: &PublicKey) -> Result<(), KeyError> {
        match self.known_hosts_files.first() {
            Some(path) => learn_known_hosts_path(&self.hostname, self.port, key, path),
            None => learn_known_hosts(&self.hostname, self.port, key),
        }
    }
}

#[cfg(unix)]
fn forward_agent_socket(options: &SshConnectionOptions) -> Option<PathBuf> {
    if !options.forward_agent {
        return None;
    }
    options.identity_agent.clone().or_else(|| env::var_os(SSH_AUTH_SOCK_ENVIRONMENT_VARIABLE).map(PathBuf::from))
}

#[cfg(not(unix))]
fn forward_agent_socket(_options: &SshConnectionOptions) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests;
