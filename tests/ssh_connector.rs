use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{self, Server as _};
use russh::{Channel, ChannelId, Error as RusshError};
use russh_sftp::protocol::{Data, FileAttributes, Handle, OpenFlags, Status, StatusCode};
use russh_sftp::server::{Handler as SftpHandler, run as run_sftp_server};
use rustinfra::api::{HiddenValue, StringCommand};
use rustinfra::connectors::{
    Connector, FileTransferOptions, FileTransferProtocol, HostKeyPolicy, RunOptions, SshConnectionOptions, SshConnector,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, split};
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;

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

struct MemorySftp {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl SftpHandler for MemorySftp {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        flags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let mut files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        if flags.contains(OpenFlags::CREATE) {
            files.entry(filename.clone()).or_default();
        }
        if flags.contains(OpenFlags::TRUNCATE) {
            files.insert(filename.clone(), Vec::new());
        }
        if !files.contains_key(&filename) {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Handle { id, handle: filename })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        let mut files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        let file = files.get_mut(&handle).ok_or(StatusCode::NoSuchFile)?;
        let offset = usize::try_from(offset).map_err(|_| StatusCode::Failure)?;
        file.resize(file.len().max(offset + data.len()), 0);
        file[offset..offset + data.len()].copy_from_slice(&data);
        Ok(ok_status(id))
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, Self::Error> {
        let files = self.files.lock().map_err(|_| StatusCode::Failure)?;
        let file = files.get(&handle).ok_or(StatusCode::NoSuchFile)?;
        let offset = usize::try_from(offset).map_err(|_| StatusCode::Failure)?;
        if offset >= file.len() {
            return Err(StatusCode::Eof);
        }
        let len = usize::try_from(len).map_err(|_| StatusCode::Failure)?;
        let end = file.len().min(offset.saturating_add(len));
        Ok(Data { id, data: file[offset..end].to_vec() })
    }

    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        Ok(ok_status(id))
    }
}

fn ok_status(id: u32) -> Status {
    Status { id, status_code: StatusCode::Ok, error_message: "Ok".to_owned(), language_tag: "en-US".to_owned() }
}

async fn run_scp_server(channel: Channel<server::Msg>, command: &str, files: Arc<Mutex<HashMap<String, Vec<u8>>>>) {
    let stream = channel.into_stream();
    let (reader, mut writer) = split(stream);
    let mut reader = BufReader::new(reader);
    if let Some(path) = command.strip_prefix("scp -t ") {
        if writer.write_all(&[0]).await.is_err() {
            return;
        }
        let mut header = String::new();
        if reader.read_line(&mut header).await.is_err() {
            return;
        }
        let size = header.split_whitespace().nth(1).and_then(|value| value.parse::<usize>().ok());
        let Some(size) = size else {
            return;
        };
        if writer.write_all(&[0]).await.is_err() {
            return;
        }
        let mut content = vec![0; size];
        if reader.read_exact(&mut content).await.is_err() {
            return;
        }
        let mut terminator = [1];
        if reader.read_exact(&mut terminator).await.is_err() || terminator != [0] {
            return;
        }
        if let Ok(mut remote_files) = files.lock() {
            remote_files.insert(path.to_owned(), content);
        }
        let _ = writer.write_all(&[0]).await;
    } else if let Some(path) = command.strip_prefix("scp -f ") {
        let mut confirmation = [1];
        if reader.read_exact(&mut confirmation).await.is_err() || confirmation != [0] {
            return;
        }
        let content = files.lock().ok().and_then(|remote_files| remote_files.get(path).cloned());
        let Some(content) = content else {
            let _ = writer.write_all(b"\x01missing\n").await;
            return;
        };
        let basename = path.rsplit('/').next().unwrap_or(path);
        if writer.write_all(format!("C0644 {} {basename}\n", content.len()).as_bytes()).await.is_err() {
            return;
        }
        if reader.read_exact(&mut confirmation).await.is_err() || confirmation != [0] {
            return;
        }
        if writer.write_all(&content).await.is_err() || writer.write_all(&[0]).await.is_err() {
            return;
        }
        let _ = reader.read_exact(&mut confirmation).await;
    }
    let _ = writer.shutdown().await;
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
        key_password: None,
        allow_agent: false,
        look_for_keys: false,
        known_hosts_file: None,
        host_key_policy: HostKeyPolicy::Accept,
        connect_timeout: Duration::from_secs(2),
        server_alive_interval: None,
        connect_retries: 0,
        connect_retry_min_delay: Duration::ZERO,
        connect_retry_max_delay: Duration::ZERO,
        file_transfer_protocol: FileTransferProtocol::Sftp,
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
        key_password: None,
        allow_agent: false,
        look_for_keys: false,
        known_hosts_file: None,
        host_key_policy: HostKeyPolicy::Accept,
        connect_timeout: Duration::from_secs(2),
        server_alive_interval: None,
        connect_retries: 0,
        connect_retry_min_delay: Duration::ZERO,
        connect_retry_max_delay: Duration::ZERO,
        file_transfer_protocol: FileTransferProtocol::Scp,
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

    server_task.abort();
    assert!(server_task.await.unwrap_err().is_cancelled());
}
