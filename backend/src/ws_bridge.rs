use crate::cli::PermissionOption;
use crate::ws_types::{
    BrowserIncomingMessage, BrowserOutgoingMessage, McpServerState, PermissionRequest, SessionState,
};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::handshake::server::ErrorResponse;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{accept_hdr_async, WebSocketStream};

const MAX_MESSAGE_HISTORY: usize = 200;
const MAX_PENDING_CLI_MESSAGES: usize = 1000;
const CLI_BROADCAST_CAPACITY: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketKind {
    Cli,
    Browser,
}

#[derive(Debug, Clone)]
struct Peer {
    tx: mpsc::UnboundedSender<Message>,
}

#[derive(Debug)]
struct Session {
    id: String,
    cli: Mutex<Option<Peer>>,
    browsers: Mutex<HashMap<usize, Peer>>,
    next_browser_id: AtomicUsize,
    state: RwLock<SessionState>,
    pending_permissions: Mutex<HashMap<String, PermissionRequest>>,
    message_history: Mutex<VecDeque<BrowserIncomingMessage>>,
    pending_cli_messages: Mutex<VecDeque<String>>,
    cli_broadcast: broadcast::Sender<Value>,
}

impl Session {
    fn new(id: &str) -> Self {
        let (tx, _rx) = broadcast::channel::<Value>(CLI_BROADCAST_CAPACITY);
        Self {
            id: id.to_string(),
            cli: Mutex::new(None),
            browsers: Mutex::new(HashMap::new()),
            next_browser_id: AtomicUsize::new(1),
            state: RwLock::new(SessionState {
                session_id: id.to_string(),
                model: "".to_string(),
                cwd: "".to_string(),
                tools: Vec::new(),
                permission_mode: "default".to_string(),
                claude_code_version: "".to_string(),
                mcp_servers: Vec::new(),
                agents: Vec::new(),
                slash_commands: Vec::new(),
                skills: Vec::new(),
                total_cost_usd: 0.0,
                num_turns: 0,
                context_used_percent: 0,
                is_compacting: false,
            }),
            pending_permissions: Mutex::new(HashMap::new()),
            message_history: Mutex::new(VecDeque::new()),
            pending_cli_messages: Mutex::new(VecDeque::new()),
            cli_broadcast: tx,
        }
    }

    fn subscribe_cli(&self) -> broadcast::Receiver<Value> {
        self.cli_broadcast.subscribe()
    }

    async fn set_cli(&self, peer: Peer) {
        let mut guard = self.cli.lock().await;
        *guard = Some(peer);
    }

    async fn clear_cli(&self) {
        let mut guard = self.cli.lock().await;
        *guard = None;
    }

    async fn add_browser(&self, peer: Peer) -> usize {
        let id = self.next_browser_id.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.browsers.lock().await;
        guard.insert(id, peer);
        id
    }

    async fn remove_browser(&self, id: usize) {
        let mut guard = self.browsers.lock().await;
        guard.remove(&id);
    }

    async fn push_history(&self, msg: BrowserIncomingMessage) {
        let mut guard = self.message_history.lock().await;
        guard.push_back(msg);
        while guard.len() > MAX_MESSAGE_HISTORY {
            guard.pop_front();
        }
    }

    async fn snapshot_history(&self) -> Vec<BrowserIncomingMessage> {
        let guard = self.message_history.lock().await;
        guard.iter().cloned().collect()
    }

    async fn queue_cli_message(&self, ndjson_line: String) {
        let mut guard = self.pending_cli_messages.lock().await;
        guard.push_back(ndjson_line);
        while guard.len() > MAX_PENDING_CLI_MESSAGES {
            let _ = guard.pop_front();
        }
    }

    async fn drain_queued_cli_messages(&self) -> Vec<String> {
        let mut guard = self.pending_cli_messages.lock().await;
        guard.drain(..).collect()
    }
}

