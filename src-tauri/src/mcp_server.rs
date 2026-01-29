use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tauri::AppHandle;
use url::form_urlencoded;

use crate::db;
use crate::worktree;
use crate::{
    create_agent_session_internal, delete_task_internal, get_task_history_internal,
    project_path_allowed, resolve_repo_root, send_chat_message_internal, soft_stop_task_internal,
    start_task_internal, stop_task_internal, AppState, CreateAgentPayload,
};

const DEFAULT_PROTOCOL_VERSION: &str = "2025-03-26";

#[derive(Clone)]
pub(crate) struct McpConfig {
    pub(crate) port: u16,
    pub(crate) token: String,
}

#[derive(Clone)]
struct McpServerState {
    app: AppHandle,
    state: AppState,
    config: McpConfig,
    sessions: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
}

pub(crate) async fn start_mcp_server(
    app: AppHandle,
    state: AppState,
    config: McpConfig,
) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let server_state = McpServerState {
        app,
        state,
        config,
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    let make_svc = make_service_fn(move |_conn| {
        let server_state = server_state.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(req, server_state.clone())
            }))
        }
    });

    println!(
        "[Harness] MCP server listening on http://127.0.0.1:{}/mcp",
        addr.port()
    );
    Server::bind(&addr).serve(make_svc).await?;
    Ok(())
}

async fn handle_request(
    req: Request<Body>,
    server_state: McpServerState,
) -> Result<Response<Body>, Infallible> {
    if let Err(resp) = verify_origin(&req) {
        return Ok(resp);
    }
    let query_token = query_param(req.uri().query(), "token");
    if !token_matches(&req, query_token.as_deref(), &server_state.config.token) {
        return Ok(response_json(
            StatusCode::UNAUTHORIZED,
            json!({"error": "unauthorized"}),
        ));
    }

    match (req.method(), req.uri().path()) {
        (&Method::GET, "/sse") => Ok(handle_sse(req, server_state).await),
        (&Method::POST, "/mcp") => Ok(handle_mcp_post(req, server_state).await),
        (&Method::GET, "/mcp") => {
            if accepts_sse(&req) {
                Ok(handle_streamable_sse(req, server_state).await)
            } else {
                Ok(response_json(
                    StatusCode::METHOD_NOT_ALLOWED,
                    json!({"error": "method_not_allowed"}),
                ))
            }
        }
        _ => Ok(response_json(StatusCode::NOT_FOUND, json!({"error": "not_found"}))),
    }
}

fn verify_origin(req: &Request<Body>) -> Result<(), Response<Body>> {
    let Some(origin) = req.headers().get("origin") else {
        return Ok(());
    };
    let Ok(origin) = origin.to_str() else {
        return Err(response_json(
            StatusCode::FORBIDDEN,
            json!({"error": "invalid_origin"}),
        ));
    };
    if origin_allowed(origin) {
        Ok(())
    } else {
        Err(response_json(
            StatusCode::FORBIDDEN,
            json!({"error": "invalid_origin"}),
        ))
    }
}

fn origin_allowed(origin: &str) -> bool {
    matches!(
        origin,
        "http://localhost"
            | "http://127.0.0.1"
            | "https://localhost"
            | "https://127.0.0.1"
            | "tauri://localhost"
    )
}

fn token_matches(req: &Request<Body>, query_token: Option<&str>, expected: &str) -> bool {
    if let Some(value) = query_token {
        if value == expected {
            return true;
        }
    }
    let Some(header) = req.headers().get("authorization") else {
        return false;
    };
    let Ok(header) = header.to_str() else {
        return false;
    };
    extract_bearer_token(header)
        .as_deref()
        .map(|token| token == expected)
        .unwrap_or(false)
}

fn extract_bearer_token(header: &str) -> Option<String> {
    let header = header.trim();
    let prefix = "Bearer ";
    if header.len() <= prefix.len() {
        return None;
    }
    if header.starts_with(prefix) {
        Some(header[prefix.len()..].trim().to_string())
    } else {
        None
    }
}

fn accepts_sse(req: &Request<Body>) -> bool {
    req.headers()
        .get("accept")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.contains("text/event-stream"))
        .unwrap_or(false)
}

fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query.and_then(|raw| {
        form_urlencoded::parse(raw.as_bytes())
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_string())
    })
}

