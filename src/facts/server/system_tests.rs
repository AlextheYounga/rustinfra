use super::*;

#[test]
fn uptime_commands_and_parsing_match_linux_and_bsd() {
    let linux = KernelInput { kernel: "Linux".to_owned() };
    assert_eq!(Uptime.command(&linux).raw_value(), "cut -d. -f1 /proc/uptime");
    assert_eq!(Uptime.process(&["86400".to_owned()], &linux).unwrap(), 86_400);
    let darwin = KernelInput { kernel: "Darwin".to_owned() };
    assert_eq!(Uptime.command(&darwin).raw_value(), "date +%s; sysctl -n kern.boottime | awk '{print $4}' | tr -d ','");
    assert_eq!(Uptime.process(&["1746748800".to_owned(), "1746662400".to_owned()], &darwin).unwrap(), 86_400);
}

#[test]
fn authorized_keys_and_hosts_quote_paths_and_parse_reference_shapes() {
    let input = AuthorizedKeysInput { user: HomeInput::try_from("alice").unwrap(), path: None };
    assert_eq!(AuthorizedKeys.command(&input).raw_value(), "cat ~alice/.ssh/authorized_keys 2>/dev/null || true");
    assert_eq!(
        AuthorizedKeys.process(&["# comment".to_owned(), " ssh-ed25519 key user ".to_owned()], &input).unwrap(),
        ["ssh-ed25519 key user"]
    );
    let custom = AuthorizedKeysInput {
        user: HomeInput::try_from("alice").unwrap(),
        path: Some("/etc/ssh/keys/alice".to_owned()),
    };
    assert_eq!(AuthorizedKeys.command(&custom).raw_value(), "cat /etc/ssh/keys/alice 2>/dev/null || true");
    assert!(AuthorizedKeys.process(&[], &custom).unwrap().is_empty());
    let path = Some("/tmp/host file".to_owned());
    assert_eq!(EtcHosts.command(&path).raw_value(), "cat '/tmp/host file' 2>/dev/null || true");
    let hosts = EtcHosts
        .process(&["127.0.0.1 localhost localhost".to_owned(), "127.0.0.1 local # note".to_owned()], &None)
        .unwrap();
    assert_eq!(hosts["127.0.0.1"], ["localhost", "local"]);
}

#[test]
fn release_gui_selinux_limits_and_reboot_match_fixtures() {
    let release = LsbRelease
        .process(
            &[
                "No LSB modules are available.".to_owned(),
                "Distributor ID: Ubuntu".to_owned(),
                "Description: Ubuntu 18.04.3 LTS".to_owned(),
                "Release: 18.04".to_owned(),
                "Codename: bionic".to_owned(),
            ],
            &(),
        )
        .unwrap();
    assert_eq!(release["id"], "Ubuntu");
    assert_eq!(release["codename"], "bionic");
    assert_eq!(
        LinuxName.process(&["/etc/os-release".to_owned(), "NAME=\"Ubuntu\"".to_owned()], &()).unwrap().as_deref(),
        Some("Ubuntu")
    );
    assert_eq!(LinuxGui.process(&["/usr/bin/gnome-session".to_owned()], &()).unwrap(), ["GNOME"]);
    assert!(HasGui.process(&["/usr/bin/gnome-session".to_owned()], &()).unwrap());
    assert!(!HasGui.process(&[], &()).unwrap());
    assert_eq!(Selinux.process(&["SELinux status: enabled".to_owned()], &()).unwrap().mode.as_deref(), Some("enabled"));
    let limits = SecurityLimits.process(&["# comment".to_owned(), "* soft nofile 104857".to_owned()], &()).unwrap();
    assert_eq!(limits[0].item, "nofile");
    assert!(RebootRequired.process(&["reboot_required".to_owned()], &()).unwrap());
    assert!(!RebootRequired.process(&["no_reboot_required".to_owned()], &()).unwrap());
    assert_eq!(Selinux.process(&[String::new()], &()).unwrap(), SelinuxInfo::default());
}

#[test]
fn linux_name_matches_distribution_command_and_ubuntu_output() {
    assert_eq!(LinuxName.command(&()).raw_value(), LinuxDistribution.command(&()).raw_value());
    assert_eq!(
        LinuxName.process(&["/etc/os-release".to_owned(), "NAME=\"Ubuntu\"".to_owned()], &()).unwrap().as_deref(),
        Some("Ubuntu")
    );
}
