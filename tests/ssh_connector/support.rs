use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use russh::{Channel, server};
use russh_sftp::protocol::{Data, FileAttributes, Handle, OpenFlags, Status, StatusCode};
use russh_sftp::server::Handler as SftpHandler;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, split};

pub struct MemorySftp {
    pub files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
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

pub async fn run_scp_server(channel: Channel<server::Msg>, command: &str, files: Arc<Mutex<HashMap<String, Vec<u8>>>>) {
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