async fn handle_sse(req: Request<Body>, server_state: McpServerState) -> Response<Body> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::unbounded_channel();

    {
        let mut sessions = server_state.sessions.lock().await;
        sessions.insert(session_id.clone(), tx.clone());
    }

    let endpoint_token = query_param(req.uri().query(), "token");
    let mut endpoint_url = format!(
        "http://127.0.0.1:{}/mcp?sessionId={}",
        server_state.config.port, session_id
    );
    if let Some(token) = endpoint_token.as_deref() {
        endpoint_url.push_str("&token=");
        endpoint_url.push_str(token);
    }

    let _ = tx.send(sse_event("endpoint", &endpoint_url));

    spawn_keepalive(tx.clone());

    let stream = UnboundedReceiverStream::new(rx).map(Ok::<_, Infallible>);
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .body(Body::wrap_stream(stream))
        .unwrap_or_else(|_| response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": "sse_failed"})))
}

async fn handle_streamable_sse(
    _req: Request<Body>,
    _server_state: McpServerState,
) -> Response<Body> {
    let (tx, rx) = mpsc::unbounded_channel();
    spawn_keepalive(tx.clone());

    let stream = UnboundedReceiverStream::new(rx).map(Ok::<_, Infallible>);
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .header("mcp-server", "phantom")
        .body(Body::wrap_stream(stream))
        .unwrap_or_else(|_| response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": "sse_failed"})))
}

async fn handle_mcp_post(req: Request<Body>, server_state: McpServerState) -> Response<Body> {
    let session_id = query_param(req.uri().query(), "sessionId");
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return response_json(
                StatusCode::BAD_REQUEST,
                json!({"error": "invalid_body"}),
            )
        }
    };

    if body_bytes.len() > 1_000_000 {
        return response_json(
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({"error": "payload_too_large"}),
        );
    }

    let payload: Value = match serde_json::from_slice(&body_bytes) {
        Ok(value) => value,
        Err(_) => {
            return response_json(
                StatusCode::BAD_REQUEST,
                json!({"error": "invalid_json"}),
            )
        }
    };

    let responses = match handle_rpc_payload(payload, server_state.clone()).await {
        Ok(responses) => responses,
        Err(err) => {
            return response_json(StatusCode::BAD_REQUEST, err);
        }
    };

    if let Some(session_id) = session_id {
        let mut sessions = server_state.sessions.lock().await;
        let Some(sender) = sessions.get(&session_id) else {
            return response_json(
                StatusCode::NOT_FOUND,
                json!({"error": "session_not_found"}),
            );
        };
        for response in &responses {
            if sender
                .send(sse_event("message", &response.to_string()))
                .is_err()
            {
                sessions.remove(&session_id);
                break;
            }
        }
        return response_json(StatusCode::OK, json!({"ok": true}));
    }

    response_json(StatusCode::OK, json_response_for(responses))
}

async fn handle_rpc_payload(
    payload: Value,
    server_state: McpServerState,
) -> Result<Vec<Value>, Value> {
    if payload.is_array() {
        let mut responses = Vec::new();
        for value in payload
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
        {
            if let Some(response) = handle_rpc_request(value, server_state.clone()).await? {
                responses.push(response);
            }
        }
        Ok(responses)
    } else {
        let mut responses = Vec::new();
        if let Some(response) = handle_rpc_request(payload, server_state).await? {
            responses.push(response);
        }
        Ok(responses)
    }
}

async fn handle_rpc_request(
    payload: Value,
    server_state: McpServerState,
) -> Result<Option<Value>, Value> {
    let Some(method) = payload.get("method").and_then(|value| value.as_str()) else {
        return Err(json!({
            "jsonrpc": "2.0",
            "id": payload.get("id").cloned().unwrap_or(Value::Null),
            "error": {
                "code": -32600,
                "message": "Invalid Request"
            }
        }));
    };

    let id = payload.get("id").cloned();

    match method {
        "initialize" => Ok(id.map(|id| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": DEFAULT_PROTOCOL_VERSION,
                    "serverInfo": {
                        "name": "phantom",
                        "version": "0.1.0"
                    },
                    "capabilities": {
                        "tools": {
                            "listChanged": false
                        }
                    }
                }
            })
        })),
        "tools/list" => Ok(id.map(|id| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": tool_definitions()
                }
            })
        })),
        "tools/call" => {
            let params = payload.get("params").cloned().unwrap_or(Value::Null);
            let result = handle_tool_call(params, server_state).await;
            Ok(id.map(|id| {
                match result {
                    Ok(value) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": value
                    }),
                    Err(err) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32602,
                            "message": err
                        }
                    })
                }
            }))
        }
        "ping" => Ok(id.map(|id| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            })
        })),
        "notifications/initialized" => Ok(None),
        _ => Ok(id.map(|id| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": "Method not found"
                }
            })
        })),
    }
}

