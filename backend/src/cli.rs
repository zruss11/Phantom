use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};

/// Token usage data from a Claude CLI session
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

/// Token usage info containing both last turn and cumulative totals
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageInfo {
    pub session_id: Option<String>,
    pub last_token_usage: TokenUsage,
    pub total_token_usage: TokenUsage,
    pub model_context_window: Option<i64>,
}

/// Minimal JSON-RPC transport for Codex `app-server`.
///
/// Codex app-server speaks JSON-RPC 2.0 over stdio (one JSON object per line).
struct CodexAppServerClient {
    // Keep the child handle so the process stays owned and can be terminated.
    _child: TokioMutex<tokio::process::Child>,
    stdin: TokioMutex<tokio::process::ChildStdin>,
    notif_rx: TokioMutex<mpsc::UnboundedReceiver<Value>>,
    server_req_rx: TokioMutex<mpsc::UnboundedReceiver<Value>>,
    pending: std::sync::Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Minimal JSON-RPC transport for ACP-based agents (e.g. Claude Code ACP).
///
/// ACP uses JSON-RPC 2.0 over newline-delimited JSON on stdio.
struct AcpClient {
    _child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    pending: std::sync::Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl AcpClient {
    async fn start(
        command: &str,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(cwd)
            .envs(env.to_vec())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("failed to spawn ACP agent")?;
        let stdin = child.stdin.take().context("missing stdin")?;
        let stdout = child.stdout.take().context("missing stdout")?;
        let stderr = child.stderr.take().context("missing stderr")?;

        // Drain stderr so the process can't block.
        tokio::spawn(async move {
            let mut r = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = r.next_line().await {
                eprintln!("[AcpClient][stderr] {}", line);
            }
        });

        let pending: std::sync::Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();

        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
                        let tx_opt = pending_clone
                            .lock()
                            .ok()
                            .and_then(|mut map| map.remove(&id));
                        if let Some(tx) = tx_opt {
                            let _ = tx.send(v);
                        }
                        continue;
                    }
                    // Notifications are ignored for model fetch.
                }
            }
        });

        let next_id = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));

        Ok(Self {
            _child: child,
            stdin,
            pending,
            next_id,
        })
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<Value>();
        {
            let mut map = self
                .pending
                .lock()
                .map_err(|_| anyhow::anyhow!("pending lock"))?;
            map.insert(id, tx);
        }

        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        self.stdin
            .write_all(req.to_string().as_bytes())
            .await
            .context("write ACP request")?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        let resp = rx.await.context("ACP response dropped")?;
        if resp.get("error").is_some() {
            return Err(anyhow::anyhow!("acp rpc error: {}", resp));
        }
        Ok(resp)
    }

    async fn shutdown(&mut self) -> Result<()> {
        let _ = self._child.kill().await;
        let _ = self._child.wait().await;
        Ok(())
    }

    async fn initialize(&mut self) -> Result<()> {
        let _ = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": {"readTextFile": false, "writeTextFile": false},
                        "terminal": false
                    }
                }),
            )
            .await?;
        Ok(())
    }

    async fn session_new(&mut self, cwd: &Path) -> Result<NewSessionResult> {
        let resp = self
            .request(
                "session/new",
                json!({
                    "cwd": cwd.to_string_lossy(),
                    "mcpServers": []
                }),
            )
            .await?;
        let data = resp.get("result").cloned().context("missing ACP result")?;
        let session: NewSessionResult =
            serde_json::from_value(data).context("failed to parse ACP session/new response")?;
        Ok(session)
    }
}

impl CodexAppServerClient {
    async fn start(cwd: &Path, env: &[(String, String)]) -> Result<Self> {
        let mut cmd = Command::new("codex");
        cmd.args(["app-server"])
            .current_dir(cwd)
            .envs(env.to_vec())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("failed to spawn codex app-server")?;
        let stdin = child.stdin.take().context("missing stdin")?;
        let stdout = child.stdout.take().context("missing stdout")?;
        let stderr = child.stderr.take().context("missing stderr")?;

        // drain stderr so the process can't block.
        tokio::spawn(async move {
            let mut r = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = r.next_line().await {
                eprintln!("[CodexAppServer][stderr] {}", line);
            }
        });

        let (notif_tx, notif_rx) = mpsc::unbounded_channel::<Value>();
        let (server_req_tx, server_req_rx) = mpsc::unbounded_channel::<Value>();
        let pending: std::sync::Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();

        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    tracing::info!("[CodexAppServer][stdout] {}", line);
                }
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    // JSON-RPC responses have an id AND a result/error.
                    if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
                        let is_response = v.get("result").is_some() || v.get("error").is_some();
                        let is_server_request = v.get("method").is_some();

                        if is_response {
                            let tx_opt = pending_clone
                                .lock()
                                .ok()
                                .and_then(|mut map| map.remove(&id));
                            if let Some(tx) = tx_opt {
                                let _ = tx.send(v);
                            }
                            continue;
                        }

