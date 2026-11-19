use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use russh::{Channel, server};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, split};

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