fn json_response_for(responses: Vec<Value>) -> Value {
    if responses.len() == 1 {
        responses.into_iter().next().unwrap_or(Value::Null)
    } else {
        Value::Array(responses)
    }
}

fn response_json(status: StatusCode, body: Value) -> Response<Body> {
    let payload = body.to_string();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(payload))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn sse_event(event: &str, data: &str) -> String {
    let data = data.replace('\n', "\\n");
    format!("event: {event}\ndata: {data}\n\n")
}

fn spawn_keepalive(sender: mpsc::UnboundedSender<String>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            if sender.send(":keep-alive\n\n".to_string()).is_err() {
                break;
            }
        }
    });
}

#[derive(Debug, Deserialize)]
struct CreateTaskArgs {
    prompt: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    plan_mode: Option<bool>,
    #[serde(default)]
    thinking: Option<bool>,
    #[serde(default)]
    use_worktree: Option<bool>,
    #[serde(default)]
    base_branch: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    exec_model: Option<String>,
    #[serde(default)]
    agent_mode: Option<String>,
    #[serde(default)]
    codex_mode: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    start: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateTaskArgs {
    task_id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    status_state: Option<String>,
    #[serde(default)]
    title_summary: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    cost: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct TaskIdArgs {
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct SendMessageArgs {
    task_id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct CreateWorkspaceArgs {
    project_path: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    sync: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct WorkspacePathArgs {
    path: String,
}

async fn handle_tool_call(params: Value, server_state: McpServerState) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "Missing tool name".to_string())?;
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

    let result = match name {
        // Superset-compatible tools
        "create_task" => tool_create_task(arguments, server_state).await,
        "update_task" => tool_update_task(arguments, server_state).await,
        "list_tasks" => tool_list_tasks(server_state).await,
        "get_task" => tool_get_task(arguments, server_state).await,
        "delete_task" => tool_delete_task(arguments, server_state).await,
        "list_task_statuses" => tool_list_task_statuses(server_state).await,
        "create_workspace" => tool_create_workspace(arguments, server_state).await,
        "switch_workspace" => tool_switch_workspace(arguments, server_state).await,
        "delete_workspace" => tool_delete_workspace(arguments, server_state).await,
        "list_workspaces" => tool_list_workspaces(server_state).await,
        "navigate_to_workspace" => tool_navigate_workspace(arguments, server_state).await,
        // Phantom-specific tools
        "phantom_start_task" => tool_start_task(arguments, server_state).await,
        "phantom_stop_task" => tool_stop_task(arguments, server_state).await,
        "phantom_soft_stop_task" => tool_soft_stop_task(arguments, server_state).await,
        "phantom_send_chat_message" => tool_send_chat_message(arguments, server_state).await,
        "phantom_get_task_history" => tool_get_task_history(arguments, server_state).await,
        "phantom_list_agents" => tool_list_agents(server_state).await,
        _ => Err(format!("Unknown tool: {name}")),
    };

    match result {
        Ok(value) => Ok(tool_result(value)),
        Err(err) => Ok(tool_error(err)),
    }
}

fn tool_result(value: Value) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    json!({
        "content": [
            {"type": "text", "text": text}
        ]
    })
}

fn tool_error(message: String) -> Value {
    json!({
        "isError": true,
        "content": [
            {"type": "text", "text": message}
        ]
    })
}

async fn tool_create_task(
    arguments: Value,
    server_state: McpServerState,
) -> Result<Value, String> {
    let args: CreateTaskArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid create_task args: {e}"))?;
    if args.prompt.trim().is_empty() {
        return Err("Prompt is required".to_string());
    }

    let settings = server_state.state.settings.lock().await.clone();
    let agent_id = args
        .agent_id
        .clone()
        .or_else(|| settings.task_last_agent.clone())
        .unwrap_or_else(|| "codex".to_string());

    let plan_mode = args
        .plan_mode
        .unwrap_or_else(|| settings.task_plan_mode.unwrap_or(false));
    let thinking = args
        .thinking
        .unwrap_or_else(|| settings.task_thinking.unwrap_or(true));
    let use_worktree = args
        .use_worktree
        .unwrap_or_else(|| settings.task_use_worktree.unwrap_or(true));

    let allowlist = settings
        .task_project_allowlist
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    if !allowlist.is_empty() {
        let Some(project_path) = args
            .project_path
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        else {
            return Err("Project path required when allowlist is set".to_string());
        };
        if !project_path_allowed(&allowlist, project_path) {
            return Err("Project path not allowed".to_string());
        }
    }

    let exec_model = args
        .exec_model
        .clone()
        .or_else(|| args.model.clone())
        .unwrap_or_else(|| "default".to_string());

    let payload = CreateAgentPayload {
        agent_id: agent_id.clone(),
        prompt: args.prompt,
        project_path: args.project_path.clone(),
        base_branch: args.base_branch.clone(),
        plan_mode,
        thinking,
        use_worktree,
        permission_mode: args
            .permission_mode
            .clone()
            .unwrap_or_else(|| "default".to_string()),
        exec_model,
        reasoning_effort: args.reasoning_effort.clone(),
        agent_mode: args.agent_mode.clone(),
        codex_mode: args.codex_mode.clone(),
        multi_create: false,
        attachments: Vec::new(),
    };

    let created = create_agent_session_internal(
        server_state.app.clone(),
        payload,
        &server_state.state,
        true,
    )
    .await?;

    let should_start = args.start.unwrap_or(true);
    if should_start {
        start_task_internal(
            created.task_id.clone(),
            &server_state.state,
            server_state.app.clone(),
            None,
        )
        .await?;
    }

    Ok(json!({
        "task_id": created.task_id,
        "session_id": created.session_id,
        "worktree_path": created.worktree_path
    }))
}

async fn tool_update_task(
    arguments: Value,
    server_state: McpServerState,
) -> Result<Value, String> {
    let args: UpdateTaskArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid update_task args: {e}"))?;
    let conn = server_state.state.db.lock().map_err(|e| e.to_string())?;
    let tasks = db::list_tasks(&conn).map_err(|e| e.to_string())?;
    let task = tasks
        .iter()
        .find(|task| task.id == args.task_id)
        .cloned()
        .ok_or_else(|| "Task not found".to_string())?;

    if args.status.is_some() || args.status_state.is_some() {
        let status = args.status.as_deref().unwrap_or(&task.status);
        let status_state = args
            .status_state
            .as_deref()
            .unwrap_or(&task.status_state)
            .to_string();
        let _ = db::update_task_status(&conn, &task.id, status, &status_state);
    }
    if let Some(title) = args.title_summary.as_ref() {
        let _ = db::update_task_title_summary(&conn, &task.id, title);
    }
    if let Some(branch) = args.branch.as_ref() {
        let _ = db::update_task_branch(&conn, &task.id, branch);
    }
    if let Some(cost) = args.cost {
        let _ = db::update_task_cost(&conn, &task.id, cost);
    }

    Ok(json!({"updated": true, "task_id": task.id}))
}

async fn tool_list_tasks(server_state: McpServerState) -> Result<Value, String> {
    let conn = server_state.state.db.lock().map_err(|e| e.to_string())?;
    let tasks = db::list_tasks(&conn).map_err(|e| e.to_string())?;
    let out: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            json!({
                "id": task.id,
                "agent_id": task.agent_id,
                "model": task.model,
                "prompt": task.prompt,
                "project_path": task.project_path,
                "worktree_path": task.worktree_path,
                "branch": task.branch,
                "status": task.status,
                "status_state": task.status_state,
                "cost": task.cost,
                "created_at": task.created_at,
                "updated_at": task.updated_at,
                "title_summary": task.title_summary,
                "agent_session_id": task.agent_session_id,
            })
        })
        .collect();
    Ok(json!({"tasks": out}))
}

