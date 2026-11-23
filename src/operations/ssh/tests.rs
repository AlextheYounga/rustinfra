use super::*;

fn raw_commands(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Shell(command) => Some(command.raw_value()),
            _ => None,
        })
        .collect()
}

#[test]
fn keyscan_matches_exact_hosts_markers_ports_and_quoting() {
    let mut input = KeyscanInput::new("remote-host.net");
    input.port = "2222".to_owned();
    let other_ports =
        vec!["remote-host.net ssh-ed25519 AAAA".to_owned(), "[remote-host.net]:22222 ssh-ed25519 AAAA".to_owned()];
    assert_eq!(
        raw_commands(&keyscan_plan(&input, "/home/a/.ssh/known_hosts", &other_ports)),
        ["ssh-keyscan -p 2222 remote-host.net >> /home/a/.ssh/known_hosts"]
    );
    let present = vec!["@cert-authority alias.net,[remote-host.net]:2222 ssh-ed25519 AAAA".to_owned()];
    assert!(keyscan_plan(&input, "/home/a/.ssh/known_hosts", &present).is_empty());
    input.force = true;
    assert_eq!(
        raw_commands(&keyscan_plan(&input, "/home/a/.ssh/known_hosts", &present))[0],
        "ssh-keygen -R '[remote-host.net]:2222'"
    );
}

#[test]
fn command_and_upload_quote_all_user_controlled_values() {
    let command = CommandInput {
        hostname: "evil; rm".to_owned(),
        command: "echo hi".to_owned(),
        user: Some("admin; id".to_owned()),
        port: "22;x".to_owned(),
    };
    assert_eq!(raw_commands(&SshCommand.commands(&command)), ["ssh -p '22;x' 'admin; id@evil; rm' 'echo hi'"]);
    let mut upload = UploadInput::new("remote-host.net", "my file; rm");
    upload.user = Some("admin; id".to_owned());
    assert_eq!(
        raw_commands(&upload_commands(&upload, "/tmp")),
        ["scp -P 22 'my file; rm' 'admin; id@remote-host.net':'my file; rm'"]
    );
}

#[test]
fn remote_sudo_upload_uses_a_safe_temporary_handoff() {
    let mut input = UploadInput::new("remote-host.net", "local file");
    input.remote_filename = Some("/root/remote file".to_owned());
    input.use_remote_sudo = true;
    let commands = raw_commands(&upload_commands(&input, "/tmp"));
    assert_eq!(commands.len(), 2);
    assert!(commands[0].starts_with("scp -P 22 'local file' remote-host.net:/tmp/pyinfra-"));
    assert!(commands[1].contains("'sudo mv /tmp/pyinfra-"));
    assert!(commands[1].contains("/root/remote file"));
}
