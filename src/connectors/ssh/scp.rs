use std::collections::VecDeque;
use std::num::TryFromIntError;
use std::path::Path;
use std::str::from_utf8;
use std::time::Duration;

use russh::client;
use russh::{Channel, ChannelMsg};
use tempfile::NamedTempFile;
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time;

use super::{SshConnector, SshConnectorError};
use crate::api::{CommandBit, StringCommand};

const BUFFER_SIZE: usize = 16_384;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

impl SshConnector {
    pub(super) async fn scp_upload(&self, source: &Path, destination: &str) -> Result<(), SshConnectorError> {
        let mut source = File::open(source).await?;
        let size = source.metadata().await?.len();
        let mut channel = ScpChannel::open(self, "-t", destination).await?;
        channel.confirm().await?;

        let basename = destination.rsplit('/').next().unwrap_or(destination).replace('\n', "\\^J");
        channel.send(format!("C0644 {size} {basename}\n").into_bytes()).await?;
        channel.confirm().await?;

        let mut buffer = vec![0; BUFFER_SIZE];
        loop {
            let read = source.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            channel.send(buffer[..read].to_vec()).await?;
        }
        channel.send(vec![0]).await?;
        channel.confirm().await?;
        channel.close().await
    }

    pub(super) async fn scp_download(&self, source: &str, destination: &Path) -> Result<(), SshConnectorError> {
        let mut channel = ScpChannel::open(self, "-f", source).await?;
        channel.send(vec![0]).await?;
        let header = channel.line().await?;
        if matches!(header.first(), Some(1 | 2)) {
            return Err(SshConnectorError::Scp(String::from_utf8_lossy(&header[1..]).into_owned()));
        }
        let size = parse_file_size(&header)?;
        channel.send(vec![0]).await?;

        let temporary = NamedTempFile::new()?;
        let mut staged = File::from_std(temporary.reopen()?);
        channel.copy_exact(&mut staged, size).await?;
        channel.confirm().await?;
        channel.send(vec![0]).await?;
        staged.flush().await?;
        drop(staged);
        channel.close().await?;
        fs::copy(temporary.path(), destination).await?;
        Ok(())
    }
}

struct ScpChannel {
    channel: Channel<client::Msg>,
    buffered: VecDeque<u8>,
    stderr: Vec<u8>,
}

impl ScpChannel {
    async fn open(connector: &SshConnector, mode: &str, path: &str) -> Result<Self, SshConnectorError> {
        let channel = connector.open_channel().await?;
        let command = StringCommand::new(vec![
            CommandBit::from("scp"),
            CommandBit::from(mode.to_owned()),
            CommandBit::quoted(path.to_owned()),
        ]);
        channel.exec(true, command.raw_value()).await?;
        Ok(Self { channel, buffered: VecDeque::new(), stderr: Vec::new() })
    }

    async fn send(&self, data: Vec<u8>) -> Result<(), SshConnectorError> {
        self.channel.data_bytes(data).await.map_err(SshConnectorError::from)
    }

    async fn confirm(&mut self) -> Result<(), SshConnectorError> {
        let response = self.byte().await?;
        match response {
            0 => Ok(()),
            1 | 2 => {
                let message = self.line().await?;
                Err(SshConnectorError::Scp(String::from_utf8_lossy(&message).into_owned()))
            }
            value => Err(SshConnectorError::Scp(format!("invalid response byte {value}"))),
        }
    }

    async fn line(&mut self) -> Result<Vec<u8>, SshConnectorError> {
        let mut line = Vec::new();
        loop {
            let byte = self.byte().await?;
            if byte == b'\n' {
                return Ok(line);
            }
            line.push(byte);
        }
    }

    async fn byte(&mut self) -> Result<u8, SshConnectorError> {
        while self.buffered.is_empty() {
            self.receive().await?;
        }
        self.buffered.pop_front().ok_or_else(|| SshConnectorError::Scp("server returned no data".to_owned()))
    }

    async fn copy_exact(&mut self, output: &mut File, mut remaining: u64) -> Result<(), SshConnectorError> {
        while remaining > 0 {
            while self.buffered.is_empty() {
                self.receive().await?;
            }
            let available = u64::try_from(self.buffered.len()).map_err(scp_integer_error)?;
            let count = usize::try_from(remaining.min(available)).map_err(scp_integer_error)?;
            let chunk: Vec<_> = self.buffered.drain(..count).collect();
            output.write_all(&chunk).await?;
            remaining -= u64::try_from(count).map_err(scp_integer_error)?;
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<(), SshConnectorError> {
        loop {
            let message = time::timeout(RESPONSE_TIMEOUT, self.channel.wait())
                .await
                .map_err(|_| SshConnectorError::Scp("timeout waiting for server response".to_owned()))?;
            match message {
                Some(ChannelMsg::Data { data }) => {
                    self.buffered.extend(data);
                    return Ok(());
                }
                Some(ChannelMsg::ExtendedData { data, ext: 1 }) => self.stderr.extend(data),
                Some(ChannelMsg::ExitStatus { exit_status }) if exit_status != 0 => {
                    return Err(self.closed_error(Some(exit_status)));
                }
                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => return Err(self.closed_error(None)),
                _ => {}
            }
        }
    }

    fn closed_error(&self, exit_status: Option<u32>) -> SshConnectorError {
        let stderr = String::from_utf8_lossy(&self.stderr);
        let detail = match (exit_status, stderr.is_empty()) {
            (Some(status), false) => format!("server exited with status {status}: {stderr}"),
            (Some(status), true) => format!("server exited with status {status}"),
            (None, false) => format!("server closed the channel: {stderr}"),
            (None, true) => "server closed the channel".to_owned(),
        };
        SshConnectorError::Scp(detail)
    }

    async fn close(self) -> Result<(), SshConnectorError> {
        self.channel.eof().await?;
        self.channel.close().await?;
        Ok(())
    }
}

fn parse_file_size(header: &[u8]) -> Result<u64, SshConnectorError> {
    if header.first() != Some(&b'C') {
        return Err(SshConnectorError::Scp(String::from_utf8_lossy(header).into_owned()));
    }
    let header = from_utf8(&header[1..]).map_err(|error| SshConnectorError::Scp(error.to_string()))?;
    let mut parts = header.splitn(3, ' ');
    let _mode = parts.next();
    let size = parts
        .next()
        .ok_or_else(|| SshConnectorError::Scp("bad file format".to_owned()))?
        .parse()
        .map_err(|_| SshConnectorError::Scp("bad file format".to_owned()))?;
    if parts.next().is_none() {
        return Err(SshConnectorError::Scp("bad file format".to_owned()));
    }
    Ok(size)
}

fn scp_integer_error(error: TryFromIntError) -> SshConnectorError {
    SshConnectorError::Scp(format!("file size is unsupported on this platform: {error}"))
}

#[cfg(test)]
mod tests {
    use super::parse_file_size;

    #[test]
    fn parses_openssh_file_header() {
        assert_eq!(parse_file_size(b"C0644 12 example").unwrap(), 12);
        assert!(parse_file_size(b"unexpected").is_err());
        assert!(parse_file_size(b"C0644 nope example").is_err());
    }
}
