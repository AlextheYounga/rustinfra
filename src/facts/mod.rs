//! Typed, read-only queries against target hosts.

pub mod apk;
pub mod apt;
pub mod cargo;
pub mod crontab;
pub mod deb;
mod executor;
pub mod files;
pub mod gem;
pub mod git;
pub mod go;
pub mod iptables;
pub mod npm;
pub(crate) mod packages;
pub mod pip;
pub mod pkgin;
pub mod rpm;
pub mod server;
pub mod systemd;
pub mod xbps;

pub use executor::{Fact, FactError, FactExecutor, FactOptions, Phase};
