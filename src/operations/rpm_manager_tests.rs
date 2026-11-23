use super::*;

#[test]
fn repository_content_and_package_commands_preserve_manager_options() {
    let mut repo = RepoInput::new("example");
    repo.base_url = Some("https://example/repo".to_owned());
    repo.gpg_key = Some("https://example/key".to_owned());
    assert!(
        repo_content(&repo, "example", Some("rpm-md")).unwrap().contains("type=rpm-md\ngpgkey=https://example/key\n")
    );
    let mut packages = PackagesInput::new(["vim", "unsafe; name"]);
    packages.extra_global_install_args.push(raw("--root /srv"));
    let commands = package_commands(
        &packages,
        &InstalledPackages::new(),
        &IndexMap::new(),
        PackageManagerConfig {
            command: "zypper",
            install_prefix: &["--non-interactive", "install", "-y"],
            remove_prefix: &["--non-interactive", "remove", "-y"],
            update_command: "update",
            clean_command: "clean",
            supports_no_best: false,
            expand_providers: false,
        },
    );
    assert!(
        matches!(&commands[0], Command::Shell(command) if command.raw_value() == "zypper --root /srv --non-interactive install -y vim 'unsafe; name'")
    );

    let rpm = RpmInput::new("unsafe package.rpm");
    let info = Some(RpmPackageInfo { name: "unsafe-package".to_owned(), version: "2".to_owned() });
    let installed = Some(RpmPackageInfo { name: "unsafe-package".to_owned(), version: "1".to_owned() });
    let commands = rpm_plan(
        &rpm,
        RpmPlan {
            is_url: false,
            temporary: "/tmp/download",
            package_path: &rpm.source,
            info: &info,
            installed: &installed,
            remove_command: "yum",
        },
    );
    assert!(matches!(&commands[0], Command::Shell(command) if command.raw_value() == "rpm -i 'unsafe package.rpm'"));
    let mut absent = rpm;
    absent.present = false;
    let info = Some(RpmPackageInfo { name: "unsafe;package".to_owned(), version: "1".to_owned() });
    let installed = info.clone();
    let commands = rpm_plan(
        &absent,
        RpmPlan {
            is_url: false,
            temporary: "/tmp/download",
            package_path: &absent.source,
            info: &info,
            installed: &installed,
            remove_command: "yum",
        },
    );
    assert!(matches!(&commands[0], Command::Shell(command) if command.raw_value() == "yum remove -y 'unsafe;package'"));
}
