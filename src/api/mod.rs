//! Public types used to describe and execute deploys.

/// Shell/path prefix denoting the current user's home directory.
pub const USER_HOME_PATH_PREFIX: &str = "~/";
pub(crate) const PROCESS_ID_COLUMN_HEADER: &str = "PID";
pub(crate) const USER_NAME_IDENTIFIER: &str = "USER";
pub(crate) const URL_SCHEME_SEPARATOR: &str = "://";
pub(crate) const IGNORE_STDERR_AND_FAILURE: &str = "2>/dev/null || true";

pub(crate) fn sha1_hex(value: &[u8]) -> String {
    use sha1::{Digest, Sha1};

    let hash = Sha1::digest(value);
    let mut hash_hex = String::with_capacity(hash.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in hash {
        hash_hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hash_hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    hash_hex
}

pub(crate) fn remote_temporary_filename(directory: &str, key: &[u8]) -> String {
    format!("{}/pyinfra-{}", directory.trim_end_matches('/'), sha1_hex(key))
}

mod command;
mod config;
mod inventory;
mod inventory_expansion;
mod state;
mod value;

pub use command::{
    Command, CommandBit, ContentUploadCommand, FileDownloadCommand, FileUploadCommand, RsyncCommand, StringCommand,
};
pub use config::{Config, ConfigState, DEFAULT_TEMP_DIRECTORY};
pub use inventory::{
    ConnectorKind, Data, GroupSpec, Host, HostSpec, Inventory, InventoryError, InventorySpec, SSH_HOSTNAME_DATA_KEY,
};
pub use inventory_expansion::{
    ConnectorRegistry, ExpandedHost, InventoryConnector, RawGroupSpec, RawHostSpec, RawInventorySpec,
};
pub use state::{HostResults, State, StateError, StateStage};
pub use value::{EnvValue, HiddenValue};
