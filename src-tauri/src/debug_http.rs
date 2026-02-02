use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex as StdMutex};

use rusqlite::Connection;
use serde_json::json;

use crate::db;
use phantom_harness_backend::cli::{AgentCliKind, AgentProcessClient};

fn respond_json(mut stream: TcpStream, status: &str, body: serde_json::Value) {
    let body_str = body.to_string();
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body_str.len(),
        body_str
    );
    let _ = stream.write_all(resp.as_bytes());
}

fn handle_request(stream: TcpStream, db_conn: Arc<StdMutex<Connection>>) {
    let mut stream = stream;
    let mut buf = [0u8; 8192];
    let Ok(n) = stream.read(&mut buf) else {
        return;
    };
    if n == 0 {
        return;
    }

    let req = String::from_utf8_lossy(&buf[..n]);
    let Some(first) = req.lines().next() else {
        return;
    };

    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    if method != "GET" {
        respond_json(
            stream,
            "405 Method Not Allowed",
            json!({"error":"method_not_allowed"}),
        );
        return;
    }

    // crude query parsing
    let (path_only, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };

    match path_only {
        "/health" => respond_json(stream, "200 OK", json!({"ok": true})),
        "/tasks" => {
            let conn = match db_conn.lock() {
                Ok(c) => c,
                Err(_) => {
                    respond_json(
                        stream,
                        "500 Internal Server Error",
                        json!({"error":"db_lock"}),
                    );
                    return;
                }
            };
            let tasks = match db::list_tasks(&conn) {
                Ok(t) => t,
                Err(e) => {
                    respond_json(
                        stream,
                        "500 Internal Server Error",
                        json!({"error": format!("db_list_tasks: {e}")}),
                    );
                    return;
                }
            };
            let out: Vec<_> = tasks
                .into_iter()
                .map(|t| {
                    json!({
                        "id": t.id,
                        "agent_id": t.agent_id,
                        "model": t.model,
                        "prompt": t.prompt,
                        "project_path": t.project_path,
                        "status": t.status,
                        "status_state": t.status_state,
                        "cost": t.cost,
                        "created_at": t.created_at,
                        "updated_at": t.updated_at,
                        "title_summary": t.title_summary,
                        "agent_session_id": t.agent_session_id,
                    })
                })
                .collect();
            respond_json(stream, "200 OK", json!({"tasks": out}));
        }
        "/claude" => {
            // Run a one-shot Claude prompt via the backend adapter.
            // Example: /claude?prompt=hello
            let prompt = query
                .and_then(|q| {
                    q.split('&').find_map(|kv| {
                        kv.split_once('=')
                            .filter(|(k, _)| *k == "prompt")
                            .map(|(_, v)| v)
                    })
                })
                .map(|v| urlencoding::decode(v).ok().map(|x| x.to_string()))
                .flatten()
                .unwrap_or_else(|| "reply with exactly OK".to_string());

            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    respond_json(
                        stream,
                        "500 Internal Server Error",
                        json!({"error": format!("runtime: {e}")}),
                    );
                    return;
                }
            };

            let result = rt.block_on(async {
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
                let client = AgentProcessClient::spawn(
                    "claude",
                    &vec!["--output-format".to_string(), "stream-json".to_string()],
                    &cwd,
                    &[],
                    AgentCliKind::Claude,
                )
                .await?;
                client.session_set_mode("", "bypassPermissions").await?;
                let res = tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    client.session_prompt_streaming("", &prompt, |_u| {}),
                )
                .await??;

                let text = res
                    .messages
                    .iter()
                    .filter(|m| m.message_type == "assistant_message")
                    .filter_map(|m| m.content.as_ref())
                    .fold(String::new(), |mut acc, s| {
                        acc.push_str(s);
                        acc
                    });

                anyhow::Ok(text)
            });

            match result {
                Ok(text) => respond_json(stream, "200 OK", json!({"ok": true, "text": text})),
                Err(e) => respond_json(
                    stream,
                    "500 Internal Server Error",
                    json!({"ok": false, "error": e.to_string()}),
                ),
            }
        }
        _ if path_only.starts_with("/task/") => {
            let id = path_only.trim_start_matches("/task/");
            let conn = match db_conn.lock() {
                Ok(c) => c,
                Err(_) => {
                    respond_json(
                        stream,
                        "500 Internal Server Error",
                        json!({"error":"db_lock"}),
                    );
                    return;
                }
            };
            let tasks = match db::list_tasks(&conn) {
                Ok(t) => t,
                Err(e) => {
                    respond_json(
                        stream,
                        "500 Internal Server Error",
                        json!({"error": format!("db_list_tasks: {e}")}),
                    );
                    return;
                }
            };
            let Some(t) = tasks.into_iter().find(|t| t.id == id) else {
                respond_json(stream, "404 Not Found", json!({"error":"not_found"}));
                return;
            };
            respond_json(
                stream,
                "200 OK",
                json!({
                    "id": t.id,
                    "agent_id": t.agent_id,
                    "model": t.model,
                    "prompt": t.prompt,
                    "project_path": t.project_path,
                    "status": t.status,
                    "status_state": t.status_state,
                    "cost": t.cost,
                    "created_at": t.created_at,
                    "updated_at": t.updated_at,
                    "title_summary": t.title_summary,
                    "agent_session_id": t.agent_session_id,
                }),
            );
        }
        _ => respond_json(stream, "404 Not Found", json!({"error":"not_found"})),
    }
}

pub fn start_debug_http(db: Arc<StdMutex<Connection>>) -> anyhow::Result<()> {
    let port: u16 = std::env::var("PHANTOM_DEBUG_HTTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(43777);

    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("[Harness] debug http listening on http://127.0.0.1:{port}");

    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let db_conn = db.clone();
            std::thread::spawn(move || handle_request(stream, db_conn));
        }
    }

    Ok(())
}