                        if is_server_request {
                            let _ = server_req_tx.send(v);
                            continue;
                        }
                    }

                    // Notifications (no id)
                    let _ = notif_tx.send(v);
                }
            }
        });

        let next_id = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));

        Ok(Self {
            _child: TokioMutex::new(child),
            stdin: TokioMutex::new(stdin),
            notif_rx: TokioMutex::new(notif_rx),
            server_req_rx: TokioMutex::new(server_req_rx),
            pending,
            next_id,
        })
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<Value>();
        {
            let mut map = self
                .pending
                .lock()
                .map_err(|_| anyhow::anyhow!("pending lock"))?;
            map.insert(id, tx);
        }

        let req = json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(req.to_string().as_bytes())
            .await
            .context("write codex request")?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        let resp = rx.await.context("codex response dropped")?;
        if resp.get("error").is_some() {
            return Err(anyhow::anyhow!("codex rpc error: {}", resp));
        }
        Ok(resp)
    }

    async fn respond_server_request(&self, request_id: &str, result: Value) -> Result<()> {
        let id_value = match request_id.parse::<u64>() {
            Ok(id) => json!(id),
            Err(_) => json!(request_id),
        };
        let resp = json!({
            "id": id_value,
            "result": result,
        });
        tracing::info!("[CodexAppServer][response] {}", resp);
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(resp.to_string().as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn shutdown(&self) -> Result<()> {
        let mut child = self._child.lock().await;
        let _ = child.kill().await;
        let _ = child.wait().await;
        Ok(())
    }

    async fn initialize(&self) -> Result<()> {
        let _ = self
            .request(
                "initialize",
                json!({"clientInfo": {"name": "phantom-harness", "version": "0.1.0"}}),
            )
            .await?;
        // Per protocol, send an `initialized` notification.
        let msg = json!({"jsonrpc": "2.0", "method": "initialized"});
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(msg.to_string().as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn thread_start(
        &self,
        cwd: &Path,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
        _mode: Option<&str>,
    ) -> Result<String> {
        let mut params = json!({
            "cwd": cwd.to_string_lossy(),
            "model": model,
            "approvalPolicy": "never",
            "sandboxPolicy": { "type": "dangerFullAccess" }
        });
        // Add reasoningEffort if specified
        if let Some(effort) = reasoning_effort {
            params["reasoningEffort"] = json!(effort);
        }
        let resp = self.request("thread/start", params).await?;
        let thread_id = resp
            .get("result")
            .and_then(|r| r.get("thread"))
            .and_then(|t| t.get("id"))
            .and_then(|x| x.as_str())
            .context("missing thread id")?;
        Ok(thread_id.to_string())
    }

    async fn thread_resume(
        &self,
        thread_id: &str,
        cwd: &Path,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
        _mode: Option<&str>,
    ) -> Result<String> {
        let mut params = json!({
            "threadId": thread_id,
            "cwd": cwd.to_string_lossy(),
            "model": model,
            "approvalPolicy": "never",
            "sandboxPolicy": { "type": "dangerFullAccess" }
        });
        // Add reasoningEffort if specified
        if let Some(effort) = reasoning_effort {
            params["reasoningEffort"] = json!(effort);
        }
        let resp = self.request("thread/resume", params).await?;
        let thread_id = resp
            .get("result")
            .and_then(|r| r.get("thread"))
            .and_then(|t| t.get("id"))
            .and_then(|x| x.as_str())
            .context("missing thread id")?;
        Ok(thread_id.to_string())
    }

    async fn turn_start_streaming<F>(
        &self,
        thread_id: &str,
        prompt: &str,
        model: Option<&str>,
        cwd: &Path,
        effort: Option<&str>,
        mode: Option<&str>,
        on_update: &mut F,
    ) -> Result<(String, Option<TokenUsageInfo>)>
    where
        F: FnMut(StreamingUpdate),
    {
        // Build params with all turn configuration - Codex requires these per-turn
        let mut params = json!({
            "threadId": thread_id,
            "input": [{"type": "text", "text": prompt}],
            "cwd": cwd.to_string_lossy(),
            "approvalPolicy": "never",
            "sandboxPolicy": { "type": "dangerFullAccess" }
        });
        if let Some(m) = model {
            params["model"] = json!(m);
        }
        // Build collaborationMode with nested settings object
        // The Codex CollaborationMode format: { "mode": "plan", "settings": { "model": "...", "reasoning_effort": "..." } }
        if let Some(mode_val) = mode {
            if mode_val != "default" && !mode_val.is_empty() {
                let mut settings = json!({});
                // model is REQUIRED in the Settings struct
                if let Some(m) = model {
                    settings["model"] = json!(m);
                } else {
                    // Fallback to a default model if none specified
                    settings["model"] = json!("gpt-4.1");
                }
                // reasoning_effort is optional but include it if provided
                if let Some(e) = effort {
                    settings["reasoning_effort"] = json!(e);
                }
                // Include developer_instructions if available for this mode
                if let Some(instr) = self.mode_developer_instructions(mode_val).await {
                    settings["developer_instructions"] = json!(instr);
                }
                params["collaborationMode"] = json!({
                    "mode": mode_val,
                    "settings": settings
                });
            } else if let Some(e) = effort {
                // Only add top-level reasoningEffort if NOT using collaborationMode
                params["reasoningEffort"] = json!(e);
            }
        } else if let Some(e) = effort {
            // No mode specified, add reasoningEffort at top level
            params["reasoningEffort"] = json!(e);
        }
        eprintln!(
            "[Codex] turn/start params: {}",
            serde_json::to_string_pretty(&params).unwrap_or_default()
        );
        let _ = self.request("turn/start", params).await?;

        let mut full = String::new();
        let mut captured_usage: Option<TokenUsageInfo> = None;
        let mut last_turn_diff: Option<String> = None;
        let mut context_compacted_emitted = false;

        let mut server_req_rx = self.server_req_rx.lock().await;
        let mut notif_rx = self.notif_rx.lock().await;

        loop {
            // Prefer handling server requests (which can block turns) promptly.
            let next = tokio::select! {
                req = server_req_rx.recv() => {
                    req.context("codex app-server closed")?
                }
                notif = notif_rx.recv() => {
                    notif.context("codex app-server closed")?
                }
            };

            // Server requests include an id.
            if let Some(req_id) = next.get("id").and_then(|x| x.as_u64()) {
                let method = next.get("method").and_then(|m| m.as_str()).unwrap_or("");
                if method == "item/tool/requestUserInput" || method == "tool/requestUserInput" {
                    // Surface questions to the UI; Codex will pause until we respond.
                    let questions_val = next
                        .get("params")
                        .and_then(|p| p.get("questions"))
                        .or_else(|| {
                            next.get("params")
                                .and_then(|p| p.get("msg"))
                                .and_then(|m| m.get("questions"))
                        })
                        .cloned()
                        .unwrap_or(Value::Array(vec![]));
                    let questions: Vec<UserInputQuestion> =
                        serde_json::from_value(questions_val).unwrap_or_default();

                    on_update(StreamingUpdate::UserInputRequest {
                        request_id: req_id.to_string(),
                        questions,
                    });
                }
                // Ignore other server requests for now (we run in approvalPolicy=never).
                continue;
            }

            let method = next.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = next.get("params").cloned().unwrap_or(Value::Null);
            match method {
                "item/started" => {
                    // Handle item/started to get proper tool names for status display
                    if let Some(item) = next.get("params").and_then(|p| p.get("item")) {
                        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        match item_type {
                            "commandExecution" => {
                                // Extract command array and format it
                                if let Some(cmd) = item.get("command").and_then(|c| c.as_array()) {
                                    let cmd_str = cmd
                                        .iter()
                                        .filter_map(|v| v.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    on_update(StreamingUpdate::ToolCall {
                                        name: "shell".to_string(),
                                        arguments: json!({"command": cmd_str}).to_string(),
                                    });
                                }
                            }
                            "fileChange" => {
                                // Extract file paths being changed
                                if let Some(changes) =
                                    item.get("changes").and_then(|c| c.as_array())
                                {
                                    for change in changes {
                                        if let Some(path) =
                                            change.get("path").and_then(|p| p.as_str())
                                        {
                                            on_update(StreamingUpdate::ToolCall {
                                                name: "edit_file".to_string(),
                                                arguments: json!({"path": path}).to_string(),
                                            });
                                        }
                                    }
                                }
                            }
                            "mcpToolCall" => {
                                // MCP tool calls have tool name directly
                                let tool =
                                    item.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
                                let args = item
                                    .get("arguments")
                                    .map(|a| a.to_string())
                                    .unwrap_or_default();
                                on_update(StreamingUpdate::ToolCall {
                                    name: tool.to_string(),
                                    arguments: args,
                                });
                            }
                            _ => {}
                        }
                    }
                }
                "item/agentMessage/delta" => {
                    let params = next.get("params");
                    let item_id = params
                        .and_then(|p| p.get("itemId"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    if let Some(delta) =
                        params.and_then(|p| p.get("delta")).and_then(|d| d.as_str())
                    {
                        full.push_str(delta);
                        on_update(StreamingUpdate::TextChunk {
                            text: delta.to_string(),
                            item_id,
                        });
                    }
                }
                "item/reasoning/summaryTextDelta"
                | "codex/event/reasoning_content_delta"
                | "codex/event/agent_reasoning_delta" => {
                    let delta = next
                        .get("params")
                        .and_then(|p| {
                            p.get("delta")
                                .or_else(|| p.get("msg").and_then(|m| m.get("delta")))
                        })
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string());
                    if let Some(text) = delta {
                        on_update(StreamingUpdate::ReasoningChunk { text });
                    }
                }
                "codex/event/agent_reasoning" | "item/completed" | "codex/event/item_completed" => {
                    let text = params
                        .get("msg")
                        .and_then(|m| m.get("text"))
                        .or_else(|| {
                            let item = params
                                .get("item")
                                .or_else(|| params.get("msg").and_then(|m| m.get("item")))?;
                            item.get("summary")
                                .or_else(|| item.get("summary_text"))
                                .and_then(|v| {
                                    v.as_array().and_then(|a| a.get(0)).or_else(|| v.get(0))
                                })
                        })
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    if let Some(text) = text {
                        on_update(StreamingUpdate::ReasoningChunk { text });
                    }
                }
                "codex/event/exec_command_begin" => {
                    let cmd = next
                        .get("params")
                        .and_then(|p| p.get("msg"))
                        .and_then(|m| m.get("command"))
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    if !cmd.is_empty() {
                        on_update(StreamingUpdate::ToolCall {
                            name: "shell".to_string(),
                            arguments: json!({"command": cmd}).to_string(),
                        });
                    }
                }
                "codex/event/exec_command_end" => {
                    let msg = next.get("params").and_then(|p| p.get("msg"));
                    let output = msg
                        .and_then(|m| {
                            m.get("formatted_output")
                                .or_else(|| m.get("aggregated_output"))
                                .or_else(|| m.get("stdout"))
                                .or_else(|| m.get("stderr"))
                        })
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    on_update(StreamingUpdate::ToolReturn { output });
                }
                "item/commandExecution/outputDelta" => {
                    // Raw command output - skip as we now show the command name from item/started
                }
                "codex/event/item_started"
                | "codex/event/agent_reasoning_section_break"
                | "item/reasoning/summaryPartAdded"
                | "codex/event/task_complete"
                | "codex/event/agent_message"
                | "codex/event/agent_message_content_delta"
                | "codex/event/agent_message_delta"
                | "turn/started"
                | "codex/event/task_started"
                | "codex/event/user_message"
                | "account/rateLimits/updated" => {
                    // No UI action needed for these notifications
                }
                "codex/event/context_compacted" | "thread/compacted" => {
                    if !context_compacted_emitted {
                        context_compacted_emitted = true;
                        on_update(StreamingUpdate::ToolCall {
                            name: "context_compaction".to_string(),
                            arguments: "{}".to_string(),
                        });
                        on_update(StreamingUpdate::ToolReturn {
                            output: "Context compacted".to_string(),
                        });
                    }
                }
                "codex/event/turn_diff" | "turn/diff/updated" => {
                    let diff = next
                        .get("params")
                        .and_then(|p| {
                            p.get("msg")
                                .and_then(|m| m.get("unified_diff"))
                                .or_else(|| p.get("diff"))
                                .or_else(|| p.get("unified_diff"))
                        })
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    if !diff.is_empty() && last_turn_diff.as_deref() != Some(&diff) {
                        last_turn_diff = Some(diff.clone());
                        on_update(StreamingUpdate::ToolReturn { output: diff });
                    }
                }
                "codex/event/token_count" => {
                    if let Some(token_usage) = next
                        .get("params")
                        .and_then(|p| p.get("msg"))
                        .and_then(|m| m.get("info"))
                        .and_then(|i| {
                            i.get("total_token_usage")
                                .or_else(|| i.get("last_token_usage"))
                        })
                    {
                        captured_usage = parse_codex_token_usage(token_usage);
                    }
                }
                "codex/event/mcp_startup_update" | "codex/event/mcp_startup_complete" => {
                    // MCP lifecycle events are noisy; we don't surface them in chat.
                }
                "codex/event/web_search_end" => {
                    let msg = next.get("params").and_then(|p| p.get("msg"));
                    let query = msg
                        .and_then(|m| m.get("query"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("web_search");
                    on_update(StreamingUpdate::ToolCall {
                        name: "web_search".to_string(),
                        arguments: json!({"query": query}).to_string(),
                    });
                    on_update(StreamingUpdate::ToolReturn {
                        output: "web search completed".to_string(),
                    });
                }
                "turn/plan/updated" => {
                    let payload = params.get("msg").unwrap_or(&params);
                    let explanation = payload
                        .get("explanation")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let turn_id = payload
                        .get("turnId")
                        .or_else(|| payload.get("turn_id"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let plan_val = payload
                        .get("plan")
                        .or_else(|| payload.get("steps"))
                        .cloned()
                        .unwrap_or(Value::Array(vec![]));
                    let steps: Vec<PlanStep> = serde_json::from_value(plan_val).unwrap_or_default();
                    on_update(StreamingUpdate::PlanUpdate {
                        turn_id,
                        explanation,
                        steps,
                    });
                }
                "thread/tokenUsage/updated" => {
                    // Capture token usage from Codex notification
                    if let Some(params) = next.get("params") {
                        if let Some(token_usage) = params
                            .get("token_usage")
                            .or_else(|| params.get("tokenUsage"))
                        {
                            captured_usage = parse_codex_token_usage(token_usage);
                        }
                    }
                }
                "turn/completed" => {
                    break;
                }
                _ => {
                    if !method.is_empty() {
                        eprintln!(
                            "[CodexAppServer] Unhandled notification: {} params={}",
                            method, params
                        );
                    }
                }
            }
        }
        Ok((full, captured_usage))
    }

    /// Fetch available models from the Codex app-server via model/list
    async fn model_list(&self) -> Result<Vec<CodexModelInfo>> {
        let resp = self.request("model/list", json!({})).await?;
        let data = resp
            .get("result")
            .and_then(|r| r.get("data"))
            .or_else(|| resp.get("data"))
            .cloned()
            .unwrap_or(Value::Array(vec![]));

        let models: Vec<CodexModelInfo> = serde_json::from_value(data).unwrap_or_default();
        Ok(models)
    }

    /// Fetch available modes from the Codex app-server via collaborationMode/list (new) or mode/list (legacy)
    /// Falls back to hardcoded modes if endpoints are not supported
    async fn mode_list(&self) -> Result<Vec<CodexModeInfo>> {
        async fn fetch_modes(
            client: &CodexAppServerClient,
            method: &str,
        ) -> Result<Option<Vec<CodexModeInfo>>> {
            match client.request(method, json!({})).await {
                Ok(resp) => {
                    let result = resp.get("result");
                    let result_data = result.and_then(|r| r.get("data"));
                    let data = result_data
                        .and_then(|v| {
                            if v.is_array() {
                                Some(v)
                            } else if v.is_object() {
                                v.get("modes")
                                    .or_else(|| v.get("items"))
                                    .or_else(|| v.get("collaborationModes"))
                            } else {
                                None
                            }
                        })
                        .or_else(|| result.and_then(|r| r.get("modes")))
                        .or_else(|| result.and_then(|r| r.get("items")))
                        .or_else(|| result.and_then(|r| r.get("collaborationModes")))
                        .or_else(|| resp.get("data"))
                        .or_else(|| resp.get("modes"))
                        .or_else(|| resp.get("items"))
                        .or_else(|| resp.get("collaborationModes"))
                        .or_else(|| {
                            if result.map(|r| r.is_array()).unwrap_or(false) {
                                result
                            } else {
                                None
                            }
                        })
                        .cloned()
                        .unwrap_or(Value::Array(vec![]));

                    let modes: Vec<CodexModeInfo> =
                        serde_json::from_value(data.clone()).unwrap_or_default();

                    // Backfill `id` from `mode` when needed (Codex uses `mode` field).
                    let modes: Vec<CodexModeInfo> = modes
                        .into_iter()
                        .map(|mut mode| {
                            if mode.id.is_none() {
                                mode.id = mode.mode.clone();
                            }
                            mode
                        })
                        .collect();

                    if !modes.is_empty() {
                        return Ok(Some(modes));
                    }
                    let keys = result
                        .and_then(|r| r.as_object())
                        .map(|obj| obj.keys().cloned().collect::<Vec<_>>().join(","))
                        .unwrap_or_else(|| "n/a".to_string());
                    let data_kind = result_data
                        .map(|v| {
                            if v.is_array() {
                                "array"
                            } else if v.is_object() {
                                "object"
                            } else {
                                "other"
                            }
                        })
                        .unwrap_or("missing");
                    eprintln!(
                        "[CodexAppServer] {} returned no modes (result keys: {}, result.data: {})",
                        method, keys, data_kind
                    );
                    Ok(None)
                }
                Err(e) => {
                    eprintln!("[CodexAppServer] {} not supported: {}", method, e);
                    Ok(None)
                }
            }
        }

        if let Some(modes) = fetch_modes(self, "collaborationMode/list").await? {
            return Ok(modes);
        }

        if let Some(modes) = fetch_modes(self, "mode/list").await? {
            return Ok(modes);
        }

        // Fallback to hardcoded modes
        Ok(vec![
            CodexModeInfo {
                id: Some("default".to_string()),
                mode: None,
                name: Some("Default".to_string()),
                description: Some("Standard coding mode".to_string()),
                developer_instructions: None,
                reasoning_effort: None,
                model: None,
            },
            CodexModeInfo {
                id: Some("plan".to_string()),
                mode: None,
                name: Some("Plan".to_string()),
                description: Some("Creates implementation plans before coding".to_string()),
                developer_instructions: None,
                reasoning_effort: None,
                model: None,
            },
        ])
    }

    async fn mode_developer_instructions(&self, mode: &str) -> Option<String> {
        let modes = self.mode_list().await.ok()?;
        modes
            .into_iter()
            .find(|m| m.matches_mode(mode))
            .and_then(|m| m.developer_instructions)
    }
}

/// Model info returned by Codex app-server model/list
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelInfo {
    pub id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub supported_reasoning_efforts: Vec<ReasoningEffortOption>,
    #[serde(default)]
    pub default_reasoning_effort: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

/// Mode info returned by Codex app-server mode/list
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodexModeInfo {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(
        default,
        alias = "developer_instructions",
        alias = "developerInstructions"
    )]
    pub developer_instructions: Option<String>,
    #[serde(default, alias = "reasoning_effort", alias = "reasoningEffort")]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

impl CodexModeInfo {
    fn matches_mode(&self, mode: &str) -> bool {
        self.id.as_deref() == Some(mode) || self.mode.as_deref() == Some(mode)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEffortOption {
    pub reasoning_effort: String,
    #[serde(default)]
    pub description: Option<String>,
}

pub struct AgentProcessClient {
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: PathBuf,
    model: std::sync::Mutex<Option<String>>,
    reasoning_effort: std::sync::Mutex<Option<String>>,
    permission_mode: std::sync::Mutex<Option<String>>,
    agent_mode: std::sync::Mutex<Option<String>>,
    codex_mode: std::sync::Mutex<Option<String>>,
    codex_app_server: Option<std::sync::Arc<CodexAppServerClient>>,
    acp_client: Option<TokioMutex<AcpClient>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigOptionValue {
    pub value: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigOption {
    pub id: String,
    pub name: Option<String>,
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub option_type: Option<String>,
    #[serde(default)]
    pub options: Vec<ConfigOptionValue>,
    #[serde(rename = "currentValue")]
    pub current_value: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModelInfo {
    #[serde(alias = "modelId")]
    pub id: String,
    #[serde(alias = "name", alias = "displayName")]
    pub label: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SessionModelState {
    #[serde(rename = "currentModelId")]
    pub current_model_id: Option<String>,
    #[serde(rename = "availableModels", default)]
    pub available_models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableMode {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionModeState {
    #[serde(rename = "currentModeId")]
    pub current_mode_id: Option<String>,
    #[serde(rename = "availableModes", default)]
    pub available_modes: Vec<AvailableMode>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NewSessionResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "configOptions", default)]
    pub config_options: Vec<ConfigOption>,
    #[serde(default)]
    pub models: Option<SessionModelState>,
    #[serde(default)]
    pub modes: Option<SessionModeState>,
}

/// Image content for Claude prompts (base64 encoded)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    /// MIME type: image/jpeg, image/png, image/gif, image/webp
    pub media_type: String,
    /// Base64-encoded image data
    pub data: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PromptMessage {
    #[serde(rename = "message_type")]
    pub message_type: String,
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
    pub tool_return: Option<String>,
}

/// Permission option for user to choose from
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableCommand {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub step: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub enum StreamingUpdate {
    ToolCall {
        name: String,
        arguments: String,
    },
    ToolReturn {
        output: String,
    },
    ReasoningChunk {
        text: String,
    },
    TextChunk {
        text: String,
        item_id: Option<String>,
    },
    Status {
        message: String,
    },
    PermissionRequest {
        request_id: String,
        tool_name: String,
        description: Option<String>,
        raw_input: Option<String>,
        options: Vec<PermissionOption>,
    },
    /// Codex `request_user_input` tool call surfaced over app-server.
    UserInputRequest {
        request_id: String,
        questions: Vec<UserInputQuestion>,
    },
    /// Codex turn/plan/updated notification.
    PlanUpdate {
        turn_id: Option<String>,
        explanation: Option<String>,
        steps: Vec<PlanStep>,
    },
    AvailableCommands {
        commands: Vec<AvailableCommand>,
    },
    /// Plan content from Write/Create tool creating a plan.md file
    PlanContent {
        file_path: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub options: Option<Vec<UserInputOption>>,
}

#[derive(Debug, Clone)]
pub struct SessionPromptResult {
    pub messages: Vec<PromptMessage>,
    pub stop_reason: Option<String>,
    pub token_usage: Option<TokenUsageInfo>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionResult {
    pub session_id: String,
    #[serde(default)]
    pub restored: bool,
}

impl AgentProcessClient {
    pub async fn spawn(
        command: &str,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<Self> {
        // Codex uses the JSON-RPC app-server protocol (not stream-json prompts).
        let codex_app_server = if std::path::Path::new(command)
            .file_name()
            .and_then(|s| s.to_str())
            == Some("codex")
        {
            let client = CodexAppServerClient::start(cwd, env).await?;
            client.initialize().await?;
            Some(std::sync::Arc::new(client))
        } else {
            None
        };

        // ACP-based agents (e.g. Claude Code ACP) use JSON-RPC over stdio.
        let acp_client = {
            let filename = std::path::Path::new(command)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let is_acp = filename.contains("claude-code-acp");
            if is_acp {
                let client = AcpClient::start(command, args, cwd, env).await?;
                Some(TokioMutex::new(client))
            } else {
                None
            }
        };

        Ok(Self {
            command: command.to_string(),
            args: args.to_vec(),
            env: env.to_vec(),
            cwd: cwd.to_path_buf(),
            model: std::sync::Mutex::new(None),
            reasoning_effort: std::sync::Mutex::new(None),
            permission_mode: std::sync::Mutex::new(None),
            agent_mode: std::sync::Mutex::new(None),
            codex_mode: std::sync::Mutex::new(None),
            codex_app_server,
            acp_client,
        })
    }

    pub async fn initialize(&self, _name: &str, _version: &str) -> Result<()> {
        if let Some(acp) = &self.acp_client {
            let mut acp = acp.lock().await;
            acp.initialize().await?;
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        if let Some(codex) = &self.codex_app_server {
            let _ = codex.shutdown().await;
        }
        if let Some(acp) = &self.acp_client {
            let mut acp = acp.lock().await;
            let _ = acp.shutdown().await;
        }
        Ok(())
    }

    /// Check if this client is connected to a Codex app-server
    pub fn is_codex(&self) -> bool {
        self.codex_app_server.is_some()
    }

    /// Fetch available models from Codex app-server (only works for Codex agents)
    pub async fn fetch_codex_models(&self) -> Result<Vec<CodexModelInfo>> {
        if let Some(codex) = &self.codex_app_server {
            return codex.model_list().await;
        }
        Err(anyhow::anyhow!("Not a Codex agent"))
    }

    /// Fetch available modes from Codex app-server (only works for Codex agents)
    pub async fn fetch_codex_modes(&self) -> Result<Vec<CodexModeInfo>> {
        if let Some(codex) = &self.codex_app_server {
            return codex.mode_list().await;
        }
        Err(anyhow::anyhow!("Not a Codex agent"))
    }

    /// Set the Codex mode (e.g., "default", "plan", "pair-programming", "execute")
    pub fn set_codex_mode(&self, mode: Option<&str>) {
        let mut guard = self.codex_mode.lock().unwrap_or_else(|e| e.into_inner());
        *guard = mode.map(|s| s.to_string());
    }

    pub async fn session_new(&self, _cwd: &str) -> Result<NewSessionResult> {
        if let Some(acp) = &self.acp_client {
            let mut acp = acp.lock().await;
            return acp.session_new(&self.cwd).await;
        }
        if let Some(codex) = &self.codex_app_server {
            let model = self.model.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let reasoning_effort = self
                .reasoning_effort
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let codex_mode = self
                .codex_mode
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let thread_id = codex
                .thread_start(
                    &self.cwd,
                    model.as_deref(),
                    reasoning_effort.as_deref(),
                    codex_mode.as_deref(),
                )
                .await?;
            return Ok(NewSessionResult {
                session_id: thread_id,
                config_options: Vec::new(),
                models: None,
                modes: None,
            });
        }

        let session_id = format!("local-{}", uuid::Uuid::new_v4());
        Ok(NewSessionResult {
            session_id,
            config_options: Vec::new(),
            models: None,
            modes: None,
        })
    }

    pub async fn session_load(
        &self,
        session_id: &str,
        _cwd: &str,
        _servers: Vec<String>,
    ) -> Result<LoadSessionResult> {
        if let Some(codex) = &self.codex_app_server {
            let model = self.model.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let reasoning_effort = self
                .reasoning_effort
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let codex_mode = self
                .codex_mode
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let thread_id = codex
                .thread_resume(
                    session_id,
                    &self.cwd,
                    model.as_deref(),
                    reasoning_effort.as_deref(),
                    codex_mode.as_deref(),
                )
                .await?;
            return Ok(LoadSessionResult {
                session_id: thread_id,
                restored: true,
            });
        }

        Ok(LoadSessionResult {
            session_id: session_id.to_string(),
            restored: true,
        })
    }

    pub fn supports_load_session(&self) -> bool {
        true
    }

    pub async fn session_set_mode(&self, _session_id: &str, mode: &str) -> Result<()> {
        let agent_modes = ["coder", "summarizer", "task", "title"];
        if agent_modes.contains(&mode) {
            let mut guard = self.agent_mode.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(mode.to_string());
        } else {
            let mut guard = self
                .permission_mode
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *guard = Some(mode.to_string());
        }
        Ok(())
    }

    pub async fn set_session_model(&self, _session_id: &str, model_value: &str) -> Result<()> {
        let mut guard = self.model.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(model_value.to_string());
        Ok(())
    }

    pub fn set_reasoning_effort(&self, effort: Option<&str>) {
        let mut guard = self
            .reasoning_effort
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = effort.map(|s| s.to_string());
    }

    pub async fn set_config_option(
        &self,
        _session_id: &str,
        _config_id: &str,
        _value: &str,
    ) -> Result<Vec<ConfigOption>> {
        Ok(Vec::new())
    }

    pub async fn send_permission_response(
        &self,
        _session_id: &str,
        _request_id: &str,
        _response_id: &str,
    ) -> Result<()> {
        Ok(())
    }

    pub async fn send_user_input_response(&self, request_id: &str, answers: Value) -> Result<()> {
        if let Some(codex) = &self.codex_app_server {
            tracing::info!(
                "[CodexAppServer] sending requestUserInput response id={} payload={}",
                request_id,
                answers
            );
            return codex
                .respond_server_request(request_id, json!({ "answers": answers }))
                .await;
        }
        Err(anyhow::anyhow!(
            "User input response only supported for Codex"
        ))
    }

    pub async fn session_prompt_streaming<F>(
        &self,
        session_id: &str,
        content: &str,
        mut on_update: F,
    ) -> Result<SessionPromptResult>
    where
        F: FnMut(StreamingUpdate),
    {
        self.run_prompt(session_id, content, &[], &mut on_update)
            .await
    }

    pub async fn session_prompt_streaming_with_images<F>(
        &self,
        session_id: &str,
        content: &str,
        images: &[ImageContent],
        mut on_update: F,
    ) -> Result<SessionPromptResult>
    where
        F: FnMut(StreamingUpdate),
    {
        self.run_prompt(session_id, content, images, &mut on_update)
            .await
    }

    async fn run_prompt<F>(
        &self,
        session_id: &str,
        content: &str,
        images: &[ImageContent],
        on_update: &mut F,
    ) -> Result<SessionPromptResult>
    where
        F: FnMut(StreamingUpdate),
    {
        // Codex uses the app-server JSON-RPC protocol and keeps a long-lived process.
        if let Some(codex) = &self.codex_app_server {
            let model = self.model.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let reasoning_effort = self
                .reasoning_effort
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let codex_mode = self
                .codex_mode
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let (text, usage) = codex
                .turn_start_streaming(
                    session_id,
                    content,
                    model.as_deref(),
                    &self.cwd,
                    reasoning_effort.as_deref(),
                    codex_mode.as_deref(),
                    on_update,
                )
                .await?;

            let messages = if text.is_empty() {
                Vec::new()
            } else {
                vec![PromptMessage {
                    message_type: "assistant_message".to_string(),
                    content: Some(text),
                    reasoning: None,
                    name: None,
                    arguments: None,
                    tool_return: None,
                }]
            };

            return Ok(SessionPromptResult {
                messages,
                stop_reason: None,
                token_usage: usage,
                session_id: Some(session_id.to_string()),
            });
        }

        let mut cmd = Command::new(&self.command);
        let mut args = self.args.clone();

        // Not every CLI supports Phantom's stream-json parsing.
        // For terminal-first CLIs (e.g. opencode), we stream raw stdout instead.
        // Note: Amp uses --stream-json (in config args), not --output-format stream-json.
        let cmd_name = std::path::Path::new(&self.command)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&self.command);
        if cmd_name != "opencode" && cmd_name != "amp" {
            ensure_output_format(&mut args);
        }

        let model = self.model.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let permission_mode = self
            .permission_mode
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let agent_mode = self
            .agent_mode
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        if let Some(model) = model.as_ref() {
            // Only pass model flags when we know the CLI supports them.
            if cmd_name == "opencode" {
                // OpenCode expects provider/model (e.g. anthropic/claude-3-5-sonnet).
                // Skip if phantom is using a placeholder/default.
                if model != "default" {
                    args.push("--model".to_string());
                    args.push(model.to_string());
                }
            } else if cmd_name != "amp" {
                args.push("--model".to_string());
                args.push(model.to_string());
            }
        }

        if !session_id.is_empty() && !session_id.starts_with("local-") {
            // Note: OpenCode's --session flag must come AFTER the `run` subcommand,
            // so we handle it in the opencode-specific block below.
            if cmd_name == "amp" {
                // Amp uses `amp threads continue` for resuming a thread.
                // The session_id IS the thread reference (e.g., "T-7f395a45...")
                // We insert "threads continue" subcommands, and the prompt will be added via --execute later
                args.insert(0, "threads".to_string());
                args.insert(1, "continue".to_string());
                eprintln!("[Harness][Amp] resuming thread: {}", session_id);
            } else if cmd_name == "droid" {
                // Factory Droid uses -s/--session-id for session continuation
                args.push("-s".to_string());
                args.push(session_id.to_string());
            } else if cmd_name != "opencode" {
                // Most other CLIs we wrap use `--resume`.
                args.push("--resume".to_string());
                args.push(session_id.to_string());
            }
        }

        if let Some(mode) = permission_mode.as_ref() {
            // Claude Code CLI can block waiting for interactive permission prompts.
            // Until we fully support permission request/response in the UI for the native CLI path,
            // default to a non-interactive permission mode.
            if cmd_name == "amp" {
                // Amp uses --dangerously-allow-all for bypassing all permission checks
                if mode == "bypassPermissions" {
                    args.push("--dangerously-allow-all".to_string());
                }
                // Note: Amp doesn't have granular permission modes like Claude
            } else if cmd_name == "droid" {
                // Factory Droid permission modes:
                // - --skip-permissions-unsafe: Full bypass (for "bypassPermissions")
                // - --auto <level>: Partial autonomy (low|medium|high)
                // Since Phantom Harness is non-interactive, we must always set a mode
                if mode == "bypassPermissions" {
                    args.push("--skip-permissions-unsafe".to_string());
                } else {
                    // Default to --auto high for non-bypass modes
                    // This allows most operations without prompts while being safer than full bypass
                    args.push("--auto".to_string());
                    args.push("high".to_string());
                }
            } else if cmd_name != "opencode" {
                // YOLO mode: always bypass permission prompts.
                let effective_mode = if cmd_name == "claude" {
                    "bypassPermissions".to_string()
                } else {
                    mode.to_string()
                };
                args.push("--permission-mode".to_string());
                args.push(effective_mode);
            }
        } else if cmd_name == "droid" {
            // No permission mode set - default to --auto high for non-interactive operation
            args.push("--auto".to_string());
            args.push("high".to_string());
        }

        if let Some(agent_mode) = agent_mode.as_ref() {
            if cmd_name == "opencode" {
                // OpenCode uses --agent <agent-name> for agent selection
                // Agents: build, plan, general, explore
                args.push("--agent".to_string());
                args.push(agent_mode.to_string());
            } else if cmd_name != "amp" {
                // Other CLIs (Claude) use --agent-mode
                args.push("--agent-mode".to_string());
                args.push(agent_mode.to_string());
            }
        }

        // Claude Code CLI expects the prompt as a positional argument with --print for non-interactive output.
        // It does NOT support a `--prompt` flag.
        let cmd_name = std::path::Path::new(&self.command)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&self.command);

        // Keep temp image files alive until the subprocess completes.
        let mut temp_images: Vec<tempfile::TempPath> = Vec::new();
        let mut attachment_markdown = String::new();
        if cmd_name == "claude" {
            let mut index = 1;
            for image in images {
                if let Some(path) = write_image_temp(image)? {
                    if attachment_markdown.is_empty() {
                        attachment_markdown.push_str("\n\nAttached images:\n");
                    }
                    let file_url = file_url_for_path(&path);
                    attachment_markdown.push_str(&format!("\n![Image {}]({})", index, file_url));
                    index += 1;
                    temp_images.push(path);
                }
            }
        } else {
            for image in images {
                if let Some(path) = write_image_temp(image)? {
                    args.push("--image".to_string());
                    args.push(path.to_string_lossy().to_string());
                    temp_images.push(path);
                }
            }
        }
        if cmd_name == "claude" {
            args.push("--print".to_string());
            // Do not force --max-turns. Let Claude decide when it's done; we'll treat
            // a `type: result` event as terminal on our side.
            // Claude requires --verbose when using --print + --output-format=stream-json
            if args.iter().any(|a| a == "--output-format") {
                args.push("--verbose".to_string());
            }
            let mut prompt = content.to_string();
            if !attachment_markdown.is_empty() {
                prompt.push_str(&attachment_markdown);
            }
            args.push(prompt);
        } else if cmd_name == "opencode" {
            // Prefer the programmatic CLI surface (no TUI):
            // `opencode run --format json [--model X] [--agent Y] [--session <id>] <message...>`
            // Insert `run` at position 0 so it comes before any flags (--model, --agent)
            // that were added earlier in the argument building process.
            args.insert(0, "run".to_string());
            args.push("--format".to_string());
            args.push("json".to_string());

            // --session for resuming sessions
            if !session_id.is_empty() && !session_id.starts_with("local-") {
                args.push("--session".to_string());
                args.push(session_id.to_string());
            }

            // prompt goes as positional "message.."
            args.push(content.to_string());
        } else if cmd_name == "amp" {
            // Amp's programmatic mode: --execute "prompt" runs non-interactively
            // (--stream-json and --stream-json-thinking are already in config args)
            args.push("--execute".to_string());
            args.push(content.to_string());
        } else if cmd_name == "droid" {
            // Factory Droid needs `exec` subcommand for non-interactive mode
            // Usage: droid exec [OPTIONS] [PROMPT]
            // Insert `exec` at the beginning (before other flags like --output-format)
            args.insert(0, "exec".to_string());
            // Prompt goes as positional argument at the end
            args.push(content.to_string());
        } else {
            args.push("--prompt".to_string());
            args.push(content.to_string());
        }

        // Debug: print the exact spawned command (helps diagnose hanging runs)
        eprintln!(
            "[Harness] spawn agent: cmd={} args={}",
            self.command,
            args.iter()
                .map(|a| format!("{:?}", a))
                .collect::<Vec<_>>()
                .join(" ")
        );

        cmd.args(&args)
            .current_dir(&self.cwd)
            .envs(self.env.clone())
            // Prevent the CLI from waiting on any stdin interaction.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("failed to spawn agent CLI")?;
        // (tokio::process::Child does not support kill_on_drop on this version)

        let stdout = child
            .stdout
            .take()
            .context("missing stdout for agent CLI")?;
        let stderr = child
            .stderr
            .take()
            .context("missing stderr for agent CLI")?;

        let mut stdout_reader = BufReader::new(stdout).lines();

        // Important: drain stderr concurrently so the child process can't block if it logs a lot.
        // Claude Code (and plugins) may emit substantial output on stderr.
        // We also keep a bounded buffer for error reporting.
        let stderr_buf: std::sync::Arc<std::sync::Mutex<String>> =
            std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let stderr_buf_clone = stderr_buf.clone();
        tokio::spawn(async move {
            let mut stderr_reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = stderr_reader.next_line().await {
                if let Ok(mut buf) = stderr_buf_clone.lock() {
                    // keep last ~50KB
                    if buf.len() > 50_000 {
                        let drain = buf.len() - 50_000;
                        buf.drain(..drain);
                    }
                    buf.push_str(&line);
                    buf.push('\n');
                }
            }
        });

        let mut messages: Vec<PromptMessage> = Vec::new();
        let mut current_assistant_text = String::new();
        let mut current_reasoning_text = String::new();
        let mut token_usage: Option<TokenUsageInfo> = None;
        let mut observed_session_id: Option<String> = None;

        // Track Claude Code sub-agent Tasks for progress pill.
        // Maps tool_use_id -> (description, status) where status is
        // "in_progress" or "completed".
        let mut claude_tasks: Vec<(String, String, String)> = Vec::new();

        // Claude's CLI may keep the process alive after emitting a final `type: result`.
        // We treat `result` as terminal for this prompt and then terminate the subprocess.
        let mut saw_result = false;

        use tokio::time::{timeout, Duration};

        loop {
            // Claude can take a while to initialize in a large repo (context/plugins).
            // Don't hang forever, but give it a realistic window.
            let next = timeout(Duration::from_secs(600), stdout_reader.next_line()).await;
            let line = match next {
                Ok(Ok(Some(l))) => l,
                Ok(Ok(None)) => break,
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => {
                    let _ = child.kill().await;
                    return Err(anyhow::anyhow!("timeout waiting for claude output"));
                }
            };

            // Always log Amp output for development/debugging (Amp is new integration)
            if cmd_name == "amp" {
                eprintln!("[Harness][Amp] stdout: {}", line);
            }

            // Always log Droid output for development/debugging (Droid is new integration)
            if cmd_name == "droid" {
                eprintln!("[Harness][Droid] stdout: {}", line);
            }

            // Always log OpenCode output for development/debugging
            if cmd_name == "opencode" {
                eprintln!("[Harness][OpenCode] stdout: {}", line);
            }

            // Debug: log raw NDJSON lines (bounded) so we can see if Claude is emitting
            // but our parsing/persistence is off.
            if std::env::var("PHANTOM_CLAUDE_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("[Harness] agent stdout: {}", line);
            }

            // File logging: write raw NDJSON to a log file for debugging tasks/todos
            // Set PHANTOM_CLAUDE_LOG_FILE=/path/to/claude_raw.log to enable
            if let Ok(log_path) = std::env::var("PHANTOM_CLAUDE_LOG_FILE") {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                {
                    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
                    let _ = writeln!(file, "[{}] {}", timestamp, line);
                }
            }

            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if observed_session_id.is_none() {
                    observed_session_id = find_session_id(&value);
                }

                if cmd_name == "amp" {
                    // Amp emits NDJSON events with --stream-json. Parse them into our streaming updates.
                    for update in parse_amp_event(&value) {
                        // Maintain text buffers for final persistence
                        if let StreamingUpdate::TextChunk { text, .. } = &update {
                            current_assistant_text.push_str(text);
                        }
                        if let StreamingUpdate::ReasoningChunk { text, .. } = &update {
                            current_reasoning_text.push_str(text);
                        }
                        on_update(update);
                    }
                    // Check for terminal result event
                    if value.get("type").and_then(|v| v.as_str()) == Some("result") {
                        saw_result = true;
                        break;
                    }
                } else if cmd_name == "droid" {
                    // Factory Droid emits NDJSON events with --output-format stream-json
                    for update in parse_droid_event(&value) {
                        // Maintain text buffers for final persistence
                        if let StreamingUpdate::TextChunk { text, .. } = &update {
                            current_assistant_text.push_str(text);
                        }
                        on_update(update);
                    }
                    // Check for terminal completion event
                    if value.get("type").and_then(|v| v.as_str()) == Some("completion") {
                        saw_result = true;
                        break;
                    }
                } else if cmd_name == "opencode" {
                    // OpenCode emits one JSON line per event. Convert events into our streaming updates.
                    for update in parse_opencode_event(&value) {
                        // maintain assistant_text buffer for final persistence
                        if let StreamingUpdate::TextChunk { text, .. } = &update {
                            current_assistant_text.push_str(text);
                        }
                        on_update(update);
                    }
                } else {
                    for update in parse_streaming_updates(&value) {
                        on_update(update);
                    }

                    // Track Claude Code sub-agent Task tool_use events for
                    // the progress pill. A "Task" tool_use starts a sub-agent;
                    // its corresponding tool_result (matched by tool_use_id)
                    // means the sub-agent finished.
                    let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if event_type == "assistant" {
                        if let Some(items) = value
                            .get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                        {
                            let mut changed = false;
                            for item in items {
                                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                if item_type == "tool_use" {
                                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                    if name == "Task" || name == "TodoWrite" {
                                        let tool_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let desc = item
                                            .get("input")
                                            .and_then(|i| i.get("description"))
                                            .and_then(|d| d.as_str())
                                            .unwrap_or("Working…")
                                            .to_string();
                                        if !tool_id.is_empty() {
                                            claude_tasks.push((tool_id, desc, "in_progress".to_string()));
                                            changed = true;
                                        }
                                    }
                                }
                            }
                            if changed {
                                let steps: Vec<PlanStep> = claude_tasks.iter().map(|(_, desc, status)| {
                                    PlanStep { step: desc.clone(), status: status.clone() }
                                }).collect();
                                on_update(StreamingUpdate::PlanUpdate {
                                    turn_id: None,
                                    explanation: None,
                                    steps,
                                });
                            }
                        }
                    } else if event_type == "user" && !claude_tasks.is_empty() {
                        if let Some(items) = value
                            .get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                        {
                            let mut changed = false;
                            for item in items {
                                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                if item_type == "tool_result" {
                                    let tool_use_id = item.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                                    for task in claude_tasks.iter_mut() {
                                        if task.0 == tool_use_id && task.2 != "completed" {
                                            task.2 = "completed".to_string();
                                            changed = true;
                                        }
                                    }
                                }
                            }
                            // Also check top-level tool_use_result for result events
                            // that reference a parent_tool_use_id
                            if let Some(parent_id) = value.get("parent_tool_use_id").and_then(|v| v.as_str()) {
                                for task in claude_tasks.iter_mut() {
                                    if task.0 == parent_id && task.2 != "completed" {
                                        task.2 = "completed".to_string();
                                        changed = true;
                                    }
                                }
                            }
                            if changed {
                                let steps: Vec<PlanStep> = claude_tasks.iter().map(|(_, desc, status)| {
                                    PlanStep { step: desc.clone(), status: status.clone() }
                                }).collect();
                                on_update(StreamingUpdate::PlanUpdate {
                                    turn_id: None,
                                    explanation: None,
                                    steps,
                                });
                            }
                        }
                    }

                    for msg in parse_prompt_messages(
                        &value,
                        &mut current_assistant_text,
                        &mut current_reasoning_text,
                    ) {
                        messages.push(msg);
                    }
                    if let Some(usage) = parse_token_usage(&value, observed_session_id.clone()) {
                        token_usage = Some(usage);
                    }

                    if event_type == "result" {
                        saw_result = true;
                        break;
                    }
                }
            } else {
                // Best-effort support for non-stream-json CLIs (e.g. amp, or opencode if user changes format).
                // Treat each line as assistant output.
                let text = format!("{}\n", line);
                current_assistant_text.push_str(&text);
                on_update(StreamingUpdate::TextChunk {
                    text,
                    item_id: None,
                });
            }
        }

        // If we saw a terminal result, do not block waiting for the process to exit.
        // Claude Code can keep the process alive after emitting `type: result`.
        // We kill it in the background and return immediately so the UI can finish the task.
        if saw_result {
            let mut child_for_kill = child;
            tokio::spawn(async move {
                use tokio::time::{timeout, Duration};
                let _ = child_for_kill.kill().await;
                // ensure it doesn't become a zombie
                let _ = timeout(Duration::from_secs(5), child_for_kill.wait()).await;
            });
        } else {
            // No result observed: wait for exit and surface stderr on failure.
            let status = child.wait().await?;
            if !status.success() {
                let stderr_output = stderr_buf
                    .lock()
                    .map(|s| s.clone())
                    .unwrap_or_else(|_| String::new());
                return Err(anyhow::anyhow!(
                    "agent CLI exited with {}: {}",
                    status,
                    stderr_output.trim()
                ));
            }
        }

        let cmd_basename = std::path::Path::new(&self.command)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&self.command);

        if cmd_basename == "claude" {
            if current_assistant_text.contains("invalid_request_error")
                && current_assistant_text.contains("tool_reference.tool_name")
            {
                current_assistant_text = "Claude API rejected a tool result because a tool name exceeded 64 characters. Shorten tool names or disable the tool and try again. (See logs for the raw error.)".to_string();
            }
            eprintln!(
                "[Harness][Claude] response summary: assistant_chars={} reasoning_chars={} tool_messages={}",
                current_assistant_text.len(),
                current_reasoning_text.len(),
                messages.len()
            );
            if std::env::var("PHANTOM_CLAUDE_DEBUG").ok().as_deref() == Some("1") {
                let preview: String = current_assistant_text.chars().take(200).collect();
                eprintln!("[Harness][Claude] assistant preview: {}", preview);
            }
        }

        // Amp summary logging (always enabled for new integration debugging)
        if cmd_basename == "amp" {
            eprintln!(
                "[Harness][Amp] response summary: assistant_chars={} reasoning_chars={} messages={}",
                current_assistant_text.len(),
                current_reasoning_text.len(),
                messages.len()
            );
        }

        if !current_reasoning_text.is_empty() {
            messages.push(PromptMessage {
                message_type: "reasoning_message".to_string(),
                content: None,
                reasoning: Some(current_reasoning_text),
                name: None,
                arguments: None,
                tool_return: None,
            });
        }
        if !current_assistant_text.is_empty() {
            messages.push(PromptMessage {
                message_type: "assistant_message".to_string(),
                content: Some(current_assistant_text),
                reasoning: None,
                name: None,
                arguments: None,
                tool_return: None,
            });
        }

        // Prefer Claude's real session_id when available.
        Ok(SessionPromptResult {
            messages,
            stop_reason: None,
            token_usage,
            session_id: observed_session_id,
        })
    }
}

fn ensure_output_format(args: &mut Vec<String>) {
    if args.iter().any(|arg| arg == "--output-format") {
        return;
    }
    args.push("--output-format".to_string());
    args.push("stream-json".to_string());
}

fn write_image_temp(image: &ImageContent) -> Result<Option<tempfile::TempPath>> {
    use base64::Engine;
    use std::io::Write;

    let extension = match image.media_type.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "img",
    };

    let data = base64::engine::general_purpose::STANDARD
        .decode(&image.data)
        .context("failed to decode image data")?;

    // Use tempfile so the file is automatically removed when dropped.
    let mut f = tempfile::Builder::new()
        .prefix("phantom-image-")
        .suffix(&format!(".{extension}"))
        .tempfile()
        .context("failed to create temp image file")?;

    f.write_all(&data)
        .context("failed to write temp image file")?;

    Ok(Some(f.into_temp_path()))
}

fn file_url_for_path(path: &std::path::Path) -> String {
    let mut path_str = path.to_string_lossy().replace('\\', "/");
    path_str = path_str.replace(' ', "%20");
    if path_str.starts_with('/') {
        format!("file://{}", path_str)
    } else {
        format!("file:///{}", path_str)
    }
}

fn find_session_id(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            // Common shapes
            if let Some(Value::String(session_id)) = map.get("session_id") {
                return Some(session_id.clone());
            }
            if let Some(Value::String(session_id)) = map.get("sessionId") {
                return Some(session_id.clone());
            }
            if let Some(Value::String(session_id)) = map.get("session") {
                return Some(session_id.clone());
            }
            // OpenCode CLI run-json uses `sessionID`
            if let Some(Value::String(session_id)) = map.get("sessionID") {
                return Some(session_id.clone());
            }
            // OpenCode sometimes nests session object
            if let Some(Value::Object(sess)) = map.get("session") {
                if let Some(Value::String(id)) = sess.get("id") {
                    return Some(id.clone());
                }
                if let Some(Value::String(id)) = sess.get("session_id") {
                    return Some(id.clone());
                }
            }
            for entry in map.values() {
                if let Some(found) = find_session_id(entry) {
                    return Some(found);
                }
            }
        }
        Value::Array(values) => {
            for entry in values {
                if let Some(found) = find_session_id(entry) {
                    return Some(found);
                }
            }
        }
        _ => {}
    }
    None
}

fn parse_opencode_event(value: &Value) -> Vec<StreamingUpdate> {
    // OpenCode emits one JSON line per event in `opencode run --format json`:
    // { type, timestamp, sessionID, ... }
    // and for most events includes `part`.
    let mut out: Vec<StreamingUpdate> = Vec::new();

    let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match event_type {
        "text" => {
            if let Some(text) = value
                .get("part")
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
            {
                // Don't add extra newline - text already includes formatting
                out.push(StreamingUpdate::TextChunk {
                    text: text.to_string(),
                    item_id: None,
                });
            }
        }
        "tool_use" => {
            let part = value.get("part");
            let tool = part
                .and_then(|p| p.get("tool"))
                .and_then(|t| t.as_str())
                .unwrap_or("tool")
                .to_string();

            // Get tool title if available (more readable)
            let title = part
                .and_then(|p| p.get("state"))
                .and_then(|s| s.get("input"))
                .and_then(|i| i.get("description").or_else(|| i.get("title")))
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            let input = part
                .and_then(|p| p.get("state"))
                .and_then(|s| s.get("input"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            // Use title as display name if available
            let display_name = title.unwrap_or_else(|| tool.clone());
            out.push(StreamingUpdate::ToolCall {
                name: display_name,
                arguments: input.to_string(),
            });

            let output = part
                .and_then(|p| p.get("state"))
                .and_then(|s| s.get("output"))
                .and_then(|o| o.as_str())
                .unwrap_or("")
                .to_string();
            if !output.trim().is_empty() {
                out.push(StreamingUpdate::ToolReturn { output });
            }
        }
        "step_start" => {
            out.push(StreamingUpdate::Status {
                message: "OpenCode is working".to_string(),
            });
        }
        "step_finish" => {
            // Extract finish reason if available
            let reason = value
                .get("part")
                .and_then(|p| p.get("reason"))
                .and_then(|r| r.as_str())
                .unwrap_or("done");
            let msg = match reason {
                "stop" => "Completed".to_string(),
                "tool-calls" => "Processing tool results...".to_string(),
                _ => format!("Step finished ({})", reason),
            };
            out.push(StreamingUpdate::Status { message: msg });
        }
        "error" => {
            let msg = value
                .get("error")
                .and_then(|e| e.get("data"))
                .and_then(|d| d.get("message"))
                .and_then(|m| m.as_str())
                .or_else(|| {
                    value
                        .get("error")
                        .and_then(|e| e.get("name"))
                        .and_then(|m| m.as_str())
                })
                .unwrap_or("opencode error")
                .to_string();
            out.push(StreamingUpdate::Status { message: msg });
        }
        _ => {
            // ignore unknown event types
        }
    }

    out
}

/// Parse Amp's stream-json events into Phantom's StreamingUpdate format.
/// See: https://ampcode.com/manual/appendix#message-schema
///
/// Amp emits NDJSON events with the following structure:
/// - `system`: Init event with session_id, tools, mcp_servers
/// - `assistant`: Message with content blocks (text, tool_use, thinking)
/// - `user`: Tool results
/// - `result`: Final result (success, error_during_execution, error_max_turns)
fn parse_amp_event(value: &Value) -> Vec<StreamingUpdate> {
    let mut updates: Vec<StreamingUpdate> = Vec::new();

    let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match event_type {
        "system" => {
            // Init event with session_id, tools, mcp_servers
            let subtype = value.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
            if subtype == "init" {
                if let Some(session_id) = value.get("session_id").and_then(|v| v.as_str()) {
                    eprintln!("[Harness][Amp] session_id: {}", session_id);
                }
                updates.push(StreamingUpdate::Status {
                    message: "Initializing Amp…".to_string(),
                });
            }
        }
        "assistant" => {
            // Extract content blocks from message.content array
            if let Some(content) = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in content {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                updates.push(StreamingUpdate::TextChunk {
                                    text: text.to_string(),
                                    item_id: None,
                                });
                            }
                        }
                        "thinking" => {
                            // Thinking blocks from --stream-json-thinking
                            if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                                updates.push(StreamingUpdate::ReasoningChunk {
                                    text: thinking.to_string(),
                                });
                            }
                        }
                        "redacted_thinking" => {
                            // Redacted thinking blocks (we skip the actual data)
                            updates.push(StreamingUpdate::ReasoningChunk {
                                text: "[thinking redacted]".to_string(),
                            });
                        }
                        "tool_use" => {
                            let name = block
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("tool")
                                .to_string();
                            let id = block
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("")
                                .to_string();
                            let input = block.get("input").cloned().unwrap_or(Value::Null);
                            updates.push(StreamingUpdate::ToolCall {
                                name,
                                arguments: serde_json::to_string(&input).unwrap_or_default(),
                            });
                            // Store the call_id for potential tool result correlation
                            if !id.is_empty() {
                                eprintln!("[Harness][Amp] tool_use id: {}", id);
                            }
                        }
                        _ => {
                            // Unknown block type - log for debugging
                            eprintln!("[Harness][Amp] unknown block type: {}", block_type);
                        }
                    }
                }
            }
        }
        "user" => {
            // User messages contain tool results
            if let Some(content) = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in content {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if block_type == "tool_result" {
                        // Content can be a string or a structured object
                        let output = block
                            .get("content")
                            .map(|c| {
                                if let Some(s) = c.as_str() {
                                    s.to_string()
                                } else {
                                    c.to_string()
                                }
                            })
                            .unwrap_or_default();
                        if !output.is_empty() {
                            updates.push(StreamingUpdate::ToolReturn { output });
                        }
                    }
                }
            }
        }
        "result" => {
            // Final result event - NOTE: Do NOT emit the `result` text here!
            // It duplicates what was already streamed in the `assistant` event.
            // The `result` field is just a summary of the final response.
            let subtype = value.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
            let is_error = subtype.starts_with("error");

            // Only emit error messages (these are NOT duplicates of assistant text)
            if is_error {
                if let Some(error) = value.get("error").and_then(|e| e.as_str()) {
                    updates.push(StreamingUpdate::Status {
                        message: format!("Error: {}", error),
                    });
                }
            }

            // Status update based on subtype
            let status = match (is_error, subtype) {
                (true, "error_during_execution") => "Failed (execution error)".to_string(),
                (true, "error_max_turns") => "Failed (max turns reached)".to_string(),
                (true, st) => format!("Failed ({})", st),
                (false, "success") => "Completed".to_string(),
                (false, st) if !st.is_empty() => format!("Completed ({})", st),
                (false, _) => "Completed".to_string(),
            };
            updates.push(StreamingUpdate::Status { message: status });

            // Log usage stats if present
            if let Some(usage) = value.get("usage") {
                eprintln!("[Harness][Amp] usage: {}", usage);
            }
            if let Some(duration) = value.get("duration_ms").and_then(|d| d.as_i64()) {
                eprintln!("[Harness][Amp] duration: {}ms", duration);
            }
        }
        _ => {
            // Unknown event type - log for debugging during development
            if !event_type.is_empty() {
                eprintln!("[Harness][Amp] unhandled event type: {}", event_type);
            }
        }
    }

    updates
}

/// Parse Factory Droid's stream-json events into Phantom's StreamingUpdate format.
/// Droid emits NDJSON with --output-format stream-json
///
/// Event types:
/// - `type: "message"` - Assistant message with `text`, `role`, `id`, `session_id`
/// - `type: "completion"` - Final result with `finalText`, `numTurns`, `durationMs`, `usage`
/// - `type: "tool_call"` - Tool invocation with `name`, `arguments`
/// - `type: "tool_result"` - Tool output with `output`
fn parse_droid_event(value: &Value) -> Vec<StreamingUpdate> {
    let mut updates = Vec::new();

    let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match event_type {
        "message" => {
            // Assistant message: {"type":"message","role":"assistant","text":"...","session_id":"..."}
            if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
                if !text.is_empty() {
                    updates.push(StreamingUpdate::TextChunk {
                        text: text.to_string(),
                        item_id: None,
                    });
                }
            }
        }
        "tool_call" => {
            // Tool call from Droid: {"type":"tool_call","toolName":"Grep","parameters":{...},...}
            // Also support generic: {"type":"tool_call","name":"...","arguments":...}
            let name = value
                .get("toolName")
                .or_else(|| value.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("tool")
                .to_string();
            let arguments = value
                .get("parameters")
                .or_else(|| value.get("arguments"))
                .map(|a| {
                    if let Some(s) = a.as_str() {
                        s.to_string()
                    } else {
                        a.to_string()
                    }
                })
                .unwrap_or_else(|| "{}".to_string());
            updates.push(StreamingUpdate::ToolCall { name, arguments });
        }
        "tool_result" => {
            // Tool result: {"type":"tool_result","output":"..."}
            let output = value
                .get("output")
                .or_else(|| value.get("content"))
                .map(|o| {
                    if let Some(s) = o.as_str() {
                        s.to_string()
                    } else {
                        o.to_string()
                    }
                })
                .unwrap_or_default();
            if !output.is_empty() {
                updates.push(StreamingUpdate::ToolReturn { output });
            }
        }
        "thinking" | "reasoning" => {
            // Reasoning/thinking blocks
            if let Some(text) = value
                .get("text")
                .or_else(|| value.get("thinking"))
                .or_else(|| value.get("reasoning"))
                .and_then(|t| t.as_str())
            {
                if !text.is_empty() {
                    updates.push(StreamingUpdate::ReasoningChunk {
                        text: text.to_string(),
                    });
                }
            }
        }
        "system" | "init" => {
            // System init event - show status so UI doesn't freeze
            updates.push(StreamingUpdate::Status {
                message: "Initializing Droid…".to_string(),
            });
        }
        "status" => {
            // Status update
            if let Some(msg) = value.get("message").and_then(|m| m.as_str()) {
                updates.push(StreamingUpdate::Status {
                    message: msg.to_string(),
                });
            }
        }
        "completion" => {
            // Final completion event: {"type":"completion","finalText":"...","numTurns":...,"durationMs":...,"usage":...}
            // Note: We don't emit finalText as TextChunk because it duplicates the message text
            // Just log usage stats for debugging
            if let Some(usage) = value.get("usage") {
                eprintln!("[Harness][Droid] usage: {}", usage);
            }
            if let Some(duration) = value.get("durationMs").and_then(|d| d.as_i64()) {
                eprintln!("[Harness][Droid] duration: {}ms", duration);
            }
            if let Some(turns) = value.get("numTurns").and_then(|n| n.as_i64()) {
                eprintln!("[Harness][Droid] turns: {}", turns);
            }
            updates.push(StreamingUpdate::Status {
                message: "Completed".to_string(),
            });
        }
        "error" => {
            // Error event
            let error_msg = value
                .get("error")
                .or_else(|| value.get("message"))
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error");
            updates.push(StreamingUpdate::Status {
                message: format!("Error: {}", error_msg),
            });
        }
        _ => {
            // Unknown event type - log for debugging
            if !event_type.is_empty() {
                eprintln!("[Harness][Droid] unhandled event type: {}", event_type);
            }
        }
    }

    updates
}

fn parse_streaming_updates(value: &Value) -> Vec<StreamingUpdate> {
    // Claude Code stream-json events are NDJSON objects with top-level `type`.
    // Most useful payload is under:
    // - assistant -> message.content[] (text/tool_use)
    // - result -> result
    // We still support a couple legacy shapes for other CLIs.

    let event_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("event").and_then(|v| v.as_str()))
        .unwrap_or("");

    let mut out: Vec<StreamingUpdate> = Vec::new();

    match event_type {
        "assistant" => {
            let content = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array());

            if let Some(items) = content {
                for item in items {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match item_type {
                        "text" => {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                out.push(StreamingUpdate::TextChunk {
                                    text: text.to_string(),
                                    item_id: None,
                                });
                            }
                        }
                        "tool_use" => {
                            let name = item
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("tool")
                                .to_string();
                            let input = item.get("input");
                            let arguments = input
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "{}".to_string());

                            // Detect plan.md file creation from Write/Create tools
                            let name_lower = name.to_lowercase();
                            if name_lower == "write"
                                || name_lower == "create"
                                || name_lower == "proxy_create"
                            {
                                if let Some(input_obj) = input {
                                    let file_path = input_obj
                                        .get("file_path")
                                        .or_else(|| input_obj.get("filePath"))
                                        .or_else(|| input_obj.get("path"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let content = input_obj
                                        .get("content")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if file_path.to_lowercase().ends_with("plan.md")
                                        && !content.is_empty()
                                    {
                                        out.push(StreamingUpdate::PlanContent {
                                            file_path: file_path.to_string(),
                                            content: content.to_string(),
                                        });
                                    }
                                }
                            }

                            out.push(StreamingUpdate::ToolCall { name, arguments });
                        }
                        "tool_result" => {
                            let output = item
                                .get("content")
                                .or_else(|| item.get("output"))
                                .map(|v| v.to_string())
                                .unwrap_or_default();
                            out.push(StreamingUpdate::ToolReturn { output });
                        }
                        // Claude can emit reasoning as a separate item type in some modes.
                        "thinking" | "reasoning" => {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                out.push(StreamingUpdate::ReasoningChunk {
                                    text: text.to_string(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "system" => {
            // surface something so UI doesn't look frozen during init/hooks
            if let Some(subtype) = value.get("subtype").and_then(|v| v.as_str()) {
                if subtype == "init" {
                    out.push(StreamingUpdate::Status {
                        message: "Initializing Claude…".to_string(),
                    });
                } else if subtype.starts_with("hook_") {
                    out.push(StreamingUpdate::Status {
                        message: "Loading context…".to_string(),
                    });
                }
            }
        }
        "result" => {
            // include subtype if present (e.g. error_max_turns)
            let subtype = value.get("subtype").and_then(|v| v.as_str());
            let is_error = value
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let status = match (is_error, subtype) {
                (true, Some(st)) => format!("Failed ({})", st),
                (true, None) => "Failed".to_string(),
                (false, Some(st)) => format!("Completed ({})", st),
                (false, None) => "Completed".to_string(),
            };
            out.push(StreamingUpdate::Status { message: status });
        }
        // Legacy / fallback
        "tool_call" | "tool_use" => {
            let name = value
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool")
                .to_string();
            let arguments = value
                .get("arguments")
                .or_else(|| value.get("input"))
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".to_string());
            out.push(StreamingUpdate::ToolCall { name, arguments });
        }
        "tool_result" | "tool_output" => {
            let output = value
                .get("output")
                .or_else(|| value.get("content"))
                .map(|v| v.to_string())
                .unwrap_or_default();
            out.push(StreamingUpdate::ToolReturn { output });
        }
        "reasoning" | "thinking" => {
            if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
                out.push(StreamingUpdate::ReasoningChunk {
                    text: text.to_string(),
                });
            }
        }
        "status" => {
            if let Some(message) = value.get("message").and_then(|v| v.as_str()) {
                out.push(StreamingUpdate::Status {
                    message: message.to_string(),
                });
            }
        }
        "permission_request" => {
            let request_id = value
                .get("request_id")
                .or_else(|| value.get("requestId"))
                .and_then(|v| v.as_str())
                .unwrap_or("permission")
                .to_string();
            let tool_name = value
                .get("tool_name")
                .or_else(|| value.get("toolName"))
                .and_then(|v| v.as_str())
                .unwrap_or("tool")
                .to_string();
            let description = value
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let raw_input = value
                .get("raw_input")
                .or_else(|| value.get("input"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let options = value
                .get("options")
                .and_then(|v| serde_json::from_value::<Vec<PermissionOption>>(v.clone()).ok())
                .unwrap_or_default();
            out.push(StreamingUpdate::PermissionRequest {
                request_id,
                tool_name,
                description,
                raw_input,
                options,
            });
        }
        "available_commands" => {
            let commands = value
                .get("commands")
                .and_then(|v| serde_json::from_value::<Vec<AvailableCommand>>(v.clone()).ok())
                .unwrap_or_default();
            out.push(StreamingUpdate::AvailableCommands { commands });
        }
        _ => {
            if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
                out.push(StreamingUpdate::TextChunk {
                    text: text.to_string(),
                    item_id: None,
                });
            }
        }
    }

    out
}

fn parse_prompt_messages(
    value: &Value,
    current_assistant_text: &mut String,
    current_reasoning_text: &mut String,
) -> Vec<PromptMessage> {
    let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let mut out: Vec<PromptMessage> = Vec::new();

    match event_type {
        "assistant" => {
            let content = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array());

            if let Some(items) = content {
                for item in items {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match item_type {
                        "text" => {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                current_assistant_text.push_str(text);
                            }
                        }
                        "tool_use" => {
                            out.push(PromptMessage {
                                message_type: "tool_call".to_string(),
                                content: None,
                                reasoning: None,
                                name: item
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                arguments: item.get("input").map(|v| v.to_string()),
                                tool_return: None,
                            });
                        }
                        "tool_result" => {
                            out.push(PromptMessage {
                                message_type: "tool_return".to_string(),
                                content: None,
                                reasoning: None,
                                name: None,
                                arguments: None,
                                tool_return: item
                                    .get("content")
                                    .or_else(|| item.get("output"))
                                    .map(|v| v.to_string()),
                            });
                        }
                        "thinking" | "reasoning" => {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                current_reasoning_text.push_str(text);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "result" => {
            if let Some(result) = value.get("result") {
                let text = result
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| result.to_string());
                current_assistant_text.clear();
                current_assistant_text.push_str(&text);
            }
        }
        // Legacy / fallback
        "reasoning" | "thinking" => {
            if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
                current_reasoning_text.push_str(text);
            }
        }
        "tool_call" | "tool_use" => {
            out.push(PromptMessage {
                message_type: "tool_call".to_string(),
                content: None,
                reasoning: None,
                name: value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                arguments: value
                    .get("arguments")
                    .or_else(|| value.get("input"))
                    .map(|v| v.to_string()),
                tool_return: None,
            });
        }
        "tool_result" | "tool_output" => {
            out.push(PromptMessage {
                message_type: "tool_return".to_string(),
                content: None,
                reasoning: None,
                name: None,
                arguments: None,
                tool_return: value
                    .get("output")
                    .or_else(|| value.get("content"))
                    .map(|v| v.to_string()),
            });
        }
        _ => {
            if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
                current_assistant_text.push_str(text);
            }
        }
    }

    out
}

fn parse_token_usage(value: &Value, session_id: Option<String>) -> Option<TokenUsageInfo> {
    let usage = value
        .get("usage")
        .or_else(|| value.get("token_usage"))
        .or_else(|| value.get("message").and_then(|m| m.get("usage")))?;
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("inputTokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("outputTokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cached_input_tokens = usage
        .get("cached_input_tokens")
        .or_else(|| usage.get("cachedInputTokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let reasoning_output_tokens = usage
        .get("reasoning_output_tokens")
        .or_else(|| usage.get("reasoningOutputTokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .or_else(|| usage.get("totalTokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(input_tokens + output_tokens);

    Some(TokenUsageInfo {
        session_id,
        last_token_usage: TokenUsage {
            input_tokens,
            output_tokens,
            cached_input_tokens,
            reasoning_output_tokens,
            total_tokens,
        },
        total_token_usage: TokenUsage {
            input_tokens,
            output_tokens,
            cached_input_tokens,
            reasoning_output_tokens,
            total_tokens,
        },
        model_context_window: None,
    })
}

/// Parse token usage from Codex app-server's thread/tokenUsage/updated notification.
///
/// The notification contains:
/// - `last`: token counts for the current turn
/// - `total`: cumulative token counts across the session
/// - `modelContextWindow`: optional context window size
fn parse_codex_token_usage(value: &Value) -> Option<TokenUsageInfo> {
    // Parse last (current turn) usage
    let last = value.get("last")?;
    let last_usage = TokenUsage {
        input_tokens: last
            .get("input_tokens")
            .or_else(|| last.get("inputTokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        output_tokens: last
            .get("output_tokens")
            .or_else(|| last.get("outputTokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        cached_input_tokens: last
            .get("cached_input_tokens")
            .or_else(|| last.get("cachedInputTokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        reasoning_output_tokens: last
            .get("reasoning_output_tokens")
            .or_else(|| last.get("reasoningOutputTokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        total_tokens: last
            .get("total_tokens")
            .or_else(|| last.get("totalTokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
    };

    // Parse total (cumulative) usage
    let total = value.get("total");
    let total_usage = total
        .map(|t| TokenUsage {
            input_tokens: t
                .get("input_tokens")
                .or_else(|| t.get("inputTokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            output_tokens: t
                .get("output_tokens")
                .or_else(|| t.get("outputTokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            cached_input_tokens: t
                .get("cached_input_tokens")
                .or_else(|| t.get("cachedInputTokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            reasoning_output_tokens: t
                .get("reasoning_output_tokens")
                .or_else(|| t.get("reasoningOutputTokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            total_tokens: t
                .get("total_tokens")
                .or_else(|| t.get("totalTokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        })
        .unwrap_or_else(|| last_usage.clone());

    Some(TokenUsageInfo {
        session_id: None,
        last_token_usage: last_usage,
        total_token_usage: total_usage,
        model_context_window: value
            .get("model_context_window")
            .or_else(|| value.get("modelContextWindow"))
            .and_then(|v| v.as_i64()),
    })
}
