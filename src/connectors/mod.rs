//! Connector implementations and shared command preparation.

mod connect;
mod connector;
mod local;
mod output;
mod unix;

pub use connect::{ConnectReport, ConnectionManager, ConnectionManagerError};
pub use connector::{Connector, ConnectorError, ConnectorFuture, ConnectorTransferFuture};
pub use local::{CommandResult, LocalConnector, LocalConnectorError, RunOptions};
pub use output::{CommandOutput, OutputLine, OutputStream};
pub use unix::{UnixCommandOptions, make_unix_command};
