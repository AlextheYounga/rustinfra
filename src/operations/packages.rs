use indexmap::IndexMap;

use crate::api::{Command, CommandBit, StringCommand};

pub type PackageMap = IndexMap<String, Vec<String>>;

pub struct PackageCommands<'a> {
    pub install: Vec<CommandBit>,
    pub uninstall: Vec<CommandBit>,
    pub upgrade: Vec<CommandBit>,
    pub version_join: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackagePolicy {
    pub present: bool,
    pub latest: bool,
}

pub fn converge_packages(
    packages: &[String],
    installed: &PackageMap,
    policy: PackagePolicy,
    commands: &PackageCommands<'_>,
) -> Vec<Command> {
    let specs = packages.iter().map(|package| PackageSpec::parse(package, commands.version_join));
    let mut changed = Vec::new();
    let mut upgrades = Vec::new();

    for spec in specs {
        let versions = installed.get(spec.name);
        let installed = versions.is_some_and(|versions| {
            spec.version.is_none_or(|version| versions.iter().any(|installed| installed == version))
        });
        if policy.present && !installed || !policy.present && installed {
            changed.push(spec.original);
        } else if policy.present && policy.latest && spec.version.is_none() && installed {
            upgrades.push(spec.original);
        }
    }

    let mut result = Vec::new();
    if !changed.is_empty() {
        let prefix = if policy.present { &commands.install } else { &commands.uninstall };
        result.push(shell_command(prefix, &changed));
    }
    if !upgrades.is_empty() {
        result.push(shell_command(&commands.upgrade, &upgrades));
    }
    result
}

fn shell_command(prefix: &[CommandBit], packages: &[&str]) -> Command {
    let bits = prefix.iter().cloned().chain(packages.iter().map(|package| CommandBit::quoted(*package)));
    Command::Shell(StringCommand::new(bits))
}

struct PackageSpec<'a> {
    original: &'a str,
    name: &'a str,
    version: Option<&'a str>,
}

impl<'a> PackageSpec<'a> {
    fn parse(original: &'a str, version_join: &str) -> Self {
        let (name, version) = original
            .rsplit_once(version_join)
            .filter(|(name, version)| !name.is_empty() && !version.is_empty())
            .map_or((original, None), |(name, version)| (name, Some(version)));
        Self { original, name, version }
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{PackageCommands, PackagePolicy, converge_packages};
    use crate::api::{Command, CommandBit};

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
    fn separates_missing_removed_and_latest_packages() {
        let installed = IndexMap::from([
            ("git".to_owned(), vec!["1.0".to_owned()]),
            ("react".to_owned(), vec!["1.0".to_owned()]),
            ("test".to_owned(), vec!["1.1".to_owned()]),
        ]);
        let commands = PackageCommands {
            install: vec![CommandBit::from("tool install")],
            uninstall: vec![CommandBit::from("tool remove")],
            upgrade: vec![CommandBit::from("tool upgrade")],
            version_join: "@",
        };

        assert_eq!(
            descriptions(&converge_packages(
                &["git@1.1".to_owned(), "react".to_owned(), "new; id".to_owned()],
                &installed,
                PackagePolicy { present: true, latest: true },
                &commands,
            )),
            ["tool install git@1.1 'new; id'", "tool upgrade react"]
        );
        assert_eq!(
            descriptions(&converge_packages(
                &["git@1.1".to_owned(), "react".to_owned(), "test@1.1".to_owned()],
                &installed,
                PackagePolicy { present: false, latest: false },
                &commands,
            )),
            ["tool remove react test@1.1"]
        );
    }
}
