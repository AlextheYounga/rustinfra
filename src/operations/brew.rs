use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::iter::once;

use thiserror::Error;

use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::brew::{
    BrewCasks, BrewItemKind, BrewPackages, BrewTaps, BrewTrustMap, BrewTrusted, BrewVersion, new_cask_cli,
};
use crate::facts::packages::PackageMap;
use crate::facts::{FactExecutor, FactOptions};

use super::packages::{PackageCommands, PackagePolicy, converge_packages};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;

impl Operation for Update {
    type Input = ();
    fn name(&self) -> &'static str {
        "brew.update"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![shell([raw("brew update")])]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Upgrade;

impl Operation for Upgrade {
    type Input = ();
    fn name(&self) -> &'static str {
        "brew.upgrade"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, _input: &Self::Input) -> Vec<Command> {
        vec![shell([raw("brew upgrade")])]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub update: bool,
    pub upgrade: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            update: false,
            upgrade: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;
    fn name(&self) -> &'static str {
        "brew.packages"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let installed = FactExecutor::new(connector, options).get(&BrewPackages, &()).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &PackageMap) -> Vec<Command> {
    let mut commands = Vec::new();
    if input.update {
        commands.push(shell([raw("brew update")]));
    }
    if input.upgrade {
        commands.push(shell([raw("brew upgrade")]));
    }
    if input.packages.is_empty() {
        return commands;
    }
    let changed =
        input.packages.iter().filter(|package| installed.contains_key(*package) != input.present).collect::<Vec<_>>();
    let upgrades = input
        .packages
        .iter()
        .filter(|package| input.present && input.latest && installed.contains_key(*package))
        .collect::<Vec<_>>();
    if !changed.is_empty() {
        commands.push(package_list_command(if input.present { "brew install" } else { "brew uninstall" }, changed));
    }
    if !upgrades.is_empty() {
        commands.push(package_list_command("brew upgrade", upgrades));
    }
    commands
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CaskUpgrade;

impl StatefulOperation for CaskUpgrade {
    type Input = ();
    fn name(&self) -> &'static str {
        "brew.cask_upgrade"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        _input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let version = FactExecutor::new(connector, options).get(&BrewVersion, &()).await?;
            Ok(vec![cask_command(version, "upgrade", &[])])
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CasksInput {
    pub casks: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub upgrade: bool,
}

impl CasksInput {
    #[must_use]
    pub fn new(casks: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { casks: casks.into_iter().map(Into::into).collect(), present: true, latest: false, upgrade: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Casks;

impl StatefulOperation for Casks {
    type Input = CasksInput;
    fn name(&self) -> &'static str {
        "brew.casks"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let executor = FactExecutor::new(connector, options);
            let version = executor.get(&BrewVersion, &()).await?;
            let installed = executor.get(&BrewCasks, &()).await?;
            let (install, uninstall, upgrade) = cask_commands(version);
            let mut commands =
                input.upgrade.then(|| cask_command(version, "upgrade", &[])).into_iter().collect::<Vec<_>>();
            commands.extend(converge_packages(
                &input.casks,
                &installed,
                PackagePolicy { present: input.present, latest: input.latest },
                &PackageCommands { install, uninstall, upgrade, version_join: "@" },
            ));
            Ok(commands)
        })
    }
}

fn cask_commands(version: [u32; 3]) -> (Vec<CommandBit>, Vec<CommandBit>, Vec<CommandBit>) {
    let action = |name| {
        if new_cask_cli(version) {
            vec![raw("brew"), raw(name), raw("--cask")]
        } else {
            vec![raw("brew cask"), raw(name)]
        }
    };
    (action("install"), action("uninstall"), action("upgrade"))
}

fn cask_command(version: [u32; 3], action: &str, packages: &[&str]) -> Command {
    let mut bits = if new_cask_cli(version) {
        vec![raw("brew"), raw(action), raw("--cask")]
    } else {
        vec![raw("brew cask"), raw(action)]
    };
    bits.extend(packages.iter().map(|package| quoted(package)));
    shell(bits)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TapInput {
    pub source: Option<String>,
    pub present: bool,
    pub trusted: bool,
    pub url: Option<String>,
}

impl Default for TapInput {
    fn default() -> Self {
        Self { source: None, present: true, trusted: false, url: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tap;

impl StatefulOperation for Tap {
    type Input = TapInput;
    fn name(&self) -> &'static str {
        "brew.tap"
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
            let Some(source) = tap_source(input) else {
                return Ok(Vec::new());
            };
            if source.split('/').count() != 2 {
                return Ok(Vec::new());
            }
            let executor = FactExecutor::new(connector, options);
            let taps = executor.get(&BrewTaps, &()).await?;
            let exists = taps.contains(&source);
            let trusted = if input.present && exists {
                executor.get(&BrewTrusted, &()).await?.get("taps").is_some_and(|items| items.contains(&source))
            } else {
                false
            };
            Ok(tap_commands(input, &source, exists, trusted))
        })
    }
}

fn tap_source(input: &TapInput) -> Option<String> {
    input.source.clone().or_else(|| {
        let url = input.url.as_deref()?;
        let path = url.split_once("://").map_or(url, |(_, rest)| rest).split_once('/').map(|(_, path)| path)?;
        Some(path.trim_matches('/').to_owned())
    })
}

fn tap_commands(input: &TapInput, source: &str, exists: bool, trusted: bool) -> Vec<Command> {
    if input.present && exists {
        return (trusted != input.trusted)
            .then(|| trust_command(source, BrewItemKind::Tap, input.trusted))
            .into_iter()
            .collect();
    }
    if exists {
        return vec![shell([raw("brew untap"), quoted(source)])];
    }
    if !input.present {
        return Vec::new();
    }
    let mut commands = vec![shell([raw("brew tap"), quoted(source)])];
    if let Some(url) = &input.url {
        commands[0] = shell([raw("brew tap"), quoted(source), quoted(url)]);
    }
    if input.trusted {
        commands.push(trust_command(source, BrewItemKind::Tap, true));
    }
    commands
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustInput {
    pub items: Vec<String>,
    pub kind: BrewItemKind,
    pub trusted: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Trust;

impl StatefulOperation for Trust {
    type Input = TrustInput;
    fn name(&self) -> &'static str {
        "brew.trust"
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
            if input.items.iter().any(String::is_empty) {
                return Err(Box::new(BrewOperationError::EmptyTrustItem) as Box<dyn StdError + Send + Sync>);
            }
            let trusted = FactExecutor::new(connector, options).get(&BrewTrusted, &()).await?;
            Ok(trust_commands(input, &trusted))
        })
    }
}

fn trust_commands(input: &TrustInput, trusted: &BrewTrustMap) -> Vec<Command> {
    let current = trusted.get(input.kind.key()).cloned().unwrap_or_default();
    input
        .items
        .iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|item| current.contains(item) != input.trusted)
        .map(|item| trust_command(item, input.kind, input.trusted))
        .collect()
}

fn trust_command(item: &str, kind: BrewItemKind, trusted: bool) -> Command {
    shell([raw("brew"), raw(if trusted { "trust" } else { "untrust" }), raw(kind.option()), quoted(item)])
}

fn package_list_command<'a>(prefix: &str, packages: impl IntoIterator<Item = &'a String>) -> Command {
    shell(once(raw(prefix)).chain(packages.into_iter().map(|package| quoted(package))))
}
fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BrewOperationError {
    #[error("all items must have non-zero length names")]
    EmptyTrustItem,
}

#[cfg(test)]
mod tests;
