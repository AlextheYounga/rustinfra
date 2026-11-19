use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{self, Server as _};
use russh::{Channel, ChannelId, Error as RusshError};
use rustinfra::api::{HiddenValue, StringCommand};
use rustinfra::connectors::{Connector, HostKeyPolicy, RunOptions, SshConnectionOptions, SshConnector};
use tokio::net::TcpListener;

#[derive(Clone)]
struct CommandServer {
    commands: Arc<Mutex<Vec<String>>>,
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
        _channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.commands.lock().map_err(|_| RusshError::Disconnect)?.push(String::from_utf8_lossy(data).into_owned());
        session.channel_success(channel)?;
        session.data(channel, &b"out\n"[..])?;
        session.extended_data(channel, 1, &b"err\n"[..])?;
        session.exit_status_request(channel, 7)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }
}

#[tokio::test]
async fn persistent_session_runs_commands_on_independent_channels() {
    let commands = Arc::new(Mutex::new(Vec::new()));
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
    let server_task = tokio::spawn(async move {
        let mut server = CommandServer { commands: server_commands };
        server.run_on_socket(config, &listener).await
    });

    let connector = SshConnector::new(SshConnectionOptions {
        hostname: "127.0.0.1".to_owned(),
        port,
        user: "deploy".to_owned(),
        password: Some(HiddenValue::new("secret")),
        key: None,
        key_password: None,
        allow_agent: false,
        look_for_keys: false,
        known_hosts_file: None,
        host_key_policy: HostKeyPolicy::Accept,
        connect_timeout: Duration::from_secs(2),
        connect_retries: 0,
        connect_retry_min_delay: Duration::ZERO,
        connect_retry_max_delay: Duration::ZERO,
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
    let commands = commands.lock().unwrap().clone();
    assert!(commands.contains(&"sh -c 'printf first'".to_owned()));
    assert!(commands.contains(&"sh -c 'printf second'".to_owned()));

    connector.disconnect().await.unwrap();
    server_task.abort();
    assert!(server_task.await.unwrap_err().is_cancelled());
}
