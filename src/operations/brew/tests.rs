use indexmap::IndexMap;

use super::{
    CasksInput, PackagesInput, TapInput, TrustInput, cask_commands, package_commands, tap_commands, trust_commands,
};
use crate::api::Command;
use crate::facts::brew::BrewItemKind;
use crate::operations::packages::{PackageCommands, PackagePolicy, converge_packages};

fn descriptions(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Shell(value) => Some(value.raw_value()),
            _ => None,
        })
        .collect()
}

#[test]
fn package_and_cask_commands_match_old_and_new_cli_fixtures() {
    let mut packages = PackagesInput::new(["php"]);
    packages.update = true;
    packages.upgrade = true;
    assert_eq!(
        descriptions(&package_commands(&packages, &IndexMap::from([("php".to_owned(), vec!["1".to_owned()])]))),
        ["brew update", "brew upgrade"]
    );
    let casks = CasksInput::new(["keepassxc"]);
    let (install, uninstall, upgrade) = cask_commands([2, 6, 0]);
    assert_eq!(
        descriptions(&converge_packages(
            &casks.casks,
            &IndexMap::new(),
            PackagePolicy { present: true, latest: false },
            &PackageCommands { install, uninstall, upgrade, version_join: "@" }
        )),
        ["brew install --cask keepassxc"]
    );
}

#[test]
fn tap_and_trust_commands_match_reference_state_transitions() {
    let input = TapInput { source: Some("homebrew/cask".to_owned()), present: true, trusted: true, url: None };
    assert_eq!(
        descriptions(&tap_commands(&input, "homebrew/cask", false, false)),
        ["brew tap homebrew/cask", "brew trust --tap homebrew/cask"]
    );
    let trusted = IndexMap::from([("commands".to_owned(), vec!["bar".to_owned()])]);
    let trust = TrustInput {
        items: vec!["foo".to_owned(), "bar".to_owned(), "baz".to_owned()],
        kind: BrewItemKind::Command,
        trusted: true,
    };
    assert_eq!(
        descriptions(&trust_commands(&trust, &trusted)),
        ["brew trust --command baz", "brew trust --command foo"]
    );
}
