use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[path = "ssh_connector/support.rs"]
mod ssh_connector_support;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{self, Server as _};
use russh::{Channel, ChannelId, Error as RusshError};
use russh_sftp::server::run as run_sftp_server;
use rustinfra::api::{Data, HiddenValue, StringCommand};
use rustinfra::connectors::{
    Connector, FileTransferOptions, FileTransferProtocol, HostKeyPolicy, RunOptions, SshConnectionOptions, SshConnector,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex as AsyncMutex;

use ssh_connector_support::{MemorySftp, run_scp_server};

#[derive(Clone)]
struct CommandServer {
    commands: Arc<Mutex<Vec<String>>>,
    channels: Arc<AsyncMutex<HashMap<ChannelId, Channel<server::Msg>>>>,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl server::Server for CommandServer {
    type Handler = Self;

    fn new_client(&mut self, _peer_addr: Option<SocketAddr>) -> Self {
        self.clone()
    }
}

impl server::Handler for CommandServer {
    type Error = RusshError;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<server::Auth, Self::Error> {
        if user == "deploy" && password == "secret" {
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::Reject { proceed_with_methods: None, partial_success: false })
        }
    }

    async fn auth_none(&mut self, user: &str) -> Result<server::Auth, Self::Error> {
        if user == "deploy" {
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::Reject { proceed_with_methods: None, partial_success: false })
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.channels.lock().await.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        let Ok(port) = u16::try_from(port_to_connect) else {
            return Ok(());
        };
        let Ok(mut socket) = TcpStream::connect((host_to_connect, port)).await else {
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = copy_bidirectional(&mut stream, &mut socket).await;
        });
        Ok(())
    }

    async fn agent_request(&mut self, channel: ChannelId, session: &mut server::Session) -> Result<bool, Self::Error> {
        session.channel_success(channel)?;
        Ok(true)
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).into_owned();
        self.commands.lock().map_err(|_| RusshError::Disconnect)?.push(command.clone());
        session.channel_success(channel)?;
        if command.starts_with("scp -t ") || command.starts_with("scp -f ") {
            let Some(scp_channel) = self.channels.lock().await.remove(&channel) else {
                return Err(RusshError::Disconnect);
            };
            let files = Arc::clone(&self.files);
            tokio::spawn(async move {
                run_scp_server(scp_channel, &command, files).await;
            });
            return Ok(());
        }
        if command.contains("agent-forward") {
            let handle = session.handle();
            tokio::spawn(async move {
                let output = match handle.channel_open_agent().await {
                    Ok(channel) => {
                        let mut stream = channel.into_stream();
                        let mut response = [0; 4];
                        if stream.write_all(b"ping").await.is_ok() && stream.read_exact(&mut response).await.is_ok() {
                            response.to_vec()
                        } else {
                            b"agent I/O failed".to_vec()
                        }
                    }
                    Err(_) => b"agent channel failed".to_vec(),
                };
                let _ = handle.data(channel, output).await;
                let _ = handle.exit_status_request(channel, 0).await;
                let _ = handle.eof(channel).await;
                let _ = handle.close(channel).await;
            });
            return Ok(());
        }
        session.data(channel, &b"out\n"[..])?;
        session.extended_data(channel, 1, &b"err\n"[..])?;
        let exit_status = if command.contains("printf") { 7 } else { 0 };
        session.exit_status_request(channel, exit_status)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel)?;
            return Ok(());
        }
        let Some(channel) = self.channels.lock().await.remove(&channel) else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        session.channel_success(channel.id())?;
        let files = Arc::clone(&self.files);
        tokio::spawn(async move {
            run_sftp_server(channel.into_stream(), MemorySftp { files }).await;
        });
        Ok(())
    }
}

