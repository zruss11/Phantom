//! Amp CLI helper for non-interactive execution and NDJSON parsing.

use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

/// Execute Amp in programmatic mode and return extracted text.
pub async fn execute(prompt: &str) -> Result<String, String> {
    let mut cmd = Command::new("amp");
    cmd.args(["--stream-json", "--execute", prompt])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn amp: {}", e))?;

    let stdout = child.stdout.take().ok_or("Failed to capture amp stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture amp stderr")?;

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

        if json.get("type").and_then(|t| t.as_str()) == Some("result") {
            if let Some(result) = json.get("result").and_then(|r| r.as_str()) {
                if !result.is_empty() {
                    result_text = result.to_string();
                }
            }
            break;
        }
    }

    let _ = child.kill().await;

    let stderr_output = stderr_task.await.unwrap_or_default();

    if result_text.is_empty() {
        if let Some(error) = last_error {
            return Err(error);
        }
        let stderr_preview = stderr_output.trim();
        if !stderr_preview.is_empty() {
            return Err(format!("No text in Amp response (stderr: {})", stderr_preview));
        }
        return Err("No text in Amp response".to_string());
    }

    Ok(result_text)
}

fn extract_text_from_event(json: &Value) -> Option<String> {
    if let Some(message) = json.get("message") {
        if let Some(content) = message.get("content") {
            if let Some(text) = extract_text_from_content(content) {
                return Some(text);
            }
        }
    }

    if let Some(content) = json.get("content") {
        if let Some(text) = extract_text_from_content(content) {
            return Some(text);
        }
    }

    if let Some(result) = json.get("result") {
        if let Some(text) = result.as_str() {
            return Some(text.to_string());
        }
    }

    None
}

fn extract_text_from_content(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    if let Some(items) = content.as_array() {
        let mut buf = String::new();
        for item in items {
            if let Some(item_text) = item.get("text").and_then(|t| t.as_str()) {
                buf.push_str(item_text);
                continue;
            }
            if let Some(item_text) = item.get("delta").and_then(|t| t.as_str()) {
                buf.push_str(item_text);
                continue;
            }
            if item.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                if let Some(item_text) = item.get("text").and_then(|t| t.as_str()) {
                    buf.push_str(item_text);
                }
            }
        }
        if !buf.is_empty() {
            return Some(buf);
        }
    }

    None
}

fn extract_error_from_event(json: &Value) -> Option<String> {
    if json.get("is_error").and_then(|v| v.as_bool()) == Some(true) {
        if let Some(error) = json.get("error") {
            return Some(error_to_string(error));
        }
        if let Some(message) = json.get("message") {
            return Some(error_to_string(message));
        }
    }

    if json.get("type").and_then(|t| t.as_str()) == Some("error") {
        if let Some(error) = json.get("error") {
            return Some(error_to_string(error));
        }
        if let Some(message) = json.get("message") {
            return Some(error_to_string(message));
        }
    }

    if let Some(subtype) = json.get("subtype").and_then(|v| v.as_str()) {
        if subtype.contains("error") {
            if let Some(error) = json.get("error") {
                return Some(error_to_string(error));
            }
        }
    }

    None
}

fn error_to_string(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(message) = value.get("message").and_then(|v| v.as_str()) {
        return message.to_string();
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_from_assistant_message() {
        let json = serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "Hello"}]}
        });
        assert_eq!(extract_text_from_event(&json), Some("Hello".to_string()));
    }

    #[test]
    fn extracts_text_from_top_level_content() {
        let json = serde_json::json!({
            "type": "assistant_message",
            "content": "Summary text"
        });
        assert_eq!(extract_text_from_event(&json), Some("Summary text".to_string()));
    }

    #[test]
    fn extracts_text_from_result() {
        let json = serde_json::json!({
            "type": "result",
            "result": "Done"
        });
        assert_eq!(extract_text_from_event(&json), Some("Done".to_string()));
    }

    #[test]
    fn extracts_error_from_result_error() {
        let json = serde_json::json!({
            "type": "result",
            "subtype": "error_during_execution",
            "is_error": true,
            "error": "402 payment required"
        });
        assert_eq!(extract_error_from_event(&json), Some("402 payment required".to_string()));
    }
}
