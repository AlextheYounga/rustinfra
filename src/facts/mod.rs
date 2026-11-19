//! Typed, read-only queries against target hosts.

pub mod deb;
mod executor;
pub mod files;
pub mod rpm;
pub mod server;

pub use executor::{Fact, FactError, FactExecutor, FactOptions, Phase};
