//! Public types used to describe and execute deploys.

/// Shell/path prefix denoting the current user's home directory.
pub const USER_HOME_PATH_PREFIX: &str = "~/";

mod command;
mod config;
mod inventory;
mod inventory_expansion;
mod state;
mod value;

pub use command::{Command, CommandBit, FileDownloadCommand, FileUploadCommand, RsyncCommand, StringCommand};
pub use config::{Config, ConfigState, DEFAULT_TEMP_DIRECTORY};
pub use inventory::{
    ConnectorKind, Data, GroupSpec, Host, HostSpec, Inventory, InventoryError, InventorySpec, SSH_HOSTNAME_DATA_KEY,
};
pub use inventory_expansion::{
    ConnectorRegistry, ExpandedHost, InventoryConnector, RawGroupSpec, RawHostSpec, RawInventorySpec,
};
pub use state::{HostResults, State, StateError, StateStage};
pub use value::{EnvValue, HiddenValue};
