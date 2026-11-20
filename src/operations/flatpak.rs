use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::flatpak::FlatpakPackages;
use crate::facts::{FactExecutor, FactOptions};

use super::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub remote: Option<String>,
    pub present: bool,
    pub user: bool,
}

impl PackagesInput {
    #[must_use]
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { packages: packages.into_iter().map(Into::into).collect(), remote: None, present: true, user: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Packages;

impl StatefulOperation for Packages {
    type Input = PackagesInput;

    fn name(&self) -> &'static str {
        "flatpak.packages"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let installed = FactExecutor::new(connector, fact_options).get(&FlatpakPackages, &input.user).await?;
            Ok(package_commands(input, &installed))
        })
    }
}

fn package_commands(input: &PackagesInput, installed: &[String]) -> Vec<Command> {
    let changed =
        input.packages.iter().filter(|package| installed.contains(package) != input.present).collect::<Vec<_>>();
    if changed.is_empty() {
        return Vec::new();
    }

    let action = if input.present { "install" } else { "uninstall" };
    let mut bits = vec![CommandBit::from(format!("flatpak {action} --noninteractive"))];
    if input.user {
        bits.push(CommandBit::from("--user"));
    }
    if input.present
        && let Some(remote) = input.remote.as_deref().map(str::trim).filter(|remote| !remote.is_empty())
    {
        bits.push(CommandBit::quoted(remote));
    }
    bits.extend(changed.into_iter().map(|package| CommandBit::quoted(package.as_str())));
    vec![Command::Shell(StringCommand::new(bits))]
}

#[cfg(test)]
mod tests {
    use super::{PackagesInput, package_commands};
    use crate::api::Command;

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some(command.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn matches_install_remove_user_remote_and_quoting_fixtures() {
        let mut input = PackagesInput::new(["org.videolan.VLC", "x; reboot"]);
        input.remote = Some(" flathub ".to_owned());
        assert_eq!(
            descriptions(&package_commands(&input, &["org.videolan.VLC".to_owned()])),
            ["flatpak install --noninteractive flathub 'x; reboot'"]
        );

        input.present = false;
        input.user = true;
        assert_eq!(
            descriptions(&package_commands(&input, &["org.videolan.VLC".to_owned()])),
            ["flatpak uninstall --noninteractive --user org.videolan.VLC"]
        );
    }
}