async fn tool_get_task(arguments: Value, server_state: McpServerState) -> Result<Value, String> {
    let args: TaskIdArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid get_task args: {e}"))?;
    let conn = server_state.state.db.lock().map_err(|e| e.to_string())?;
    let tasks = db::list_tasks(&conn).map_err(|e| e.to_string())?;
    let task = tasks
        .into_iter()
        .find(|task| task.id == args.task_id)
        .ok_or_else(|| "Task not found".to_string())?;
    Ok(json!({
        "id": task.id,
        "agent_id": task.agent_id,
        "model": task.model,
        "prompt": task.prompt,
        "project_path": task.project_path,
        "worktree_path": task.worktree_path,
        "branch": task.branch,
        "status": task.status,
        "status_state": task.status_state,
        "cost": task.cost,
        "created_at": task.created_at,
        "updated_at": task.updated_at,
        "title_summary": task.title_summary,
        "agent_session_id": task.agent_session_id,
        "total_tokens": task.total_tokens,
        "context_window": task.context_window,
    }))
}

async fn tool_delete_task(arguments: Value, server_state: McpServerState) -> Result<Value, String> {
    let args: TaskIdArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid delete_task args: {e}"))?;
    delete_task_internal(
        args.task_id.clone(),
        &server_state.state,
        server_state.app.clone(),
    )
    .await?;
    Ok(json!({"deleted": true, "task_id": args.task_id}))
}