#[derive(Debug)]
pub struct WsBridge {
    port: u16,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl WsBridge {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn ensure_started() -> Result<Arc<WsBridge>> {
        static CELL: tokio::sync::OnceCell<Arc<WsBridge>> = tokio::sync::OnceCell::const_new();
        CELL.get_or_try_init(|| async { WsBridge::start().await })
            .await
            .map(|v| v.clone())
    }

    async fn start() -> Result<Arc<WsBridge>> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("bind ws bridge")?;
        let port = listener
            .local_addr()
            .context("ws bridge local_addr")?
            .port();

        let bridge = Arc::new(WsBridge {
            port,
            sessions: Mutex::new(HashMap::new()),
        });

        let bridge_accept = bridge.clone();
        tokio::spawn(async move {
            loop {
                let (stream, addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[WsBridge] accept error: {}", e);
                        continue;
                    }
                };
                let bridge_conn = bridge_accept.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(bridge_conn, stream, addr).await {
                        eprintln!("[WsBridge] connection error: {}", e);
                    }
                });
            }
        });

        eprintln!("[WsBridge] listening on ws://127.0.0.1:{}", port);
        Ok(bridge)
    }

    async fn session(&self, session_id: &str) -> Arc<Session> {
        let mut guard = self.sessions.lock().await;
        if let Some(s) = guard.get(session_id) {
            return s.clone();
        }
        let s = Arc::new(Session::new(session_id));
        guard.insert(session_id.to_string(), s.clone());
        s
    }

    pub async fn subscribe_cli(&self, session_id: &str) -> broadcast::Receiver<Value> {
        self.session(session_id).await.subscribe_cli()
    }

    pub async fn send_to_cli(&self, session_id: &str, ndjson_line: &str) -> Result<()> {
        let session = self.session(session_id).await;
        let peer_opt = { session.cli.lock().await.clone() };
        if let Some(peer) = peer_opt {
            // NDJSON requires newline delimiter.
            let text = format!("{}\n", ndjson_line);
            if peer.tx.send(Message::Text(text.into())).is_ok() {
                return Ok(());
            }
        }

        session.queue_cli_message(ndjson_line.to_string()).await;
        Ok(())
    }

    pub async fn respond_permission_simple(
        &self,
        session_id: &str,
        request_id: &str,
        response_id: &str,
    ) -> Result<()> {
        let session = self.session(session_id).await;
        let pending = {
            let mut guard = session.pending_permissions.lock().await;
            guard.remove(request_id)
        };

        let behavior = if response_id.eq_ignore_ascii_case("deny") {
            "deny"
        } else {
            "allow"
        };

        let ndjson = if behavior == "allow" {
            let updated_input = pending
                .as_ref()
                .map(|p| p.input.clone())
                .unwrap_or_else(|| json!({}));
            json!({
              "type": "control_response",
              "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": {
                  "behavior": "allow",
                  "updatedInput": updated_input
                }
              }
            })
            .to_string()
        } else {
            json!({
              "type": "control_response",
              "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": {
                  "behavior": "deny",
                  "message": "Denied by user"
                }
              }
            })
            .to_string()
        };

        self.send_to_cli(session_id, &ndjson).await
    }

    async fn broadcast_to_browsers(
        &self,
        session: &Arc<Session>,
        msg: &BrowserIncomingMessage,
    ) {
        let payload = match serde_json::to_string(msg) {
            Ok(s) => s,
            Err(_) => return,
        };
        let peers = { session.browsers.lock().await.clone() };
        for (id, peer) in peers {
            if peer.tx.send(Message::Text(payload.clone().into())).is_err() {
                session.remove_browser(id).await;
            }
        }
    }
}

