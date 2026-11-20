//! Typed, read-only queries against target hosts.

pub mod apt;
pub mod crontab;
pub mod deb;
mod executor;
pub mod files;
pub mod git;
pub mod iptables;
pub mod pip;
pub mod rpm;
pub mod server;
pub mod systemd;

pub use executor::{Fact, FactError, FactExecutor, FactOptions, Phase};
