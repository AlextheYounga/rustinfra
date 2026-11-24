use super::{
    Arch, Command, Fact, Groups, Home, HomeInput, Hostname, LoadAverage, LoadAverages, Locales, MacosVersion, Os,
    OsVersion, Timezone, TmpDir, UnsafeHomeUser, Which, command_available,
};

#[test]
fn locales_match_reference_fixture_normalization() {
    let output = ["C", "C.UTF-8", "en_US.utf8", "fr_FR.utf8", "POSIX"].map(str::to_owned);

    let locales = Locales.process(&output, &()).unwrap();

    assert_eq!(locales, ["C", "C.UTF-8", "en_US.UTF-8", "fr_FR.UTF-8", "POSIX"]);
}

#[test]
fn simple_server_facts_match_reference_commands_and_outputs() {
    assert_eq!(Hostname.command(&()).raw_value(), "uname -n");
    assert_eq!(Hostname.process(&["my-host.net".to_owned()], &()).unwrap().as_deref(), Some("my-host.net"));
    assert_eq!(Arch.command(&()).raw_value(), "uname -m");
    assert_eq!(Arch.process(&["x86_64".to_owned()], &()).unwrap().as_deref(), Some("x86_64"));
    assert_eq!(Os.command(&()).raw_value(), "uname -s");
    assert_eq!(Os.process(&["os".to_owned()], &()).unwrap().as_deref(), Some("os"));
    assert_eq!(OsVersion.command(&()).raw_value(), "uname -r");
    assert_eq!(OsVersion.process(&["os_version".to_owned()], &()).unwrap().as_deref(), Some("os_version"));
    assert_eq!(Timezone.command(&()).raw_value(), "readlink -f /etc/localtime | sed 's|.*/zoneinfo/||'");
    assert_eq!(Timezone.process(&["Europe/Amsterdam".to_owned()], &()).unwrap().as_deref(), Some("Europe/Amsterdam"));
    assert_eq!(TmpDir.command(&()).raw_value(), super::TEMP_DIRECTORY_COMMAND);
    for temporary_directory in ["/tmp", "/temp", "/usr/tmp", "/var/tmp"] {
        assert_eq!(
            TmpDir.process(&[temporary_directory.to_owned()], &()).unwrap().as_deref(),
            Some(temporary_directory)
        );
    }
    let command = "some-command".to_owned();
    assert_eq!(Command.command(&command).raw_value(), "some-command");
    assert_eq!(Command.process(&["a".to_owned(), "nop".to_owned()], &command).unwrap().as_deref(), Some("a\nnop"));
    assert_eq!(MacosVersion.requires_command(&()), Some("sw_vers"));
    assert_eq!(
        Groups
            .process(
                &[
                    "lpadmin:x:114:vagrant".to_owned(),
                    "sambashare:x:115:vagrant".to_owned(),
                    "vboxsf:x:999:".to_owned(),
                ],
                &(),
            )
            .unwrap(),
        ["lpadmin", "sambashare", "vboxsf"]
    );
}

#[test]
fn which_quotes_commands_and_returns_empty_output_for_missing_commands() {
    let python = "python".to_owned();
    assert_eq!(Which.command(&python).raw_value(), "command -v python || true");
    assert_eq!(
        Which.process(&["/usr/local/bin/python".to_owned()], &python).unwrap().as_deref(),
        Some("/usr/local/bin/python")
    );

    let unsafe_command = "py; reboot".to_owned();
    assert_eq!(Which.command(&unsafe_command).raw_value(), "command -v 'py; reboot' || true");
    assert_eq!(Which.process(&[], &unsafe_command).unwrap().as_deref(), Some(""));
    assert!(command_available(Some("/usr/local/bin/python")));
    assert!(!command_available(Some("")));
    assert!(!command_available(None));
}

#[test]
fn home_preserves_safe_tilde_expansion_and_rejects_shell_input() {
    let current = HomeInput::default();
    let user = HomeInput::try_from("some_user").unwrap();

    assert_eq!(Home.command(&current).raw_value(), "echo ~");
    assert_eq!(Home.command(&user).raw_value(), "echo ~some_user");
    assert_eq!(Home.command(&HomeInput::try_from("root").unwrap()).raw_value(), "echo ~root");
    assert_eq!(Home.process(&["/home/some_user".to_owned()], &user).unwrap().as_deref(), Some("/home/some_user"));
    assert_eq!(HomeInput::try_from("root; id"), Err(UnsafeHomeUser("root; id".to_owned())));
}

#[test]
fn load_average_parses_procfs_and_bsd_uptime_formats() {
    let procfs = LoadAverage.process(&["0.12 0.21 0.22 1/145 71536".to_owned()], &()).unwrap();
    let freebsd =
        LoadAverage.process(&["12:03PM up 16 days, 2 users, load averages: 0.12, 0.21, 0.22".to_owned()], &()).unwrap();
    let linux =
        LoadAverage.process(&["12:03:00 up 16 days, 2 users, load average: 0.12, 0.21, 0.22".to_owned()], &()).unwrap();
    let expected = Some(LoadAverages { one_minute: 0.12, five_minutes: 0.21, fifteen_minutes: 0.22 });

    assert_eq!(procfs, expected);
    assert_eq!(freebsd, expected);
    assert_eq!(linux, expected);
}
