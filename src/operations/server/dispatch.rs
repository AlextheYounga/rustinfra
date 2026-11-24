use crate::connectors::Connector;
use crate::facts::files::{Directory, Link, PathInfo, PathInput};
use crate::facts::server::{Os, Which, command_available};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::service::ServiceControl;
use crate::operations::{StatefulOperation, StatefulOperationFuture};
use crate::operations::{apk, apt, brew, bsdinit, dnf, openrc, pacman, pkg, runit, systemd, sysvinit, upstart, xbps};
use crate::operations::{yum, zypper};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { packages: packages.into_iter().map(Into::into).collect(), present: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;
    fn name(&self) -> &'static str {
        "server.packages"
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
            let facts = FactExecutor::new(connector, options);
            if which(&facts, "zypper").await? {
                let mut selected = zypper::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return zypper::Packages.commands(&selected, connector, options).await;
            }
            if which(&facts, "apk").await? {
                let mut selected = apk::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return apk::Packages.commands(&selected, connector, options).await;
            }
            if which(&facts, "apt").await? {
                let mut selected = apt::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return apt::Packages.commands(&selected, connector, options).await;
            }
            if which(&facts, "brew").await? {
                let mut selected = brew::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return brew::Packages.commands(&selected, connector, options).await;
            }
            if which(&facts, "dnf").await? {
                let mut selected = dnf::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return dnf::Packages.commands(&selected, connector, options).await;
            }
            if which(&facts, "pacman").await? {
                let mut selected = pacman::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return pacman::Packages.commands(&selected, connector, options).await;
            }
            if which(&facts, "xbps-install").await? || which(&facts, "xbps").await? {
                let mut selected = xbps::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return xbps::Packages.commands(&selected, connector, options).await;
            }
            if which(&facts, "yum").await? {
                let mut selected = yum::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return yum::Packages.commands(&selected, connector, options).await;
            }
            if which(&facts, "pkg").await? || which(&facts, pkg::PKG_ADD_COMMAND).await? {
                let mut selected = pkg::PackagesInput::new(input.packages.clone());
                selected.present = input.present;
                return pkg::Packages.commands(&selected, connector, options).await;
            }
            Err("no supported system package manager found".into())
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInput {
    pub service: String,
    pub running: bool,
    pub restarted: bool,
    pub reloaded: bool,
    pub command: Option<String>,
    pub enabled: Option<bool>,
}

impl ServiceInput {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into(), running: true, restarted: false, reloaded: false, command: None, enabled: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Service;

impl StatefulOperation for Service {
    type Input = ServiceInput;
    fn name(&self) -> &'static str {
        "server.service"
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
            let facts = FactExecutor::new(connector, options);
            if which(&facts, "systemctl").await? {
                let mut selected = systemd::ServiceInput::new(&input.service);
                selected.running = Some(input.running);
                selected.restarted = input.restarted;
                selected.reloaded = input.reloaded;
                selected.command.clone_from(&input.command);
                selected.enabled = input.enabled;
                return systemd::Service.commands(&selected, connector, options).await;
            }
            let control = service_control(input);
            if which(&facts, "rc-service").await? {
                let mut selected = openrc::ServiceInput::new(&input.service);
                selected.control = control;
                selected.enabled = input.enabled;
                return openrc::Service.commands(&selected, connector, options).await;
            }
            if which(&facts, "initctl").await? {
                let mut selected = upstart::ServiceInput::new(&input.service);
                selected.control = control;
                selected.enabled = input.enabled;
                return upstart::Service.commands(&selected, connector, options).await;
            }
            if which(&facts, "sv").await? {
                let mut selected = runit::ServiceInput::new(&input.service);
                selected.control = control;
                selected.enabled = input.enabled;
                return runit::Service.commands(&selected, connector, options).await;
            }
            let os = facts.get(&Os, &()).await?;
            if matches!(os.as_deref(), Some("FreeBSD" | "OpenBSD" | "NetBSD" | "DragonFly"))
                && matches!(facts.get(&Directory, &PathInput::from("/etc/rc.d")).await?, PathInfo::Present(_))
            {
                let mut selected = bsdinit::ServiceInput::new(&input.service);
                selected.control = control;
                selected.enabled = input.enabled;
                return bsdinit::Service.commands(&selected, connector, options).await;
            }
            let has_service = which(&facts, "service").await?;
            if has_service
                || matches!(facts.get(&Link, &PathInput::from("/etc/init.d")).await?, PathInfo::Present(_))
                || matches!(facts.get(&Directory, &PathInput::from("/etc/init.d")).await?, PathInfo::Present(_))
            {
                let mut selected = sysvinit::ServiceInput::new(&input.service);
                selected.control = control;
                selected.enabled = input.enabled;
                return sysvinit::Service.commands(&selected, connector, options).await;
            }
            Err("no supported init system found".into())
        })
    }
}

fn service_control(input: &ServiceInput) -> ServiceControl {
    ServiceControl {
        service: input.service.clone(),
        running: input.running,
        restarted: input.restarted,
        reloaded: input.reloaded,
        command: input.command.clone(),
    }
}

async fn which(facts: &FactExecutor<'_>, command: &str) -> Result<bool, Box<dyn Error + Send + Sync>> {
    Ok(command_available(facts.get(&Which, &command.to_owned()).await?.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::{ServiceInput, service_control};

    #[test]
    fn common_service_options_are_forwarded_without_loss() {
        let mut input = ServiceInput::new("worker");
        input.running = false;
        input.restarted = true;
        input.reloaded = true;
        input.command = Some("rotate".to_owned());
        let control = service_control(&input);
        assert_eq!(control.service, "worker");
        assert!(!control.running);
        assert!(control.restarted && control.reloaded);
        assert_eq!(control.command.as_deref(), Some("rotate"));
    }
}
use std::error::Error;