async fn handle_connection(
    bridge: Arc<WsBridge>,
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
) -> Result<()> {
    let mut meta: Option<(SocketKind, String)> = None;
    let mut last_request_id: Option<String> = None;

    let ws_stream: WebSocketStream<tokio::net::TcpStream> = accept_hdr_async(
        stream,
        |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
         resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
            let path = req.uri().path();
            if let Some((kind, session_id)) = parse_ws_path(path) {
                meta = Some((kind, session_id.to_string()));
            } else {
                return Err(ErrorResponse::new(Some(
                    "invalid websocket path".to_string(),
                )));
            }

            if let Some(value) = req
                .headers()
                .get("x-last-request-id")
                .and_then(|h| h.to_str().ok())
            {
                last_request_id = Some(value.to_string());
            }

            let _ = req
                .headers()
                .get("authorization")
                .and_then(|h| h.to_str().ok())
                .map(|auth| auth.to_string());

            Ok(resp)
        },
    )
    .await
    .context("ws accept")?;

    let (kind, session_id) = meta.context("missing ws meta")?;
    let session = bridge.session(&session_id).await;

    let (mut ws_write, mut ws_read) = ws_stream.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

    // Write pump.
    tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if ws_write.send(msg).await.is_err() {
                break;
            }
        }
    });

    let peer = Peer { tx: out_tx.clone() };

    match kind {
        SocketKind::Cli => {
            eprintln!(
                "[WsBridge] CLI connected session={} from={}",
                session_id, addr
            );
            session.set_cli(peer).await;
            bridge
                .broadcast_to_browsers(&session, &BrowserIncomingMessage::CliConnected)
                .await;

            // Best-effort replay support: we don't implement UUID-based replay yet,
            // but we do flush any queued messages (startup / UI sent before connect).
            // `X-Last-Request-Id` is recorded for future implementation.
            let _ = last_request_id;

            for line in session.drain_queued_cli_messages().await {
                let _ = out_tx.send(Message::Text(format!("{}\n", line).into()));
            }
        }
        SocketKind::Browser => {
            eprintln!(
                "[WsBridge] browser connected session={} from={}",
                session_id, addr
            );
            let browser_id = session.add_browser(peer).await;

            // Snapshot session state.
            let snapshot = { session.state.read().await.clone() };
            let _ = out_tx.send(Message::Text(
                serde_json::to_string(&BrowserIncomingMessage::SessionInit { session: snapshot })
                    .unwrap_or_else(|_| "{}".to_string())
                    .into(),
            ));

            // Replay history.
            let history = session.snapshot_history().await;
            if !history.is_empty() {
                let _ = out_tx.send(Message::Text(
                    serde_json::to_string(&BrowserIncomingMessage::MessageHistory {
                        messages: history,
                    })
                    .unwrap_or_else(|_| "{}".to_string())
                    .into(),
                ));
            }

            // Pending permission requests.
            let pending = { session.pending_permissions.lock().await.clone() };
            for perm in pending.values() {
                let _ = out_tx.send(Message::Text(
                    serde_json::to_string(&BrowserIncomingMessage::PermissionRequest {
                        request: perm.clone(),
                    })
                    .unwrap_or_else(|_| "{}".to_string())
                    .into(),
                ));
            }

            // Notify if CLI is not connected.
            let cli_connected = { session.cli.lock().await.is_some() };
            if !cli_connected {
                let _ = out_tx.send(Message::Text(
                    serde_json::to_string(&BrowserIncomingMessage::CliDisconnected)
                        .unwrap_or_else(|_| "{}".to_string())
                        .into(),
                ));
            }

            // Read loop for browser.
            while let Some(msg) = ws_read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(parsed) = serde_json::from_str::<BrowserOutgoingMessage>(&text) {
                            route_browser_message(&bridge, &session, parsed).await;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }

            session.remove_browser(browser_id).await;
            eprintln!(
                "[WsBridge] browser disconnected session={} from={}",
                session_id, addr
            );
            return Ok(());
        }
    }

    // Read loop for CLI.
    while let Some(msg) = ws_read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                handle_cli_payload(&bridge, &session, &text).await;
            }
            Ok(Message::Binary(bin)) => {
                if let Ok(text) = String::from_utf8(bin.to_vec()) {
                    handle_cli_payload(&bridge, &session, &text).await;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }

    // CLI disconnected.
    session.clear_cli().await;
    eprintln!(
        "[WsBridge] CLI disconnected session={} from={}",
        session_id, addr
    );
    bridge
        .broadcast_to_browsers(&session, &BrowserIncomingMessage::CliDisconnected)
        .await;

    // Cancel pending permissions.
    let pending_ids: Vec<String> = {
        let mut guard = session.pending_permissions.lock().await;
        let ids = guard.keys().cloned().collect::<Vec<_>>();
        guard.clear();
        ids
    };
    for req_id in pending_ids {
        bridge
            .broadcast_to_browsers(
                &session,
                &BrowserIncomingMessage::PermissionCancelled { request_id: req_id },
            )
            .await;
    }

    Ok(())
}

