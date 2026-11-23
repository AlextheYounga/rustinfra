use super::*;

#[test]
fn installed_and_held_packages_match_reference_formats() {
    let installed = AptPackages
        .process(
            &[
                "Listing...".to_owned(),
                "docker-ce/jammy,now 5:24.0.7 amd64 [installed]".to_owned(),
                "libc6:i386/jammy,now 2.35 i386 [installed,automatic]".to_owned(),
            ],
            &(),
        )
        .unwrap();
    assert_eq!(installed["docker-ce"], ["5:24.0.7"]);
    assert_eq!(installed["libc6:i386"], ["2.35"]);
    assert_eq!(AptHeldPackages.process(&["nomad".to_owned(), " ".to_owned()], &()).unwrap(), ["nomad"]);
}

#[test]
fn virtual_package_providers_start_after_reverse_provides() {
    let output =
        vec!["Provides:".to_owned(), "Reverse Provides:".to_owned(), "fortune-mod 1:1.99.1-7.3 (= )".to_owned()];
    assert_eq!(
        AptPackageProvides.process(&output, &"fortune".to_owned()).unwrap(),
        [("fortune-mod".to_owned(), "1:1.99.1-7.3".to_owned())]
    );
    assert_eq!(AptPackageProvides.command(&"bad; value".to_owned()).raw_value(), "apt-cache showpkg 'bad; value'");
}

#[test]
fn simulation_parses_summary_and_quotes_action() {
    let action = "upgrade".to_owned();
    assert_eq!(
        SimulateOperationWillChange
            .process(&["15 upgraded, 2 newly installed, 3 to remove and 4 not upgraded.".to_owned()], &action)
            .unwrap(),
        Some(AptSimulation { upgraded: 15, newly_installed: 2, removed: 3, not_upgraded: 4 })
    );
    assert!(SimulateOperationWillChange.process(&["unexpected".to_owned()], &action).is_err());
    assert!(
        SimulateOperationWillChange.command(&"bad; action".to_owned()).raw_value().contains("'bad; action' --dry-run")
    );
}

#[test]
fn apt_keys_reuses_keyring_parsing_and_flattens_keys() {
    let output = vec![
        "KEYRING:/etc/apt/keyrings/vendor.gpg".to_owned(),
        "pub:-:4096:1:ABC123:1:::::scSC::::::23::0:".to_owned(),
        "fpr:::::::::FINGERPRINT:".to_owned(),
        "uid:-::::1::HASH::Vendor Key::::::::::0:".to_owned(),
    ];
    let keys = AptKeys.process(&output, &None).unwrap();
    assert_eq!(keys["ABC123"].fingerprint.as_deref(), Some("FINGERPRINT"));
    assert!(AptKeys.command(&None).raw_value().contains("/etc/apt/keyrings"));
}
