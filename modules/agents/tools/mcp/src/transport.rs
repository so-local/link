//! Bounded concurrent requests and cancellation without blocking discovery.
use crate::{Catalog, SETTINGS};
use anyhow::ensure;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
    task::JoinSet,
};

/// Read complete bounded messages independently of in-flight commands.
async fn read(input: impl AsyncBufRead + Unpin, sender: mpsc::Sender<anyhow::Result<String>>) {
    let mut input = input;
    loop {
        let mut line = String::new();
        let result = (&mut input)
            .take(SETTINGS.message_max + 1)
            .read_line(&mut line)
            .await;
        match result {
            Ok(0) => break,
            Ok(bytes) if bytes as u64 <= SETTINGS.message_max => {
                if sender.send(Ok(line)).await.is_err() {
                    break;
                }
            }
            other => {
                let error = match other {
                    Err(err) => anyhow::Error::from(err),
                    _ => anyhow::anyhow!("MCP message exceeds its limit"),
                };
                let _ = sender.send(Err(error)).await;
                break;
            }
        }
    }
}

async fn write(output: &mut (impl AsyncWrite + Unpin), reply: Value) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec(&reply)?;
    bytes.push(b'\n');
    output.write_all(&bytes).await?;
    output.flush().await?;
    Ok(())
}

/// EOF drains accepted calls. Cancellation drops the owned command future.
pub async fn serve(
    catalog: Catalog,
    input: impl AsyncBufRead + Send + Unpin + 'static,
    mut output: impl AsyncWrite + Unpin,
) -> anyhow::Result<()> {
    ensure!(SETTINGS.calls > 0, "MCP concurrency must be positive");
    let (sender, mut messages) = mpsc::channel(SETTINGS.calls);
    let reader = tokio::spawn(read(input, sender));
    let catalog = Arc::new(catalog);
    let mut jobs = JoinSet::new();
    let mut handles = BTreeMap::new();
    let mut ended = false;
    loop {
        tokio::select! {
            message = messages.recv(), if !ended => {
                let Some(line) = message else { ended = true; continue; };
                let line = line?;
                let message: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                if message["method"] == "notifications/cancelled" {
                    let key = message["params"]["requestId"].to_string();
                    if let Some(handle) = handles.remove(&key) {
                        let handle: tokio::task::AbortHandle = handle;
                        handle.abort();
                    }
                    continue;
                }
                if message["method"] != "tools/call" {
                    if let Some(reply) = catalog.answer(&line).await { write(&mut output, reply).await?; }
                    continue;
                }
                let Some(id) = message.get("id") else { continue; };
                let key = id.to_string();
                if jobs.len() >= SETTINGS.calls || handles.contains_key(&key) {
                    write(&mut output, json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"Request capacity exceeded or duplicate ID"}})).await?;
                    continue;
                }
                let catalog = catalog.clone();
                let task_key = key.clone();
                let handle = jobs.spawn(async move { (task_key, catalog.answer(&line).await) });
                handles.insert(key, handle);
            }
            completed = jobs.join_next(), if !jobs.is_empty() => {
                if let Some(Ok((key, reply))) = completed {
                    handles.remove(&key);
                    if let Some(reply) = reply { write(&mut output, reply).await?; }
                }
            }
            else => break,
        }
        if ended && jobs.is_empty() {
            break;
        }
    }
    reader.await?;
    Ok(())
}
