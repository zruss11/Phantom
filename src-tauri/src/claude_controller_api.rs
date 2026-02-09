use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, HeaderMap, Method, Request, Response, Server, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::form_urlencoded;

use crate::claude_controller::ClaudeTeamsController;

// Note: 43778 is used by the MCP server in Phantom. Use a different default to avoid conflicts.
const DEFAULT_PORT: u16 = 43779;

#[derive(Clone)]
pub(crate) struct ClaudeCtrlConfig {
    pub(crate) port: u16,
    pub(crate) token: String,
}

#[derive(Clone)]
struct ApiState {
    config: ClaudeCtrlConfig,
    controller: Arc<Mutex<Option<ClaudeTeamsController>>>,
    started_at: i64,
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

fn apply_cors_headers(resp: &mut Response<Body>, origin: Option<&str>) {
    let Some(origin) = origin else {
        return;
    };
    let headers = resp.headers_mut();
    headers.insert("access-control-allow-origin", origin.parse().unwrap());
    headers.insert(
        "access-control-allow-methods",
        "GET, POST, OPTIONS".parse().unwrap(),
    );
    headers.insert(
        "access-control-allow-headers",
        "authorization, content-type".parse().unwrap(),
    );
    headers.insert("access-control-max-age", "600".parse().unwrap());
}

fn ok_with_cors(
    mut resp: Response<Body>,
    origin: Option<&str>,
) -> Result<Response<Body>, Infallible> {
    apply_cors_headers(&mut resp, origin);
    Ok(resp)
}

fn response_json(status: StatusCode, body: Value) -> Response<Body> {
    let bytes = body.to_string();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::from("{\"error\":\"response_build_failed\"}")))
}

fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query.and_then(|raw| {
        form_urlencoded::parse(raw.as_bytes())
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_string())
    })
}

fn extract_bearer_token(header: &str) -> Option<String> {
    let header = header.trim();
    let prefix = "Bearer ";
    if header.starts_with(prefix) && header.len() > prefix.len() {
        Some(header[prefix.len()..].trim().to_string())
    } else {
        None
    }
}

fn token_matches(headers: &HeaderMap, query_token: Option<&str>, expected: &str) -> bool {
    let allow_query_token = std::env::var("PHANTOM_CLAUDE_CTRL_ALLOW_QUERY_TOKEN")
        .ok()
        .as_deref()
        == Some("1");
    if allow_query_token {
        if let Some(v) = query_token {
            if v == expected {
                return true;
            }
        }
    }
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(extract_bearer_token)
        .map(|t| t == expected)
        .unwrap_or(false)
}

