//! Typed, read-only queries against target hosts.

pub mod apk;
pub mod apt;
pub mod cargo;
pub mod crontab;
pub mod deb;
mod executor;
pub mod files;
pub mod flatpak;
pub mod gem;
pub mod git;
pub mod go;
pub mod iptables;
pub mod npm;
pub mod opkg;
pub(crate) mod packages;
pub mod pacman;
pub mod pip;
pub mod pipx;
pub mod pkg;
pub mod pkgin;
pub mod pnpm;
pub mod rpm;
pub mod server;
pub mod snap;
pub mod systemd;
pub mod xbps;

pub use executor::{Fact, FactError, FactExecutor, FactOptions, Phase};
