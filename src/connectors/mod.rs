//! Connector implementations and shared command preparation.

mod connector;
mod local;
mod output;
mod unix;

pub use connector::{Connector, ConnectorError};
pub use local::{CommandResult, LocalConnector, LocalConnectorError, RunOptions};
pub use output::{CommandOutput, OutputLine, OutputStream};
pub use unix::{UnixCommandOptions, make_unix_command};
