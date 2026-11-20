use std::error::Error as StdError;

use thiserror::Error;

use crate::api::Command;
use crate::connectors::Connector;
use crate::facts::files::{File, PathInfo, PathInput};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::{Directory, DirectoryInput};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

use super::{quoted, raw, shell};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkMode {
    Clone,
    Copy,
    Hardlink,
    Symlink,
}

impl LinkMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Clone => "clone",
            Self::Copy => "copy",
            Self::Hardlink => "hardlink",
            Self::Symlink => "symlink",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VenvInput {
    pub path: String,
    pub present: bool,
    pub python: Option<String>,
    pub allow_existing: bool,
    pub clear_existing: bool,
    pub link_mode: Option<LinkMode>,
    pub seed: bool,
    pub site_packages: bool,
    pub extra_arguments: Vec<String>,
}

impl VenvInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            present: true,
            python: None,
            allow_existing: false,
            clear_existing: false,
            link_mode: None,
            seed: false,
            site_packages: false,
            extra_arguments: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Venv;

impl StatefulOperation for Venv {
    type Input = VenvInput;

    fn name(&self) -> &'static str {
        "uv.venv"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let indicator = format!("{}/.venv/bin/activate", input.path);
            let exists = matches!(
                FactExecutor::new(connector, options).get(&File, &PathInput::from(indicator)).await?,
                PathInfo::Present(_)
            );
            if input.present && input.allow_existing && input.clear_existing {
                return Err(Box::new(UvOperationError::ConflictingExistingOptions) as Box<dyn StdError + Send + Sync>);
            }
            if input.present && input.path.trim().is_empty() {
                return Err(Box::new(UvOperationError::EmptyPath) as Box<dyn StdError + Send + Sync>);
            }
            if input.present {
                return Ok(venv_create_commands(input, exists));
            }
            if !exists {
                return Ok(Vec::new());
            }
            let mut directory = DirectoryInput::new(format!("{}/.venv", input.path));
            directory.present = false;
            Directory.commands(&directory, connector, options).await
        })
    }
}

pub(super) fn venv_create_commands(input: &VenvInput, exists: bool) -> Vec<Command> {
    if exists {
        return Vec::new();
    }
    let mut bits = vec![raw("uv venv")];
    if input.allow_existing {
        bits.push(raw("--allow-existing"));
    }
    if input.clear_existing {
        bits.push(raw("--clear"));
    }
    if input.seed {
        bits.push(raw("--seed"));
    }
    if let Some(mode) = input.link_mode {
        bits.extend([raw("--link-mode"), raw(mode.as_str())]);
    }
    if input.site_packages {
        bits.push(raw("--site-packages"));
    }
    if let Some(python) = &input.python {
        bits.extend([raw("--python"), quoted(python)]);
    }
    bits.extend(input.extra_arguments.iter().map(|argument| quoted(argument)));
    bits.push(quoted(&input.path));
    vec![shell(bits)]
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum UvOperationError {
    #[error("allow_existing and clear_existing cannot both be set")]
    ConflictingExistingOptions,
    #[error("must provide path in which .venv is to be created")]
    EmptyPath,
}
