use std::io;
use std::iter::once;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::task::{Context, Poll};

use russh::client;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time;

use super::auth;
use super::options::SshConnectionOptions;
use super::session::{HandlerError, ServerKeyHandler, SshConnectorError, SshHandle};

pub(super) async fn connect(options: &SshConnectionOptions) -> Result<(SshHandle, Vec<SshHandle>), SshConnectorError> {
    if options.proxy_jump.is_empty() {
        return Ok((connect_direct(options).await?, Vec::new()));
    }

    let mut jumps = Vec::with_capacity(options.proxy_jump.len());
    let mut current = connect_direct(&options.proxy_jump[0]).await?;
    for (jump, next) in options.proxy_jump.iter().zip(options.proxy_jump.iter().skip(1).chain(once(options))) {
        let channel = time::timeout(
            jump.proxy_jump_channel_timeout,
            current.channel_open_direct_tcpip(&next.hostname, u32::from(next.port), "127.0.0.1", 0),
        )
        .await
        .map_err(|_| SshConnectorError::Timeout(jump.proxy_jump_channel_timeout))??;
        let next_session = connect_stream(next, channel.into_stream()).await?;
        jumps.push(current);
        current = next_session;
    }
    Ok((current, jumps))
}

async fn connect_direct(options: &SshConnectionOptions) -> Result<SshHandle, SshConnectorError> {
    if let Some(command) = &options.proxy_command {
        let stream = ProxyCommandStream::spawn(command, options)?;
        return connect_stream(options, stream).await;
    }
    let connection = client::connect(
        client_config(options),
        (options.hostname.as_str(), options.port),
        ServerKeyHandler::for_options(options),
    );
    let mut session = time::timeout(options.connect_timeout, connection)
        .await
        .map_err(|_| SshConnectorError::Timeout(options.connect_timeout))?
        .map_err(map_handler_error)?;
    authenticate(options, &mut session).await?;
    Ok(session)
}

async fn connect_stream<R>(options: &SshConnectionOptions, stream: R) -> Result<SshHandle, SshConnectorError>
where
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let connection = client::connect_stream(client_config(options), stream, ServerKeyHandler::for_options(options));
    let mut session = time::timeout(options.connect_timeout, connection)
        .await
        .map_err(|_| SshConnectorError::Timeout(options.connect_timeout))?
        .map_err(map_handler_error)?;
    authenticate(options, &mut session).await?;
    Ok(session)
}

async fn authenticate(options: &SshConnectionOptions, session: &mut SshHandle) -> Result<(), SshConnectorError> {
    if auth::authenticate(options, session).await? {
        Ok(())
    } else {
        Err(SshConnectorError::Authentication { user: options.user.clone(), host: options.hostname.clone() })
    }
}

fn client_config(options: &SshConnectionOptions) -> Arc<client::Config> {
    Arc::new(client::Config { keepalive_interval: options.server_alive_interval, ..client::Config::default() })
}

fn map_handler_error(error: HandlerError) -> SshConnectorError {
    match error {
        HandlerError::Protocol(error) => SshConnectorError::Protocol(error),
        HandlerError::HostKey(error) => SshConnectorError::Key(error),
    }
}

struct ProxyCommandStream {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl ProxyCommandStream {
    fn spawn(command: &[String], options: &SshConnectionOptions) -> Result<Self, SshConnectorError> {
        let Some((program, arguments)) = command.split_first() else {
            return Err(SshConnectorError::Proxy("empty ProxyCommand".to_owned()));
        };
        let mut child = Command::new(expand_token(program, &options.hostname, options.port, &options.user))
            .args(
                arguments.iter().map(|argument| expand_token(argument, &options.hostname, options.port, &options.user)),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| SshConnectorError::Proxy(error.to_string()))?;
        let stdin = child.stdin.take().ok_or_else(|| SshConnectorError::Proxy("missing command stdin".to_owned()))?;
        let stdout =
            child.stdout.take().ok_or_else(|| SshConnectorError::Proxy("missing command stdout".to_owned()))?;
        Ok(Self { child, stdin, stdout })
    }
}

impl Drop for ProxyCommandStream {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl AsyncRead for ProxyCommandStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdout).poll_read(context, buffer)
    }
}

impl AsyncWrite for ProxyCommandStream {
    fn poll_write(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stdin).poll_write(context, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdin).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdin).poll_shutdown(context)
    }
}

fn expand_token(token: &str, hostname: &str, port: u16, user: &str) -> String {
    token
        .replace("%%", "\0")
        .replace("%h", hostname)
        .replace("%p", &port.to_string())
        .replace("%r", user)
        .replace('\0', "%")
}

#[cfg(test)]
mod tests {
    use super::expand_token;

    #[test]
    fn proxy_command_tokens_expand_like_openssh() {
        assert_eq!(expand_token("%r@%h:%p-%%", "target.example", 2202, "deploy"), "deploy@target.example:2202-%");
    }
}
