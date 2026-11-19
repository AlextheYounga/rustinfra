use std::ffi::OsString;
use std::io;

use tokio::process::Command;
use tokio::time;

use crate::api::{CommandBit, RsyncCommand, StringCommand};
use crate::connectors::{RunOptions, UnixCommandOptions, make_unix_command};

use super::options::{HostKeyPolicy, SshConnectionOptions};
use super::session::SshConnectorError;

pub(super) async fn run(
    command: &RsyncCommand,
    options: &RunOptions,
    connection: &SshConnectionOptions,
) -> Result<(), SshConnectorError> {
    if connection.password.is_some() {
        return Err(SshConnectorError::RsyncUnsupported("SSH passwords"));
    }
    if connection.key_password.is_some() {
        return Err(SshConnectorError::RsyncUnsupported("password-protected SSH keys"));
    }

    let arguments = arguments(command, options, connection);
    let process = Command::new("rsync").args(arguments).kill_on_drop(true).output();
    let output = match options.timeout {
        Some(timeout) => time::timeout(timeout, process)
            .await
            .map_err(|_| SshConnectorError::Timeout(timeout))?
            .map_err(map_process_error)?,
        None => process.await.map_err(map_process_error)?,
    };
    if output.status.success() {
        Ok(())
    } else {
        Err(SshConnectorError::RsyncFailed(String::from_utf8_lossy(&output.stderr).into_owned()))
    }
}

fn map_process_error(error: io::Error) -> SshConnectorError {
    if error.kind() == io::ErrorKind::NotFound {
        SshConnectorError::RsyncUnavailable
    } else {
        SshConnectorError::TransferIo(error)
    }
}

fn arguments(command: &RsyncCommand, options: &RunOptions, connection: &SshConnectionOptions) -> Vec<OsString> {
    let mut arguments = command.flags.iter().map(OsString::from).collect::<Vec<_>>();
    arguments.extend([OsString::from("--rsh"), OsString::from(ssh_command(connection))]);
    arguments.extend([OsString::from("--rsync-path"), OsString::from(remote_rsync_command(options))]);
    arguments.push(command.source.as_os_str().to_owned());
    arguments.push(OsString::from(format!("{}@{}:{}", connection.user, connection.host_alias, command.destination)));
    arguments
}

fn ssh_command(connection: &SshConnectionOptions) -> String {
    let mut arguments = vec!["ssh".to_owned(), "-o".to_owned(), "BatchMode=yes".to_owned()];
    if let Some(path) = connection.known_hosts_files.first() {
        arguments.extend(["-o".to_owned(), format!("UserKnownHostsFile={}", path.display())]);
    }
    arguments
        .extend(["-o".to_owned(), format!("StrictHostKeyChecking={}", host_key_policy(connection.host_key_policy))]);
    if let Some(path) = &connection.config_file {
        arguments.extend(["-F".to_owned(), path.display().to_string()]);
    }
    if connection.port != 22 {
        arguments.extend(["-p".to_owned(), connection.port.to_string()]);
    }
    if let Some(path) = &connection.key {
        arguments.extend(["-i".to_owned(), path.display().to_string()]);
    }
    arguments.into_iter().map(|value| quote(&value)).collect::<Vec<_>>().join(" ")
}

fn remote_rsync_command(options: &RunOptions) -> String {
    let mut remote = UnixCommandOptions {
        shell_executable: None,
        environment: Default::default(),
        working_directory: None,
        su_user: None,
        doas: false,
        dzdo: false,
        ..options.unix.clone()
    };
    remote.su_password = None;
    make_unix_command(StringCommand::new(["rsync"]), &remote).raw_value()
}

fn host_key_policy(policy: HostKeyPolicy) -> &'static str {
    match policy {
        HostKeyPolicy::Ask => "ask",
        HostKeyPolicy::Accept => "no",
        HostKeyPolicy::Strict => "yes",
        HostKeyPolicy::AcceptNew => "accept-new",
    }
}

fn quote(value: &str) -> String {
    StringCommand::new([CommandBit::quoted(value)]).raw_value()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::time::Duration;

    use serde_json::json;

    use super::arguments;
    use crate::api::{Data, RsyncCommand};
    use crate::connectors::{RunOptions, SshConnectionOptions};

    #[test]
    fn arguments_preserve_pyinfra_ssh_flags_and_quote_nested_values() {
        let data = [
            ("ssh_user".to_owned(), json!("deploy")),
            ("ssh_port".to_owned(), json!(2202)),
            ("ssh_key".to_owned(), json!("/tmp/key file")),
            ("ssh_config_file".to_owned(), json!("/tmp/config file")),
            ("ssh_strict_host_key_checking".to_owned(), json!("no")),
        ]
        .into_iter()
        .collect::<Data>();
        let connection = SshConnectionOptions::from_data("target-alias", &data, Duration::from_secs(10)).unwrap();
        let command = RsyncCommand {
            source: "/local/source path".into(),
            destination: "/remote/path;touch bad".to_owned(),
            flags: vec!["-ax".to_owned(), "--delete".to_owned()],
        };
        let mut options = RunOptions::default();
        options.unix.sudo = true;
        options.unix.sudo_user = Some("root".to_owned());

        let actual = arguments(&command, &options, &connection);

        assert_eq!(
            actual,
            vec![
                OsString::from("-ax"),
                OsString::from("--delete"),
                OsString::from("--rsh"),
                OsString::from(
                    "ssh -o BatchMode=yes -o StrictHostKeyChecking=no -F '/tmp/config file' -p 2202 -i '/tmp/key file'"
                ),
                OsString::from("--rsync-path"),
                OsString::from("sudo -H -n -u root rsync"),
                OsString::from("/local/source path"),
                OsString::from("deploy@target-alias:/remote/path;touch bad"),
            ]
        );
    }
}