fn parse_ws_path(path: &str) -> Option<(SocketKind, &str)> {
    let mut it = path.split('/').filter(|p| !p.is_empty());
    let a = it.next()?;
    let b = it.next()?;
    let c = it.next()?;
    if it.next().is_some() {
        return None;
    }
    if a != "ws" {
        return None;
    }
    match b {
        "cli" => Some((SocketKind::Cli, c)),
        "browser" => Some((SocketKind::Browser, c)),
        _ => None,
    }
}

async fn handle_cli_payload(bridge: &Arc<WsBridge>, session: &Arc<Session>, data: &str) {
    let lines = data.split('\n').filter(|l| !l.trim().is_empty());
    for line in lines {
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Fan out raw CLI messages for internal consumers (AgentProcessClient).
        let _ = session.cli_broadcast.send(value.clone());
        route_cli_message(bridge, session, &value).await;
    }
}

async fn route_cli_message(bridge: &Arc<WsBridge>, session: &Arc<Session>, value: &Value) {
    let ty = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match ty {
        "system" => {
            let subtype = value.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
            if subtype == "init" {
                let model = value
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let cwd = value
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tools = value
                    .get("tools")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let permission_mode = value
                    .get("permissionMode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                let claude_code_version = value
                    .get("claude_code_version")
                    .or_else(|| value.get("claudeCodeVersion"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mcp_servers = value
                    .get("mcp_servers")
                    .or_else(|| value.get("mcpServers"))
                    .and_then(|v| serde_json::from_value::<Vec<McpServerState>>(v.clone()).ok())
                    .unwrap_or_default();
                let agents = value
                    .get("agents")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let slash_commands = value
                    .get("slash_commands")
                    .or_else(|| value.get("slashCommands"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let skills = value
                    .get("skills")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                {
                    let mut st = session.state.write().await;
                    st.model = model;
                    st.cwd = cwd;
                    st.tools = tools;
                    st.permission_mode = permission_mode;
                    st.claude_code_version = claude_code_version;
                    st.mcp_servers = mcp_servers;
                    st.agents = agents;
                    st.slash_commands = slash_commands;
                    st.skills = skills;
                }

                let snapshot = { session.state.read().await.clone() };
                bridge
                    .broadcast_to_browsers(
                        session,
                        &BrowserIncomingMessage::SessionInit { session: snapshot },
                    )
                    .await;
            } else if subtype == "status" {
                let status = value
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(mode) = value.get("permissionMode").and_then(|v| v.as_str()) {
                    let mut st = session.state.write().await;
                    st.permission_mode = mode.to_string();
                }

                {
                    let mut st = session.state.write().await;
                    st.is_compacting = status.as_deref() == Some("compacting");
                }

                bridge
                    .broadcast_to_browsers(
                        session,
                        &BrowserIncomingMessage::StatusChange { status },
                    )
                    .await;
            }
        }
        "assistant" => {
            if let Some(message) = value.get("message") {
                let parent_tool_use_id = value
                    .get("parent_tool_use_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let browser_msg = BrowserIncomingMessage::Assistant {
                    message: message.clone(),
                    parent_tool_use_id,
                };
                session.push_history(browser_msg.clone()).await;
                bridge.broadcast_to_browsers(session, &browser_msg).await;
            }
        }
        "stream_event" => {
            let parent_tool_use_id = value
                .get("parent_tool_use_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let evt = value.get("event").cloned().unwrap_or_else(|| json!({}));
            bridge
                .broadcast_to_browsers(
                    session,
                    &BrowserIncomingMessage::StreamEvent {
                        event: evt,
                        parent_tool_use_id,
                    },
                )
                .await;
        }
        "result" => {
            // Update session cost/turns/context.
            if let Some(total_cost) = value.get("total_cost_usd").and_then(|v| v.as_f64()) {
                let mut st = session.state.write().await;
                st.total_cost_usd = total_cost;
            }
            if let Some(num_turns) = value.get("num_turns").and_then(|v| v.as_i64()) {
                let mut st = session.state.write().await;
                st.num_turns = num_turns;
            }
            if let Some(model_usage) = value.get("modelUsage").and_then(|v| v.as_object()) {
                for usage in model_usage.values() {
                    let ctx = usage
                        .get("contextWindow")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let input = usage
                        .get("inputTokens")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let output = usage
                        .get("outputTokens")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    if ctx > 0.0 {
                        let mut st = session.state.write().await;
                        st.context_used_percent = (((input + output) / ctx) * 100.0).round() as i64;
                    }
                }
            }

            let browser_msg = BrowserIncomingMessage::Result {
                data: value.clone(),
            };
            session.push_history(browser_msg.clone()).await;
            bridge.broadcast_to_browsers(session, &browser_msg).await;
        }
        "control_request" => {
            let subtype = value
                .get("request")
                .and_then(|r| r.get("subtype"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if subtype == "can_use_tool" {
                let request_id = value
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_name = value
                    .get("request")
                    .and_then(|r| r.get("tool_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let input = value
                    .get("request")
                    .and_then(|r| r.get("input"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let description = value
                    .get("request")
                    .and_then(|r| r.get("description"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let tool_use_id = value
                    .get("request")
                    .and_then(|r| r.get("tool_use_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let agent_id = value
                    .get("request")
                    .and_then(|r| r.get("agent_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let permission_suggestions = value
                    .get("request")
                    .and_then(|r| r.get("permission_suggestions"))
                    .cloned();

                let options = vec![
                    PermissionOption {
                        id: "manual".to_string(),
                        label: "Allow".to_string(),
                        kind: Some("allow_once".to_string()),
                        shortcut: Some("Y".to_string()),
                        icon: Some("check".to_string()),
                        style: Some("primary".to_string()),
                    },
                    PermissionOption {
                        id: "deny".to_string(),
                        label: "Deny".to_string(),
                        kind: Some("reject_once".to_string()),
                        shortcut: Some("N".to_string()),
                        icon: Some("times".to_string()),
                        style: Some("danger".to_string()),
                    },
                ];

                let perm = PermissionRequest {
                    request_id: request_id.clone(),
                    tool_name,
                    input,
                    permission_suggestions,
                    options,
                    description,
                    tool_use_id,
                    agent_id,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };
                {
                    let mut guard = session.pending_permissions.lock().await;
                    guard.insert(request_id.clone(), perm.clone());
                }

                bridge
                    .broadcast_to_browsers(
                        session,
                        &BrowserIncomingMessage::PermissionRequest { request: perm },
                    )
                    .await;
            }
        }
        "tool_progress" => {
            let tool_use_id = value
                .get("tool_use_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool_name = value
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let elapsed = value
                .get("elapsed_time_seconds")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            bridge
                .broadcast_to_browsers(
                    session,
                    &BrowserIncomingMessage::ToolProgress {
                        tool_use_id,
                        tool_name,
                        elapsed_time_seconds: elapsed,
                    },
                )
                .await;
        }
        "tool_use_summary" => {
            let summary = value
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let ids = value
                .get("preceding_tool_use_ids")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            bridge
                .broadcast_to_browsers(
                    session,
                    &BrowserIncomingMessage::ToolUseSummary {
                        summary,
                        tool_use_ids: ids,
                    },
                )
                .await;
        }
        "auth_status" => {
            let is_authenticating = value
                .get("isAuthenticating")
                .or_else(|| value.get("is_authenticating"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let output = value
                .get("output")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let error = value
                .get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            bridge
                .broadcast_to_browsers(
                    session,
                    &BrowserIncomingMessage::AuthStatus {
                        is_authenticating,
                        output,
                        error,
                    },
                )
                .await;
        }
        "keep_alive" => {}
        _ => {}
    }
}

async fn route_browser_message(
    bridge: &Arc<WsBridge>,
    session: &Arc<Session>,
    msg: BrowserOutgoingMessage,
) {
    match msg {
        BrowserOutgoingMessage::UserMessage {
            content,
            session_id,
            images,
        } => {
            // Build `message.content`: string for text-only, or blocks when images present.
            let message_content = if let Some(images) = images {
                let mut blocks: Vec<Value> = Vec::new();
                for img in images {
                    blocks.push(json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": img.media_type, "data": img.data }
                    }));
                }
                blocks.push(json!({ "type": "text", "text": content }));
                Value::Array(blocks)
            } else {
                Value::String(content)
            };

            let ndjson = json!({
              "type": "user",
              "message": { "role": "user", "content": message_content },
              "parent_tool_use_id": Value::Null,
              "session_id": session_id.unwrap_or_else(|| "".to_string())
            })
            .to_string();
            let _ = bridge.send_to_cli(&session.id, &ndjson).await;
        }
        BrowserOutgoingMessage::PermissionResponse {
            request_id,
            behavior,
            updated_input,
            updated_permissions,
            message,
        } => {
            let mut resp = json!({
              "behavior": behavior,
            });
            if let Some(inp) = updated_input {
                resp["updatedInput"] = inp;
            }
            if let Some(perms) = updated_permissions {
                resp["updatedPermissions"] = perms;
            }
            if let Some(m) = message {
                resp["message"] = Value::String(m);
            }
            let ndjson = json!({
              "type": "control_response",
              "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": resp
              }
            })
            .to_string();
            let _ = bridge.send_to_cli(&session.id, &ndjson).await;
        }
        BrowserOutgoingMessage::Interrupt => {
            let ndjson = json!({
              "type": "control_request",
              "request_id": uuid::Uuid::new_v4().to_string(),
              "request": { "subtype": "interrupt" }
            })
            .to_string();
            let _ = bridge.send_to_cli(&session.id, &ndjson).await;
        }
        BrowserOutgoingMessage::SetModel { model } => {
            let ndjson = json!({
              "type": "control_request",
              "request_id": uuid::Uuid::new_v4().to_string(),
              "request": { "subtype": "set_model", "model": model }
            })
            .to_string();
            let _ = bridge.send_to_cli(&session.id, &ndjson).await;
        }
        BrowserOutgoingMessage::SetPermissionMode { mode } => {
            let ndjson = json!({
              "type": "control_request",
              "request_id": uuid::Uuid::new_v4().to_string(),
              "request": { "subtype": "set_permission_mode", "mode": mode }
            })
            .to_string();
            let _ = bridge.send_to_cli(&session.id, &ndjson).await;
        }
    }
}

/// Wait for the CLI socket to connect for a session.
pub async fn wait_for_cli_connected(session_id: &str, timeout_duration: Duration) -> Result<()> {
    let bridge = WsBridge::ensure_started().await?;
    let session = bridge.session(session_id).await;
    timeout(timeout_duration, async {
        loop {
            if session.cli.lock().await.is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("timeout waiting for cli websocket")?;
    Ok(())
}
