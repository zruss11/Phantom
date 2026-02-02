//! OpenCode CLI helper for non-interactive execution and JSON event parsing.

use serde_json::Value;
use std::process::Stdio;
use std::sync::OnceLock;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use crate::utils::resolve_command_path;

const FREE_MODELS: &[&str] = &[
    "opencode/glm-4.7-free",
    "opencode/minimax-m2.1-free",
    "opencode/kimi-k2.5-free",
    "opencode/big-pickle",
    "opencode/gpt-5-nano",
    "opencode/trinity-large-preview-free",
];
const DEFAULT_MODEL: &str = "opencode/glm-4.7-free";

/// Execute OpenCode in non-interactive mode and return extracted text.
pub async fn execute(prompt: &str) -> Result<String, String> {
    let _guard = opencode_lock().lock().await;
    execute_with_retries(prompt).await
}

async fn execute_with_retries(prompt: &str) -> Result<String, String> {
    let mut last_err = String::new();
    for attempt in 0..3 {
        match execute_with_model_fallback(prompt).await {
            Ok(text) => return Ok(text),
            Err(err) => {
                last_err = err.clone();
                if attempt < 2 && should_retry(&err) {
                    sleep(retry_delay(attempt)).await;
                    continue;
                }
                return Err(err);
            }
        }
    }
    Err(last_err)
}

async fn execute_with_model_fallback(prompt: &str) -> Result<String, String> {
    match execute_with_model(prompt, Some(DEFAULT_MODEL)).await {
        Ok(text) => Ok(text),
        Err(err) => {
            if should_try_alternate_model(&err) {
                for model in FREE_MODELS.iter().skip(1) {
                    if let Ok(text) = execute_with_model(prompt, Some(model)).await {
                        return Ok(text);
                    }
                }
            }
            Err(err)
        }
    }
}

async fn execute_with_model(prompt: &str, model: Option<&str>) -> Result<String, String> {
    let command_path = resolve_command_path("opencode")
        .unwrap_or_else(|| PathBuf::from("opencode"));
    let mut cmd = Command::new(command_path);
    cmd.args(["run", "--format", "json"]);
    if let Some(model) = model {
        if !model.trim().is_empty() {
            cmd.args(["--model", model]);
        }
    }
    cmd.arg(prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn opencode: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture opencode stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to capture opencode stderr")?;

    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut buf = String::new();
        let _ = reader.read_to_string(&mut buf).await;
        buf
    });

    let mut reader = BufReader::new(stdout).lines();
    let mut result_text = String::new();
    let mut last_error: Option<String> = None;

    while let Ok(Some(line)) = reader.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        let json: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if let Some(error) = extract_error_from_event(&json) {
            last_error = Some(error);
        }

        if let Some(text) = extract_text_from_event(&json) {
            if !text.is_empty() {
                result_text.push_str(&text);
            }
        }
    }

    let _ = child.wait().await;
    let stderr_output = stderr_task.await.unwrap_or_default();

    if result_text.is_empty() {
        if let Some(error) = last_error {
            return Err(error);
        }
        let stderr_preview = stderr_output.trim();
        if !stderr_preview.is_empty() {
            return Err(format!(
                "No text in OpenCode response (stderr: {})",
                stderr_preview
            ));
        }
        return Err("No text in OpenCode response".to_string());
    }

    Ok(result_text)
}

fn extract_text_from_event(json: &Value) -> Option<String> {
    if json.get("type").and_then(|t| t.as_str()) == Some("text") {
        if let Some(text) = json
            .get("part")
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
        {
            return Some(text.to_string());
        }
    }

    None
}

fn extract_error_from_event(json: &Value) -> Option<String> {
    if json.get("type").and_then(|t| t.as_str()) == Some("error") {
        if let Some(error) = json.get("error") {
            if let Some(message) = error.get("data").and_then(|d| d.get("message")) {
                if let Some(text) = message.as_str() {
                    return Some(text.to_string());
                }
            }
            if let Some(text) = error.get("name").and_then(|n| n.as_str()) {
                return Some(text.to_string());
            }
        }
    }

    None
}

fn should_try_alternate_model(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("model") || lower.contains("not found")
}

fn should_retry(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("high concurrency usage")
        || lower.contains("code\":\"1302\"")
        || lower.contains("enoent")
        || lower.contains("cannot find module")
        || lower.contains("unexpected error")
}

fn retry_delay(attempt: usize) -> Duration {
    match attempt {
        0 => Duration::from_millis(500),
        1 => Duration::from_millis(1500),
        _ => Duration::from_millis(3000),
    }
}

fn opencode_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extracts_text_from_text_event() {
        let json = serde_json::json!({
            "type": "text",
            "part": {"text": "Hello"}
        });
        assert_eq!(extract_text_from_event(&json), Some("Hello".to_string()));
    }

    #[test]
    fn test_extracts_error_from_error_event() {
        let json = serde_json::json!({
            "type": "error",
            "error": {"data": {"message": "bad request"}}
        });
        assert_eq!(
            extract_error_from_event(&json),
            Some("bad request".to_string())
        );
    }
}
