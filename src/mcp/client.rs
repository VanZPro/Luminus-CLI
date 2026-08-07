use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("RPC error: {0}")]
    Rpc(String),
    #[error("Process exited")]
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

pub struct McpClient {
    _child: Child,
    request_tx: mpsc::Sender<(
        serde_json::Value,
        tokio::sync::oneshot::Sender<serde_json::Value>,
    )>,
}

impl McpClient {
    pub async fn connect(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, McpError> {
        let mut child = Command::new(command)
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = tokio::io::BufReader::new(stdout);

        let (request_tx, mut request_rx) = mpsc::channel::<(
            serde_json::Value,
            tokio::sync::oneshot::Sender<serde_json::Value>,
        )>(32);

        let (response_tx, mut response_rx) = mpsc::channel::<(String, serde_json::Value)>(32);

        // Reader loop
        tokio::spawn(async move {
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = buf.trim_end().to_owned();
                        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line)
                            && let Some(id) = msg.get("id").and_then(|id| id.as_str())
                        {
                            let _ = response_tx.send((id.to_owned(), msg)).await;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Writer + Matcher loop
        tokio::spawn(async move {
            let mut pending = HashMap::new();
            loop {
                tokio::select! {
                    Some((req, reply)) = request_rx.recv() => {
                        if let Some(id) = req.get("id").and_then(|id| id.as_str()) {
                            pending.insert(id.to_owned(), reply);
                        }
                        let line = format!("{}\n", serde_json::to_string(&req).unwrap());
                        if stdin.write_all(line.as_bytes()).await.is_err() {
                            break;
                        }
                        if stdin.flush().await.is_err() {
                            break;
                        }
                    }
                    Some((id, resp)) = response_rx.recv() => {
                        if let Some(reply) = pending.remove(&id) {
                            let _ = reply.send(resp);
                        }
                    }
                    else => break,
                }
            }
        });

        let client = Self {
            _child: child,
            request_tx,
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let id = Uuid::new_v4().to_string();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.request_tx
            .send((req, tx))
            .await
            .map_err(|_| McpError::Exited)?;

        let resp = rx.await.map_err(|_| McpError::Exited)?;

        if let Some(err) = resp.get("error") {
            return Err(McpError::Rpc(err.to_string()));
        }

        Ok(resp.get("result").cloned().unwrap_or(serde_json::json!({})))
    }

    pub async fn initialize(&self) -> Result<(), McpError> {
        let _ = self
            .send_request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "clientInfo": {
                        "name": "Luminus",
                        "version": "0.1.0"
                    },
                    "capabilities": {}
                }),
            )
            .await?;

        // Send initialized notification
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let (tx, _) = tokio::sync::oneshot::channel();
        let _ = self.request_tx.send((req, tx)).await;

        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        let result = self
            .send_request("tools/list", serde_json::json!({}))
            .await?;
        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            let parsed: Result<Vec<McpTool>, _> = tools
                .iter()
                .map(|t| serde_json::from_value(t.clone()))
                .collect();
            return Ok(parsed?);
        }
        Ok(Vec::new())
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        let result = self
            .send_request(
                "tools/call",
                serde_json::json!({
                    "name": name,
                    "arguments": arguments
                }),
            )
            .await?;

        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            let mut texts = Vec::new();
            for item in content {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    texts.push(text.to_owned());
                }
            }
            return Ok(texts.join("\n"));
        }

        Ok(String::new())
    }
}