async fn tool_list_task_statuses(server_state: McpServerState) -> Result<Value, String> {
    let conn = server_state.state.db.lock().map_err(|e| e.to_string())?;
    let tasks = db::list_tasks(&conn).map_err(|e| e.to_string())?;
    let out: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            json!({
                "id": task.id,
                "status": task.status,
                "status_state": task.status_state,
            })
        })
        .collect();
    Ok(json!({"tasks": out}))
}

async fn tool_create_workspace(
    arguments: Value,
    _server_state: McpServerState,
) -> Result<Value, String> {
    let args: CreateWorkspaceArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid create_workspace args: {e}"))?;
    let source_path = std::path::PathBuf::from(args.project_path.clone());
    let repo_root = resolve_repo_root(&source_path).await;
    let sync_source = repo_root.as_deref().unwrap_or(&source_path);
    let repo_slug = worktree::repo_slug(sync_source);
    let root = worktree::workspace_root_dir()?;
    let repo_dir = root.join(&repo_slug);
    std::fs::create_dir_all(&repo_dir)
        .map_err(|e| format!("Failed to create workspace repo dir: {e}"))?;

    let workspace_path = if let Some(name) = args.name.as_ref() {
        let workspace_name = unique_workspace_name(&repo_dir, name);
        repo_dir.join(workspace_name)
    } else {
        worktree::build_workspace_path(&repo_slug)?
    };
    if !workspace_path.exists() {
        std::fs::create_dir_all(&workspace_path)
            .map_err(|e| format!("Failed to create workspace: {e}"))?;
    }

    if args.sync.unwrap_or(true) {
        worktree::sync_workspace_from_source(sync_source, &workspace_path).await?;
    }

    Ok(json!({
        "path": workspace_path.to_string_lossy().to_string(),
        "repo": repo_dir.to_string_lossy().to_string(),
    }))
}

async fn tool_switch_workspace(
    arguments: Value,
    server_state: McpServerState,
) -> Result<Value, String> {
    let args: WorkspacePathArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid switch_workspace args: {e}"))?;
    let mut settings = server_state.state.settings.lock().await;
    settings.task_project_path = Some(args.path.clone());
    if let Err(e) = crate::persist_settings(&settings) {
        eprintln!("[Harness] MCP persist settings failed: {e}");
    }
    Ok(json!({"active": args.path}))
}

async fn tool_delete_workspace(
    arguments: Value,
    _server_state: McpServerState,
) -> Result<Value, String> {
    let args: WorkspacePathArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid delete_workspace args: {e}"))?;
    let path = std::path::PathBuf::from(args.path.clone());
    worktree::remove_workspace_dir(&path)?;
    Ok(json!({"deleted": true, "path": args.path}))
}

