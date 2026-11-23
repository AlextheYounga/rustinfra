use crate::api::{Command, CommandBit, StringCommand, URL_SCHEME_SEPARATOR, remote_temporary_filename};
use crate::connectors::Connector;
use crate::facts::apt::{AptHeldPackages, AptSimulation, SimulateOperationWillChange};
use crate::facts::deb::{DebPackage, DebPackageInfo, DebPackages, InstalledPackages};
use crate::facts::files::{File, PathInfo, PathInput};
use crate::facts::server::Date;
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{Operation, StatefulOperation, StatefulOperationFuture};

const UPDATE_STAMP: &str = "/var/lib/apt/periodic/update-success-stamp";
const APT_PREFIX: &str = "DEBIAN_FRONTEND=noninteractive apt-get -y";

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
mod tests {
    use super::*;
    use indexmap::IndexMap;
    #[test]
    fn ppa_upgrade_hold_and_deb_commands_match_reference_shapes() {
        assert!(
            matches!(&Ppa.commands(&PpaInput::new("x\"; reboot #"))[0], Command::Shell(command) if command.raw_value() == "apt-add-repository -y 'x\"; reboot #'")
        );
        assert!(simulated_upgrade("upgrade", None, Some(AptSimulation::default())).is_empty());
        assert!(
            matches!(&simulated_upgrade("dist-upgrade --autoremove", Some(60), None)[0], Command::Shell(command) if command.raw_value().contains("DPkg::Lock::Timeout=60"))
        );
        let deb = DebInput::new("unsafe package.deb");
        let info = DebPackageInfo { name: Some("test".to_owned()), version: Some("2".to_owned()) };
        let installed = IndexMap::from([("test".to_owned(), vec!["1".to_owned()])]);
        let commands =
            deb_commands(&deb, DebState { temporary: None, package: &deb.source, info: &info, installed: &installed });
        assert!(
            matches!(&commands[0], Command::Shell(command) if command.raw_value().contains("-i 'unsafe package.deb'"))
        );
    }
}
