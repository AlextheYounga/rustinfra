use crate::api::{Command, CommandBit, StringCommand, URL_SCHEME_SEPARATOR, remote_temporary_filename};
use crate::connectors::Connector;
use crate::facts::apt::{APT_KEYRING_DIRECTORIES, AptHeldPackages, AptSimulation, SimulateOperationWillChange};
use crate::facts::deb::{DebPackage, DebPackageInfo, DebPackages, InstalledPackages};
use crate::facts::files::{File, PathInfo, PathInput};
use crate::facts::server::Date;
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::gpg::{Key as GpgKey, KeyInput as GpgKeyInput};
use crate::operations::{Operation, StatefulOperation, StatefulOperationFuture};

const UPDATE_STAMP: &str = "/var/lib/apt/periodic/update-success-stamp";
const APT_PREFIX: &str = "DEBIAN_FRONTEND=noninteractive apt-get -y";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyInput {
    pub source: Option<String>,
    pub keyserver: Option<String>,
    pub key_ids: Vec<String>,
    pub destination: Option<String>,
    pub present: bool,
}
impl Default for KeyInput {
    fn default() -> Self {
        Self { source: None, keyserver: None, key_ids: Vec::new(), destination: None, present: true }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Key;
impl StatefulOperation for Key {
    type Input = KeyInput;
    fn name(&self) -> &'static str {
        "apt.key"
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
            let mut key = GpgKeyInput::default();
            key.source = input.source.clone();
            key.keyserver = input.keyserver.clone();
            key.key_ids = input.key_ids.clone();
            key.present = input.present;
            key.destination = resolve_key_destination(input);
            key.working_directories = if !input.present && key.destination.is_none() {
                APT_KEYRING_DIRECTORIES.iter().map(|value| (*value).to_owned()).collect()
            } else {
                Vec::new()
            };
            GpgKey.commands(&key, connector, options).await
        })
    }
}

fn resolve_key_destination(input: &KeyInput) -> Option<String> {
    if let Some(value) = &input.destination {
        return Some(if value.starts_with('/') {
            ensure_gpg(value)
        } else {
            format!("/etc/apt/keyrings/{}", ensure_gpg(value))
        });
    }
    if !input.present && !input.key_ids.is_empty() && input.source.is_none() && input.keyserver.is_none() {
        return None;
    }
    let base = if let Some(source) = &input.source {
        derive_source_name(source)
    } else if !input.key_ids.is_empty() {
        format!("keyserver-{}", sanitize(&input.key_ids.join("-")))
    } else {
        "apt-key".to_owned()
    };
    Some(format!("/etc/apt/keyrings/{base}.gpg"))
}
fn ensure_gpg(value: &str) -> String {
    if value.ends_with(".gpg") { value.to_owned() } else { format!("{value}.gpg") }
}
fn derive_source_name(source: &str) -> String {
    let without_scheme = source.split_once(URL_SCHEME_SEPARATOR);
    if let Some((_, rest)) = without_scheme {
        let (host, path) = rest.split_once('/').unwrap_or((rest, "key"));
        let name = path.rsplit('/').next().unwrap_or("key").trim_end_matches(".asc").trim_end_matches(".gpg");
        format!("{}-{}", sanitize(&host.replace(':', "_")), sanitize(if name.is_empty() { "key" } else { name }))
    } else {
        sanitize(source.rsplit('/').next().unwrap_or(source).trim_end_matches(".asc").trim_end_matches(".gpg"))
    }
}
fn sanitize(value: &str) -> String {
    let mut output = String::new();
    for character in value.trim().to_lowercase().chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '.' | '-') {
            output.push(character);
        } else if !output.ends_with('_') {
            output.push('_');
        }
    }
    let output = output.trim_matches(['_', '.']).to_owned();
    if output.is_empty() { "apt-keyring".to_owned() } else { output }
}