async fn tool_list_workspaces(_server_state: McpServerState) -> Result<Value, String> {
    let root = worktree::workspace_root_dir()?;
    let mut workspaces = Vec::new();
    if root.exists() {
        for repo_entry in std::fs::read_dir(&root).map_err(|e| format!("read_dir: {e}"))? {
            let repo_entry = repo_entry.map_err(|e| format!("repo entry: {e}"))?;
            let repo_path = repo_entry.path();
            if !repo_path.is_dir() {
                continue;
            }
            for ws_entry in std::fs::read_dir(&repo_path)
                .map_err(|e| format!("workspace dir: {e}"))?
            {
                let ws_entry = ws_entry.map_err(|e| format!("workspace entry: {e}"))?;
                let ws_path = ws_entry.path();
                if !ws_path.is_dir() {
                    continue;
                }
                workspaces.push(json!({
                    "id": ws_path.to_string_lossy().to_string(),
                    "path": ws_path.to_string_lossy().to_string(),
                    "repo": repo_path.to_string_lossy().to_string(),
                }));
            }
        }
    }
    Ok(json!({"workspaces": workspaces}))
}

async fn tool_navigate_workspace(
    arguments: Value,
    _server_state: McpServerState,
) -> Result<Value, String> {
    let args: WorkspacePathArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid navigate_to_workspace args: {e}"))?;
    Ok(json!({"path": args.path}))
}

async fn tool_start_task(arguments: Value, server_state: McpServerState) -> Result<Value, String> {
    let args: TaskIdArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid phantom_start_task args: {e}"))?;
    start_task_internal(
        args.task_id.clone(),
        &server_state.state,
        server_state.app.clone(),
        None,
    )
    .await?;
    Ok(json!({"started": true, "task_id": args.task_id}))
}

async fn tool_stop_task(arguments: Value, server_state: McpServerState) -> Result<Value, String> {
    let args: TaskIdArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid phantom_stop_task args: {e}"))?;
    stop_task_internal(
        args.task_id.clone(),
        &server_state.state,
        server_state.app.clone(),
    )
    .await?;
    Ok(json!({"stopped": true, "task_id": args.task_id}))
}

async fn tool_soft_stop_task(
    arguments: Value,
    server_state: McpServerState,
) -> Result<Value, String> {
    let args: TaskIdArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid phantom_soft_stop_task args: {e}"))?;
    soft_stop_task_internal(
        args.task_id.clone(),
        &server_state.state,
        server_state.app.clone(),
    )
    .await?;
    Ok(json!({"soft_stopped": true, "task_id": args.task_id}))
}

async fn tool_send_chat_message(
    arguments: Value,
    server_state: McpServerState,
) -> Result<Value, String> {
    let args: SendMessageArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid phantom_send_chat_message args: {e}"))?;
    send_chat_message_internal(
        args.task_id.clone(),
        args.message.clone(),
        &server_state.state,
        server_state.app.clone(),
        crate::MessageOrigin::Ui,
    )
    .await?;
    Ok(json!({"sent": true, "task_id": args.task_id}))
}

async fn tool_get_task_history(
    arguments: Value,
    server_state: McpServerState,
) -> Result<Value, String> {
    let args: TaskIdArgs = serde_json::from_value(arguments)
        .map_err(|e| format!("Invalid phantom_get_task_history args: {e}"))?;
    let history = get_task_history_internal(&args.task_id, &server_state.state).await?;
    Ok(history)
}

