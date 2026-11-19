//! Typed, read-only queries against target hosts.

mod executor;
pub mod server;

pub use executor::{Fact, FactError, FactExecutor, FactOptions, Phase};