#[tokio::test]
async fn persistent_session_runs_commands_on_independent_channels() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let files = Arc::new(Mutex::new(HashMap::new()));
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let config = Arc::new(server::Config {
        keys: vec![key],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..server::Config::default()
    });
    let server_commands = Arc::clone(&commands);
    let server_files = Arc::clone(&files);
    let server_task = tokio::spawn(async move {
        let mut server = CommandServer {
            commands: server_commands,
            channels: Arc::new(AsyncMutex::new(HashMap::new())),
            files: server_files,
        };
        server.run_on_socket(config, &listener).await
    });

    let connector = SshConnector::new(SshConnectionOptions {
        hostname: "127.0.0.1".to_owned(),
        port,
        user: "deploy".to_owned(),
        password: Some(HiddenValue::new("secret")),
        key: None,
        config_identity_files: Vec::new(),
        config_certificate_file: None,
        key_password: None,
        allow_agent: false,
        identity_agent: None,
        look_for_keys: false,
        forward_agent: false,
        known_hosts_files: Vec::new(),
        host_key_policy: HostKeyPolicy::Accept,
        connect_timeout: Duration::from_secs(2),
        server_alive_interval: None,
        connect_retries: 0,
        connect_retry_min_delay: Duration::ZERO,
        connect_retry_max_delay: Duration::ZERO,
        file_transfer_protocol: FileTransferProtocol::Sftp,
        proxy_command: None,
        proxy_jump: Vec::new(),
        proxy_jump_channel_timeout: Duration::from_secs(2),
    });
    connector.connect().await.unwrap();
    let options = RunOptions { success_exit_codes: vec![7], ..RunOptions::default() };

    let (first, second) = tokio::join!(
        connector.run_shell_command(StringCommand::new(["printf first"]), &options),
        connector.run_shell_command(StringCommand::new(["printf second"]), &options),
    );

    for result in [first.unwrap(), second.unwrap()] {
        assert!(result.success);
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.output.stdout(), "out");
        assert_eq!(result.output.stderr(), "err");
    }
    let command_snapshot = commands.lock().unwrap().clone();
    assert!(command_snapshot.contains(&"sh -c 'printf first'".to_owned()));
    assert!(command_snapshot.contains(&"sh -c 'printf second'".to_owned()));

    let local = tempfile::tempdir().unwrap();
    let upload = local.path().join("upload");
    fs::write(&upload, b"transferred").unwrap();
    let transfer_run = RunOptions::default();
    let transfer = FileTransferOptions { run: &transfer_run, remote_temporary_filename: None };
    connector.put_file(&upload, "/remote", transfer).await.unwrap();
    let download = local.path().join("download");
    connector.get_file("/remote", &download, transfer).await.unwrap();
    assert_eq!(fs::read(&download).unwrap(), b"transferred");

    let existing = local.path().join("existing");
    fs::write(&existing, b"keep me").unwrap();
    assert!(connector.get_file("/missing", &existing, transfer).await.is_err());
    assert_eq!(fs::read(existing).unwrap(), b"keep me");

    let mut privileged_run = RunOptions::default();
    privileged_run.unix.sudo = true;
    privileged_run.unix.sudo_user = Some("root".to_owned());
    let privileged = FileTransferOptions { run: &privileged_run, remote_temporary_filename: Some("/custom-temp") };
    connector.put_file(&upload, "/privileged destination", privileged).await.unwrap();
    {
        let commands = commands.lock().unwrap();
        assert!(commands.iter().any(|command| command == "sh -c 'setfacl -m u:root:r /custom-temp'"));
        assert!(commands.iter().any(|command| command.starts_with("sudo -H -n -u root sh -c 'cp /custom-temp ")));
        assert!(commands.iter().any(|command| command == "sh -c 'rm -f /custom-temp'"));
    }

    connector.disconnect().await.unwrap();

    let scp_connector = SshConnector::new(SshConnectionOptions {
        hostname: "127.0.0.1".to_owned(),
        port,
        user: "deploy".to_owned(),
        password: Some(HiddenValue::new("secret")),
        key: None,
        config_identity_files: Vec::new(),
        config_certificate_file: None,
        key_password: None,
        allow_agent: false,
        identity_agent: None,
        look_for_keys: false,
        forward_agent: false,
        known_hosts_files: Vec::new(),
        host_key_policy: HostKeyPolicy::Accept,
        connect_timeout: Duration::from_secs(2),
        server_alive_interval: None,
        connect_retries: 0,
        connect_retry_min_delay: Duration::ZERO,
        connect_retry_max_delay: Duration::ZERO,
        file_transfer_protocol: FileTransferProtocol::Scp,
        proxy_command: None,
        proxy_jump: Vec::new(),
        proxy_jump_channel_timeout: Duration::from_secs(2),
    });
    scp_connector.connect().await.unwrap();
    scp_connector.put_file(&upload, "/scp-remote", transfer).await.unwrap();
    let scp_download = local.path().join("scp-download");
    scp_connector.get_file("/scp-remote", &scp_download, transfer).await.unwrap();
    assert_eq!(fs::read(scp_download).unwrap(), b"transferred");
    let preserved = local.path().join("scp-preserved");
    fs::write(&preserved, b"preserved").unwrap();
    assert!(scp_connector.get_file("/scp-missing", &preserved, transfer).await.is_err());
    assert_eq!(fs::read(preserved).unwrap(), b"preserved");
    scp_connector.disconnect().await.unwrap();

    let config = local.path().join("ssh-config");
    fs::write(
        &config,
        format!(
            "Host jump\n  HostName 127.0.0.1\n  Port {port}\n  User deploy\n  StrictHostKeyChecking no\nHost target\n  HostName 127.0.0.1\n  Port {port}\n  User deploy\n  ProxyJump jump\n  StrictHostKeyChecking no\n"
        ),
    )
    .unwrap();
    let proxy_data = [("ssh_config_file".to_owned(), json!(config)), ("ssh_password".to_owned(), json!("secret"))]
        .into_iter()
        .collect::<Data>();
    let proxy_options = SshConnectionOptions::from_data("target", &proxy_data, Duration::from_secs(2)).unwrap();
    let proxy_connector = SshConnector::new(proxy_options);
    proxy_connector.connect().await.unwrap();
    let proxied = proxy_connector.run_shell_command(StringCommand::new(["printf proxied"]), &options).await.unwrap();
    assert!(proxied.success);
    assert_eq!(proxied.output.stdout(), "out");
    proxy_connector.disconnect().await.unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        use tokio::net::UnixListener;

        let proxy_script = local.path().join("proxy-command");
        fs::write(
            &proxy_script,
            "#!/usr/bin/env bash\n[ \"$3\" = deploy ] || exit 2\nexec 3<>\"/dev/tcp/$1/$2\" || exit 3\ncat <&3 &\nreader=$!\ncat >&3\nwait \"$reader\"\n",
        )
        .unwrap();
        fs::set_permissions(&proxy_script, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            &config,
            format!(
                "Host command\n  HostName 127.0.0.1\n  Port {port}\n  User deploy\n  ProxyCommand {} %h %p %r\n  StrictHostKeyChecking no\n",
                proxy_script.display()
            ),
        )
        .unwrap();
        let command_options = SshConnectionOptions::from_data("command", &proxy_data, Duration::from_secs(2)).unwrap();
        let command_connector = SshConnector::new(command_options);
        command_connector.connect().await.unwrap();
        let command_result =
            command_connector.run_shell_command(StringCommand::new(["printf proxy-command"]), &options).await.unwrap();
        assert!(command_result.success);
        assert_eq!(command_result.output.stdout(), "out");
        command_connector.disconnect().await.unwrap();

        let agent_path = local.path().join("agent.sock");
        let agent_listener = UnixListener::bind(&agent_path).unwrap();
        let agent_task = tokio::spawn(async move {
            let (mut stream, _) = agent_listener.accept().await.unwrap();
            let mut request = [0; 4];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(&request, b"ping");
            stream.write_all(b"pong").await.unwrap();
        });
        fs::write(
            &config,
            format!(
                "Host agent\n  HostName 127.0.0.1\n  Port {port}\n  User deploy\n  IdentityAgent {}\n  StrictHostKeyChecking no\n",
                agent_path.display()
            ),
        )
        .unwrap();
        let agent_data = [
            ("ssh_config_file".to_owned(), json!(config)),
            ("ssh_password".to_owned(), json!("secret")),
            ("ssh_allow_agent".to_owned(), json!(false)),
            ("ssh_forward_agent".to_owned(), json!(true)),
        ]
        .into_iter()
        .collect::<Data>();
        let agent_options = SshConnectionOptions::from_data("agent", &agent_data, Duration::from_secs(2)).unwrap();
        let agent_connector = SshConnector::new(agent_options);
        agent_connector.connect().await.unwrap();
        let agent_result = agent_connector
            .run_shell_command(StringCommand::new(["agent-forward"]), &RunOptions::default())
            .await
            .unwrap();
        assert!(agent_result.success);
        assert_eq!(agent_result.output.stdout(), "pong");
        agent_connector.disconnect().await.unwrap();
        agent_task.await.unwrap();
    }

    server_task.abort();
    assert!(server_task.await.unwrap_err().is_cancelled());
}
