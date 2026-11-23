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
    assert!(matches!(&commands[0], Command::Shell(command) if command.raw_value().contains("-i 'unsafe package.deb'")));
}

#[test]
fn apt_key_destinations_match_modern_keyring_rules() {
    let mut input = KeyInput::default();
    input.source = Some("https://download.example:8443/keys/vendor.asc".to_owned());
    assert_eq!(resolve_key_destination(&input).as_deref(), Some("/etc/apt/keyrings/download.example_8443-vendor.gpg"));
    input.destination = Some("custom".to_owned());
    assert_eq!(resolve_key_destination(&input).as_deref(), Some("/etc/apt/keyrings/custom.gpg"));
    input.present = false;
    input.source = None;
    input.destination = None;
    input.key_ids = vec!["ABC123".to_owned()];
    assert_eq!(resolve_key_destination(&input), None);
}
