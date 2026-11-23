mod derived;
mod inspect;
mod other;
#[cfg(test)]
mod tests;

pub use derived::*;
pub use inspect::*;
pub use other::*;

use std::error::Error;
use std::io;

use serde_json::Value;

use crate::api::{CommandBit, StringCommand};

fn json(output: &[String]) -> Result<Value, Box<dyn Error + Send + Sync>> {
    Ok(serde_json::from_str(&output.join(""))?)
}

fn json_lines(output: &[String]) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
    output.iter().filter(|line| !line.trim().is_empty()).map(|line| Ok(serde_json::from_str(line)?)).collect()
}

fn invalid(message: &str) -> Box<dyn Error + Send + Sync> {
    io::Error::new(io::ErrorKind::InvalidData, message).into()
}

fn quote(value: &str) -> String {
    StringCommand::new([CommandBit::quoted(value)]).raw_value()
}