fn apt_command(action: &str, wait_seconds: Option<u64>, force: bool) -> Command {
    let mut bits = vec![raw(APT_PREFIX)];
    if force {
        bits.push(raw("--force-yes"));
    }
    if let Some(seconds) = wait_seconds {
        bits.extend([raw("-o"), raw(format!("DPkg::Lock::Timeout={seconds}"))]);
    }
    bits.extend([
        raw("-o"),
        raw("Dpkg::Options::=\"--force-confdef\""),
        raw("-o"),
        raw("Dpkg::Options::=\"--force-confold\""),
        raw(action),
    ]);
    shell(bits)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PpaInput {
    pub source: String,
    pub present: bool,
}
impl PpaInput {
    pub fn new(source: impl Into<String>) -> Self {
        Self { source: source.into(), present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ppa;
impl Operation for Ppa {
    type Input = PpaInput;
    fn name(&self) -> &'static str {
        "apt.ppa"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &PpaInput) -> Vec<Command> {
        let prefix = if input.present { "apt-add-repository -y" } else { "apt-add-repository -y --remove" };
        vec![shell([raw(prefix), quoted(input.source.clone())])]
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateInput {
    pub cache_seconds: Option<i64>,
    pub wait_seconds: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;
impl StatefulOperation for Update {
    type Input = UpdateInput;
    fn name(&self) -> &'static str {
        "apt.update"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        input: &'a UpdateInput,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            if let Some(cache_seconds) = input.cache_seconds {
                let facts = FactExecutor::new(connector, options);
                let file = facts.get(&File, &PathInput::from(UPDATE_STAMP)).await?;
                if let PathInfo::Present(info) = file
                    && let Some(modified) = info.modified_at
                    && modified > facts.get(&Date, &()).await?.timestamp() - cache_seconds
                {
                    return Ok(Vec::new());
                }
            }
            let mut bits = vec![raw("apt-get")];
            if let Some(seconds) = input.wait_seconds {
                bits.extend([raw("-o"), raw(format!("DPkg::Lock::Timeout={seconds}"))]);
            }
            bits.push(raw("update"));
            let mut commands = vec![shell(bits)];
            if input.cache_seconds.is_some() {
                commands.push(shell([raw("touch"), quoted(UPDATE_STAMP)]));
            }
            Ok(commands)
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpgradeInput {
    pub auto_remove: bool,
    pub wait_seconds: Option<u64>,
}

macro_rules! upgrade_operation {
    ($name:ident, $fact_name:literal, $action:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl StatefulOperation for $name {
            type Input = UpgradeInput;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn is_idempotent(&self) -> bool {
                true
            }
            fn commands<'a>(
                &'a self,
                input: &'a UpgradeInput,
                connector: &'a dyn Connector,
                options: &'a FactOptions,
            ) -> StatefulOperationFuture<'a> {
                Box::pin(async move {
                    let action = if input.auto_remove { concat!($action, " --autoremove") } else { $action };
                    let changes = FactExecutor::new(connector, options)
                        .get(&SimulateOperationWillChange, &action.to_owned())
                        .await?;
                    Ok(simulated_upgrade(action, input.wait_seconds, changes))
                })
            }
        }
    };
}

upgrade_operation!(Upgrade, "apt.upgrade", "upgrade");
upgrade_operation!(DistUpgrade, "apt.dist_upgrade", "dist-upgrade");

fn simulated_upgrade(action: &str, wait_seconds: Option<u64>, changes: Option<AptSimulation>) -> Vec<Command> {
    if changes.is_some_and(|changes| changes == AptSimulation::default()) {
        Vec::new()
    } else {
        vec![apt_command(action, wait_seconds, false)]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HoldInput {
    pub packages: Vec<String>,
    pub present: bool,
}
impl HoldInput {
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { packages: packages.into_iter().map(Into::into).collect(), present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Hold;
impl StatefulOperation for Hold {
    type Input = HoldInput;
    fn name(&self) -> &'static str {
        "apt.hold"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a HoldInput,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let mut names = input
                .packages
                .iter()
                .map(|package| package.split_once('=').map_or(package.as_str(), |(name, _)| name).trim().to_owned())
                .collect::<Vec<_>>();
            names.sort();
            names.dedup();
            if names.is_empty() || names.iter().any(String::is_empty) {
                return Err("all packages must have non-empty names".into());
            }
            let held = FactExecutor::new(connector, options).get(&AptHeldPackages, &()).await?;
            let changed = names.into_iter().filter(|name| held.contains(name) != input.present).collect::<Vec<_>>();
            if changed.is_empty() {
                return Ok(Vec::new());
            }
            let bits = [raw("apt-mark"), raw(if input.present { "hold" } else { "unhold" })]
                .into_iter()
                .chain(changed.into_iter().map(quoted));
            Ok(vec![shell(bits)])
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebInput {
    pub source: String,
    pub present: bool,
    pub force: bool,
    pub wait_seconds: Option<u64>,
}
impl DebInput {
    pub fn new(source: impl Into<String>) -> Self {
        Self { source: source.into(), present: true, force: false, wait_seconds: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Deb;
impl StatefulOperation for Deb {
    type Input = DebInput;
    fn name(&self) -> &'static str {
        "apt.deb"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a DebInput,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let is_url = input.source.split_once(URL_SCHEME_SEPARATOR).is_some();
            let temporary = remote_temporary_filename(&options.run.remote_temp_directory, input.source.as_bytes());
            let package = if is_url { temporary.as_str() } else { input.source.as_str() };
            let facts = FactExecutor::new(connector, options);
            let info = facts.get(&DebPackage, &package.to_owned()).await?;
            let installed = facts.get(&DebPackages, &()).await?;
            Ok(deb_commands(
                input,
                DebState {
                    temporary: is_url.then_some(temporary.as_str()),
                    package,
                    info: &info,
                    installed: &installed,
                },
            ))
        })
    }
}

#[derive(Clone, Copy)]
struct DebState<'a> {
    temporary: Option<&'a str>,
    package: &'a str,
    info: &'a DebPackageInfo,
    installed: &'a InstalledPackages,
}

fn deb_commands(input: &DebInput, state: DebState<'_>) -> Vec<Command> {
    let exists =
        state.info.name.as_ref().zip(state.info.version.as_ref()).is_some_and(|(name, version)| {
            state.installed.get(name).is_some_and(|versions| versions.contains(version))
        });
    let mut commands = Vec::new();
    if let Some(temporary) = state.temporary {
        commands.extend([
            shell([raw("curl"), raw("-sSLf"), quoted(input.source.clone()), raw("-o"), quoted(temporary)]),
            shell([raw("mv"), quoted(temporary), quoted(state.package)]),
        ]);
    }
    if input.present && !exists {
        commands.push(shell([
            raw("dpkg --force-confdef --force-confold -i"),
            quoted(state.package),
            raw("2> /dev/null || true"),
        ]));
        commands.push(apt_command("install -f", input.wait_seconds, input.force));
        commands.push(shell([raw("dpkg --force-confdef --force-confold -i"), quoted(state.package)]));
    } else if !input.present
        && exists
        && let Some(name) = state.info.name.as_deref()
    {
        let command = apt_command("remove", input.wait_seconds, input.force);
        if let Command::Shell(command) = command {
            commands.push(Command::Shell(StringCommand::new([command.into(), quoted(name)])));
        }
    }
    commands
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: impl Into<CommandBit>) -> CommandBit {
    CommandBit::quoted(value)
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
#[path = "extra_tests.rs"]
mod tests;