async fn tool_list_agents(server_state: McpServerState) -> Result<Value, String> {
    let agents = server_state
        .state
        .config
        .agents
        .iter()
        .map(|agent| {
            json!({
                "id": agent.id.clone(),
                "display_name": agent.display_name.clone(),
                "supports_plan": agent.supports_plan.unwrap_or(false),
                "models": agent.models.clone(),
                "model_source": agent.model_source.clone(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"agents": agents}))
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool_schema(
            "create_task",
            "Create a new Phantom task (optionally starts immediately).",
            json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string"},
                    "agent_id": {"type": "string"},
                    "project_path": {"type": "string"},
                    "model": {"type": "string"},
                    "plan_mode": {"type": "boolean"},
                    "thinking": {"type": "boolean"},
                    "use_worktree": {"type": "boolean"},
                    "base_branch": {"type": "string"},
                    "permission_mode": {"type": "string"},
                    "exec_model": {"type": "string"},
                    "agent_mode": {"type": "string"},
                    "codex_mode": {"type": "string"},
                    "reasoning_effort": {"type": "string"},
                    "start": {"type": "boolean"}
                },
                "required": ["prompt"]
            }),
        ),
        tool_schema(
            "update_task",
            "Update task metadata (status, title, branch, cost).",
            json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "status": {"type": "string"},
                    "status_state": {"type": "string"},
                    "title_summary": {"type": "string"},
                    "branch": {"type": "string"},
                    "cost": {"type": "number"}
                },
                "required": ["task_id"]
            }),
        ),
        tool_schema(
            "list_tasks",
            "List tasks stored in Phantom.",
            json!({"type": "object", "properties": {}}),
        ),
        tool_schema(
            "get_task",
            "Fetch a single task by id.",
            json!({
                "type": "object",
                "properties": {"task_id": {"type": "string"}},
                "required": ["task_id"]
            }),
        ),
        tool_schema(
            "delete_task",
            "Delete a task by id.",
            json!({
                "type": "object",
                "properties": {"task_id": {"type": "string"}},
                "required": ["task_id"]
            }),
        ),
        tool_schema(
            "list_task_statuses",
            "List statuses for all tasks.",
            json!({"type": "object", "properties": {}}),
        ),
        tool_schema(
            "create_workspace",
            "Create a local workspace from a project path.",
            json!({
                "type": "object",
                "properties": {
                    "project_path": {"type": "string"},
                    "name": {"type": "string"},
                    "sync": {"type": "boolean"}
                },
                "required": ["project_path"]
            }),
        ),
        tool_schema(
            "switch_workspace",
            "Set the active project path in Phantom settings.",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        ),
        tool_schema(
            "delete_workspace",
            "Delete a workspace directory.",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        ),
        tool_schema(
            "list_workspaces",
            "List local workspaces.",
            json!({"type": "object", "properties": {}}),
        ),
        tool_schema(
            "navigate_to_workspace",
            "Return the workspace path for navigation.",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        ),
        tool_schema(
            "phantom_start_task",
            "Start an existing task.",
            json!({
                "type": "object",
                "properties": {"task_id": {"type": "string"}},
                "required": ["task_id"]
            }),
        ),
        tool_schema(
            "phantom_stop_task",
            "Stop an existing task.",
            json!({
                "type": "object",
                "properties": {"task_id": {"type": "string"}},
                "required": ["task_id"]
            }),
        ),
        tool_schema(
            "phantom_soft_stop_task",
            "Cancel current generation without killing the task session.",
            json!({
                "type": "object",
                "properties": {"task_id": {"type": "string"}},
                "required": ["task_id"]
            }),
        ),
        tool_schema(
            "phantom_send_chat_message",
            "Send a message to a running task.",
            json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "message": {"type": "string"}
                },
                "required": ["task_id", "message"]
            }),
        ),
        tool_schema(
            "phantom_get_task_history",
            "Fetch task history and messages.",
            json!({
                "type": "object",
                "properties": {"task_id": {"type": "string"}},
                "required": ["task_id"]
            }),
        ),
        tool_schema(
            "phantom_list_agents",
            "List configured Phantom agents.",
            json!({"type": "object", "properties": {}}),
        ),
    ]
}

fn tool_schema(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn unique_workspace_name(repo_dir: &std::path::Path, base: &str) -> String {
    let base = worktree::sanitize_workspace_slug(base);
    if base.is_empty() {
        return "task".to_string();
    }
    let mut candidate = base.clone();
    let mut counter = 2;
    loop {
        let path = repo_dir.join(&candidate);
        if !path.exists() {
            return candidate;
        }
        candidate = format!("{}-v{}", base, counter);
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_bearer_token, origin_allowed};

    #[test]
    fn test_origin_allowed_local() {
        assert!(origin_allowed("http://localhost"));
        assert!(origin_allowed("http://127.0.0.1"));
        assert!(origin_allowed("tauri://localhost"));
    }

    #[test]
    fn test_origin_denied_external() {
        assert!(!origin_allowed("https://example.com"));
    }

    #[test]
    fn test_extract_bearer_token() {
        let token = extract_bearer_token("Bearer abc123").unwrap();
        assert_eq!(token, "abc123");
        assert!(extract_bearer_token("Basic abc").is_none());
    }
}
