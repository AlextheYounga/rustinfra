use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use thiserror::Error;

use crate::api::{RsyncCommand, StringCommand};

use super::{CommandResult, LocalConnector, LocalConnectorError, RunOptions, SshConnectorError};

/// A boxed connector call, allowing connectors to be selected at runtime.
pub type ConnectorFuture<'a> = Pin<Box<dyn Future<Output = Result<CommandResult, ConnectorError>> + Send + 'a>>;
pub type ConnectorTransferFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ConnectorError>> + Send + 'a>>;

#[derive(Clone, Copy)]
pub struct FileTransferOptions<'a> {
    pub run: &'a RunOptions,
    pub remote_temporary_filename: Option<&'a str>,
}

/// Common execution failure reported by a connector.
#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error(transparent)]
    Local(#[from] LocalConnectorError),
    #[error("connection failed: {message}")]
    Connection { message: String },
    #[error(transparent)]
    Ssh(#[from] SshConnectorError),
}

/// Executes prepared shell commands against one target.
pub trait Connector: Send + Sync {
    fn connect(&self) -> ConnectorTransferFuture<'_>;
    fn disconnect(&self) -> ConnectorTransferFuture<'_>;
    fn run_shell_command<'a>(&'a self, command: StringCommand, options: &'a RunOptions) -> ConnectorFuture<'a>;
    fn put_file<'a>(
        &'a self,
        source: &'a Path,
        destination: &'a str,
        options: FileTransferOptions<'a>,
    ) -> ConnectorTransferFuture<'a>;
    fn get_file<'a>(
        &'a self,
        source: &'a str,
        destination: &'a Path,
        options: FileTransferOptions<'a>,
    ) -> ConnectorTransferFuture<'a>;
    fn rsync<'a>(&'a self, command: &'a RsyncCommand, options: &'a RunOptions) -> ConnectorTransferFuture<'a>;
}

impl Connector for LocalConnector {
    fn connect(&self) -> ConnectorTransferFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn disconnect(&self) -> ConnectorTransferFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn run_shell_command<'a>(&'a self, command: StringCommand, options: &'a RunOptions) -> ConnectorFuture<'a> {
        Box::pin(async move { self.run_shell_command(command, options).await.map_err(ConnectorError::from) })
    }

    fn put_file<'a>(
        &'a self,
        source: &'a Path,
        destination: &'a str,
        options: FileTransferOptions<'a>,
    ) -> ConnectorTransferFuture<'a> {
        Box::pin(async move { self.put_file(source, destination, options.run).await.map_err(ConnectorError::from) })
    }

    fn get_file<'a>(
        &'a self,
        source: &'a str,
        destination: &'a Path,
        options: FileTransferOptions<'a>,
    ) -> ConnectorTransferFuture<'a> {
        Box::pin(async move { self.get_file(source, destination, options.run).await.map_err(ConnectorError::from) })
    }

    fn rsync<'a>(&'a self, command: &'a RsyncCommand, options: &'a RunOptions) -> ConnectorTransferFuture<'a> {
        Box::pin(async move { self.rsync(command, options).await.map_err(ConnectorError::from) })
    }
}