fn verify_origin(headers: &HeaderMap) -> Result<Option<String>, Response<Body>> {
    let Some(origin) = headers.get("origin") else {
        return Ok(None);
    };
    let Ok(origin) = origin.to_str() else {
        return Err(response_json(
            StatusCode::FORBIDDEN,
            json!({"error":"invalid_origin"}),
        ));
    };
    if origin_allowed(origin) {
        Ok(Some(origin.to_string()))
    } else {
        Err(response_json(
            StatusCode::FORBIDDEN,
            json!({"error":"invalid_origin"}),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct InitSessionBody {
    #[serde(default)]
    #[serde(rename = "teamName")]
    team_name: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    #[serde(rename = "claudeBinary")]
    claude_binary: Option<String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct SpawnAgentBody {
    name: String,
    #[serde(default)]
    #[serde(rename = "type")]
    agent_type: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    #[serde(rename = "permissionMode")]
    permission_mode: Option<String>,
    #[serde(default)]
    permissions: Option<Vec<String>>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct SendMessageBody {
    message: String,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BroadcastBody {
    message: String,
    #[serde(default)]
    summary: Option<String>,
}

fn validate_name(value: &str) -> Result<(), Response<Body>> {
    let ok = !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(response_json(
            StatusCode::BAD_REQUEST,
            json!({"error":"name must be 1-64 chars of [A-Za-z0-9_-]"}),
        ))
    }
}

async fn read_body_json<T: DeserializeOwned>(body: Body) -> Result<T, Response<Body>> {
    let bytes = hyper::body::to_bytes(body)
        .await
        .map_err(|_| response_json(StatusCode::BAD_REQUEST, json!({"error":"invalid_body"})))?;
    if bytes.len() > 1_000_000 {
        return Err(response_json(
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({"error":"payload_too_large"}),
        ));
    }
    serde_json::from_slice::<T>(&bytes)
        .map_err(|_| response_json(StatusCode::BAD_REQUEST, json!({"error":"invalid_json"})))
}

async fn handle(req: Request<Body>, state: ApiState) -> Result<Response<Body>, Infallible> {
    let (parts, body) = req.into_parts();
    let origin = match verify_origin(&parts.headers) {
        Ok(o) => o,
        Err(mut resp) => {
            apply_cors_headers(&mut resp, None);
            return Ok(resp);
        }
    };

    if parts.method == Method::OPTIONS {
        let mut resp = Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap_or_else(|_| Response::new(Body::empty()));
        apply_cors_headers(&mut resp, origin.as_deref());
        return Ok(resp);
    }
    let qt = query_param(parts.uri.query(), "token");
    if !token_matches(&parts.headers, qt.as_deref(), &state.config.token) {
        let mut resp = response_json(StatusCode::UNAUTHORIZED, json!({"error":"unauthorized"}));
        apply_cors_headers(&mut resp, origin.as_deref());
        return Ok(resp);
    }

    let method = parts.method;
    let path = parts.uri.path().to_string();

    let mut resp = match (method, path, body) {
        (Method::GET, p, _) if p == "/health" => {
            let ctrl = state.controller.lock().await;
            let has_session = ctrl.is_some();
            response_json(
                StatusCode::OK,
                json!({"status":"ok","uptime": (chrono::Utc::now().timestamp_millis() - state.started_at), "session": has_session, "port": state.config.port}),
            )
        }
        (Method::GET, p, _) if p == "/session" => {
            let ctrl = state.controller.lock().await;
            if let Some(c) = ctrl.as_ref() {
                response_json(
                    StatusCode::OK,
                    json!({"initialized": true, "teamName": c.team_name()}),
                )
            } else {
                response_json(
                    StatusCode::OK,
                    json!({"initialized": false, "teamName": ""}),
                )
            }
        }
        (Method::POST, p, body) if p == "/session/init" => {
            let body: InitSessionBody = match read_body_json(body).await {
                Ok(b) => b,
                Err(resp) => return ok_with_cors(resp, origin.as_deref()),
            };
            if let Some(ref name) = body.team_name {
                if let Err(resp) = validate_name(name) {
                    return ok_with_cors(resp, origin.as_deref());
                }
            }
            let team_name = body
                .team_name
                .unwrap_or_else(|| "phantom-harness".to_string());
            let cwd = body.cwd.unwrap_or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".to_string())
            });
            let claude_binary = body.claude_binary.unwrap_or_else(|| "claude".to_string());
            let env = body.env.unwrap_or_default().into_iter().collect::<Vec<_>>();

            match ClaudeTeamsController::init(team_name.clone(), cwd, claude_binary, env).await {
                Ok(controller) => {
                    let mut slot = state.controller.lock().await;
                    if let Some(prev) = slot.take() {
                        let _ = prev.shutdown_all().await;
                    }
                    *slot = Some(controller);
                    response_json(
                        StatusCode::CREATED,
                        json!({"initialized": true, "teamName": team_name}),
                    )
                }
                Err(e) => response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
            }
        }
        (Method::POST, p, _) if p == "/session/shutdown" => {
            let mut slot = state.controller.lock().await;
            if let Some(ctrl) = slot.take() {
                let _ = ctrl.shutdown_all().await;
            }
            response_json(StatusCode::OK, json!({"ok": true}))
        }
        (Method::GET, p, _) if p == "/agents" => {
            let slot = state.controller.lock().await;
            let Some(ctrl) = slot.as_ref() else {
                return ok_with_cors(
                    response_json(
                        StatusCode::BAD_REQUEST,
                        json!({"error":"No active session. Call POST /session/init first."}),
                    ),
                    origin.as_deref(),
                );
            };
            match ctrl.list_agents().await {
                Ok(list) => {
                    let agents = list
                        .into_iter()
                        .map(|(name, typ, model, running)| {
                            json!({"name": name, "type": typ, "model": model, "running": running})
                        })
                        .collect::<Vec<_>>();
                    response_json(StatusCode::OK, json!(agents))
                }
                Err(e) => response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
            }
        }
        (Method::POST, p, body) if p == "/agents" => {
            let body: SpawnAgentBody = match read_body_json(body).await {
                Ok(b) => b,
                Err(resp) => return ok_with_cors(resp, origin.as_deref()),
            };
            if let Err(resp) = validate_name(&body.name) {
                return ok_with_cors(resp, origin.as_deref());
            }
            let slot = state.controller.lock().await;
            let Some(ctrl) = slot.as_ref() else {
                return ok_with_cors(
                    response_json(
                        StatusCode::BAD_REQUEST,
                        json!({"error":"No active session. Call POST /session/init first."}),
                    ),
                    origin.as_deref(),
                );
            };
            let env = body.env.unwrap_or_default().into_iter().collect::<Vec<_>>();
            match ctrl
                .spawn_agent(
                    body.name.clone(),
                    body.agent_type.clone(),
                    body.model.clone(),
                    body.cwd.clone(),
                    body.permission_mode.clone(),
                    body.permissions.unwrap_or_default(),
                    env,
                )
                .await
            {
                Ok(pid) => response_json(
                    StatusCode::CREATED,
                    json!({"name": body.name, "type": body.agent_type.unwrap_or_else(|| "general-purpose".to_string()), "model": body.model, "pid": pid, "running": true}),
                ),
                Err(e) => response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
            }
        }
        (Method::GET, p, _) if p.starts_with("/agents/") => {
            let name = p.trim_start_matches("/agents/");
            if name.is_empty() || name.contains('/') {
                return ok_with_cors(
                    response_json(StatusCode::NOT_FOUND, json!({"error":"not_found"})),
                    origin.as_deref(),
                );
            }
            if let Err(resp) = validate_name(name) {
                return ok_with_cors(resp, origin.as_deref());
            }
            let slot = state.controller.lock().await;
            let Some(ctrl) = slot.as_ref() else {
                return ok_with_cors(
                    response_json(
                        StatusCode::BAD_REQUEST,
                        json!({"error":"No active session. Call POST /session/init first."}),
                    ),
                    origin.as_deref(),
                );
            };
            let agents = match ctrl.list_agents().await {
                Ok(list) => list,
                Err(e) => {
                    return ok_with_cors(
                        response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
                        origin.as_deref(),
                    );
                }
            };
            if let Some((agent_name, typ, model, running)) =
                agents.into_iter().find(|(n, _t, _m, _r)| n == name)
            {
                response_json(
                    StatusCode::OK,
                    json!({"name": agent_name, "type": typ, "model": model, "running": running}),
                )
            } else {
                let running = ctrl.is_agent_running(name).await;
                response_json(StatusCode::OK, json!({"name": name, "running": running}))
            }
        }
        (Method::POST, p, body) if p.starts_with("/agents/") && p.ends_with("/messages") => {
            let name = p
                .trim_start_matches("/agents/")
                .trim_end_matches("/messages");
            if let Err(resp) = validate_name(name) {
                return ok_with_cors(resp, origin.as_deref());
            }
            let body: SendMessageBody = match read_body_json(body).await {
                Ok(b) => b,
                Err(resp) => return ok_with_cors(resp, origin.as_deref()),
            };
            let slot = state.controller.lock().await;
            let Some(ctrl) = slot.as_ref() else {
                return ok_with_cors(
                    response_json(
                        StatusCode::BAD_REQUEST,
                        json!({"error":"No active session. Call POST /session/init first."}),
                    ),
                    origin.as_deref(),
                );
            };
            if let Err(e) = ctrl.send(name, &body.message, body.summary).await {
                return ok_with_cors(
                    response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
                    origin.as_deref(),
                );
            }
            response_json(StatusCode::OK, json!({"ok": true}))
        }
        (Method::POST, p, _) if p.starts_with("/agents/") && p.ends_with("/kill") => {
            let name = p.trim_start_matches("/agents/").trim_end_matches("/kill");
            if let Err(resp) = validate_name(name) {
                return ok_with_cors(resp, origin.as_deref());
            }
            let slot = state.controller.lock().await;
            let Some(ctrl) = slot.as_ref() else {
                return ok_with_cors(
                    response_json(
                        StatusCode::BAD_REQUEST,
                        json!({"error":"No active session. Call POST /session/init first."}),
                    ),
                    origin.as_deref(),
                );
            };
            if let Err(e) = ctrl.kill_agent(name).await {
                return ok_with_cors(
                    response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
                    origin.as_deref(),
                );
            }
            response_json(StatusCode::OK, json!({"ok": true}))
        }
        (Method::POST, p, _) if p.starts_with("/agents/") && p.ends_with("/shutdown") => {
            let name = p
                .trim_start_matches("/agents/")
                .trim_end_matches("/shutdown");
            if let Err(resp) = validate_name(name) {
                return ok_with_cors(resp, origin.as_deref());
            }
            let slot = state.controller.lock().await;
            let Some(ctrl) = slot.as_ref() else {
                return ok_with_cors(
                    response_json(
                        StatusCode::BAD_REQUEST,
                        json!({"error":"No active session. Call POST /session/init first."}),
                    ),
                    origin.as_deref(),
                );
            };
            if let Err(e) = ctrl.shutdown_agent(name, "API shutdown requested").await {
                return ok_with_cors(
                    response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
                    origin.as_deref(),
                );
            }
            response_json(StatusCode::OK, json!({"ok": true}))
        }
        (Method::POST, p, _) if p.starts_with("/agents/") && p.ends_with("/approve-plan") => {
            let name = p
                .trim_start_matches("/agents/")
                .trim_end_matches("/approve-plan");
            if let Err(resp) = validate_name(name) {
                return ok_with_cors(resp, origin.as_deref());
            }
            // v1: controller auto-approves; accept for API parity.
            response_json(StatusCode::OK, json!({"ok": true}))
        }
        (Method::POST, p, _) if p.starts_with("/agents/") && p.ends_with("/approve-permission") => {
            let name = p
                .trim_start_matches("/agents/")
                .trim_end_matches("/approve-permission");
            if let Err(resp) = validate_name(name) {
                return ok_with_cors(resp, origin.as_deref());
            }
            // v1: controller auto-approves; accept for API parity.
            response_json(StatusCode::OK, json!({"ok": true}))
        }
        (Method::POST, p, body) if p == "/broadcast" => {
            let body: BroadcastBody = match read_body_json(body).await {
                Ok(b) => b,
                Err(resp) => return ok_with_cors(resp, origin.as_deref()),
            };
            let slot = state.controller.lock().await;
            let Some(ctrl) = slot.as_ref() else {
                return ok_with_cors(
                    response_json(
                        StatusCode::BAD_REQUEST,
                        json!({"error":"No active session. Call POST /session/init first."}),
                    ),
                    origin.as_deref(),
                );
            };
            let agents = match ctrl.list_agents().await {
                Ok(list) => list,
                Err(e) => {
                    return ok_with_cors(
                        response_json(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e})),
                        origin.as_deref(),
                    )
                }
            };
            for (name, _typ, _model, _running) in agents {
                let _ = ctrl.send(&name, &body.message, body.summary.clone()).await;
            }
            response_json(StatusCode::OK, json!({"ok": true}))
        }
        _ => response_json(StatusCode::NOT_FOUND, json!({"error":"not_found"})),
    };

    apply_cors_headers(&mut resp, origin.as_deref());
    Ok(resp)
}

pub(crate) async fn start_claude_controller_api(
    controller_slot: Arc<Mutex<Option<ClaudeTeamsController>>>,
    token: String,
) -> anyhow::Result<()> {
    let port = std::env::var("PHANTOM_CLAUDE_CTRL_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let state = ApiState {
        config: ClaudeCtrlConfig { port, token },
        controller: controller_slot,
        started_at: chrono::Utc::now().timestamp_millis(),
    };

    let make_svc = make_service_fn(move |_conn| {
        let state = state.clone();
        async move { Ok::<_, Infallible>(service_fn(move |req| handle(req, state.clone()))) }
    });

    let server = match Server::try_bind(&addr) {
        Ok(b) => b,
        Err(err) => {
            eprintln!(
                "[Harness] Claude controller API could not bind to http://127.0.0.1:{port}: {err}"
            );
            return Ok(());
        }
    };

    println!(
        "[Harness] Claude controller API listening on http://127.0.0.1:{}/ (token required)",
        addr.port()
    );
    server
        .http1_keepalive(true)
        .http1_only(true)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .serve(make_svc)
        .await?;
    Ok(())
}
