//! AI-powered summarization for task titles and status messages.
//! Uses OAuth tokens from existing Claude/Codex logins.

use crate::utils::safe_prefix;
use std::time::Duration;

/// Generate a short title from a task prompt (async with timeout)
pub async fn summarize_title(prompt: &str, agent_id: &str) -> String {
    let result =
        tokio::time::timeout(Duration::from_secs(5), generate_title(prompt, agent_id)).await;

    match result {
        Ok(Ok(title)) => title,
        Ok(Err(e)) => {
            println!("[Summarize] Title generation failed: {}", e);
            fallback_title(prompt)
        }
        Err(_) => {
            println!("[Summarize] Title generation timed out");
            fallback_title(prompt)
        }
    }
}

/// Generate a status summary from agent response (async with timeout)
pub async fn summarize_status(response: &str, agent_id: &str) -> String {
    let result =
        tokio::time::timeout(Duration::from_secs(5), generate_status(response, agent_id)).await;

    match result {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            println!("[Summarize] Status generation failed: {}", e);
            truncate_status(response)
        }
        Err(_) => {
            println!("[Summarize] Status generation timed out");
            truncate_status(response)
        }
    }
}

async fn generate_title(prompt: &str, agent_id: &str) -> Result<String, String> {
    // Truncate to first 300 chars to keep token usage low (safe for UTF-8)
    let truncated = safe_prefix(prompt, 300);
    let full_prompt = format!(
        "Create a 15-30 character title for this task. Use Title Case. Return ONLY the title, no quotes or extra text.\n\n{}",
        truncated
    );

    match agent_id {
        "codex" => call_codex_api(&full_prompt).await,
        "amp" => call_amp_cli(&full_prompt).await,
        // For claude-code and unknown agents, use Claude API
        _ => call_claude_api(&full_prompt).await,
    }
}

async fn generate_status(response: &str, agent_id: &str) -> Result<String, String> {
    // Check for PR links first - handle them directly without LLM
    if let Some(pr_summary) = extract_pr_summary(response) {
        return Ok(pr_summary);
    }

    // Truncate to first 500 chars (safe for UTF-8)
    let truncated = safe_prefix(response, 500);
    let full_prompt = format!(
        "Summarize what was done in max 40 characters. Return ONLY the summary, no quotes. Do NOT try to access any URLs - just describe what was done based on the text.\n\n{}",
        truncated
    );

    match agent_id {
        "claude-code" => call_claude_api(&full_prompt).await,
        "codex" => call_codex_api(&full_prompt).await,
        "amp" => call_amp_cli(&full_prompt).await,
        _ => call_claude_api(&full_prompt).await,
    }
}

/// Call Claude API using OAuth token
/// IMPORTANT: OAuth requires the anthropic-beta header!
async fn call_claude_api(prompt: &str) -> Result<String, String> {
    let token = crate::get_claude_oauth_token().ok_or("Claude OAuth token not found")?;

    let payload = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": prompt}]
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "oauth-2025-04-20") // Required for OAuth!
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    // Check for auth errors
    if response.status() == 401 {
        return Err("Claude OAuth token expired or invalid".to_string());
    }

    if !response.status().is_success() {
        return Err(format!("Claude API error: {}", response.status()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Parse failed: {}", e))?;

    json["content"][0]["text"]
        .as_str()
        .map(|s| clean_response(s))
        .ok_or_else(|| "No content in response".to_string())
}

/// Call Codex API for summarization using the ChatGPT backend Responses API
/// Requires specific headers discovered from clawdbot integration
async fn call_codex_api(prompt: &str) -> Result<String, String> {
    let (token, account_id) = get_codex_auth()?;

    let account_id = account_id.ok_or("Codex account_id required for API calls")?;

    // Use the Responses API format with ChatGPT backend
    // Based on clawdbot's integration: https://github.com/clawdbot/clawdbot
    let payload = serde_json::json!({
        "model": "gpt-5.1-codex-mini",
        "instructions": "You create short titles for coding tasks.",
        "input": [{
            "role": "user",
            "content": [{"type": "input_text", "text": prompt}]
        }],
        "stream": true,
        "store": false
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://chatgpt.com/backend-api/codex/responses")
        .header("Authorization", format!("Bearer {}", token))
        .header("chatgpt-account-id", account_id)
        .header("OpenAI-Beta", "responses=experimental")
        .header("originator", "codex_cli_rs")
        .header("accept", "text/event-stream")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if response.status() == 401 {
        return Err("Codex token expired or invalid".to_string());
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let body_preview = safe_prefix(&body, 200);
        return Err(format!("Codex API error: {} - {}", status, body_preview));
    }

    // Parse SSE stream to extract the final text
    let body = response
        .text()
        .await
        .map_err(|e| format!("Read failed: {}", e))?;
    extract_codex_sse_text(&body)
}

/// Call Amp CLI for summarization using programmatic mode
/// Spawns amp with --execute --stream-json and parses the NDJSON output
async fn call_amp_cli(prompt: &str) -> Result<String, String> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let mut cmd = Command::new("amp");
    cmd.args(["--stream-json", "--execute", prompt])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn amp: {}", e))?;

    let stdout = child.stdout.take().ok_or("Failed to capture amp stdout")?;

    let mut reader = BufReader::new(stdout).lines();
    let mut result_text = String::new();

    // Parse NDJSON output looking for assistant text or final result
    while let Ok(Some(line)) = reader.next_line().await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match event_type {
                "assistant" => {
                    // Extract text from message.content array
                    if let Some(content) = json
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    result_text.push_str(text);
                                }
                            }
                        }
                    }
                }
                "result" => {
                    // Check for final result text
                    if let Some(result) = json.get("result").and_then(|r| r.as_str()) {
                        if !result.is_empty() {
                            result_text = result.to_string();
                        }
                    }
                    break;
                }
                _ => {}
            }
        }
    }

    // Kill the process if still running
    let _ = child.kill().await;

    if result_text.is_empty() {
        return Err("No text in Amp response".to_string());
    }

    Ok(clean_response(&result_text))
}

/// Get Codex OAuth token and account ID from ~/.codex/auth.json
fn get_codex_auth() -> Result<(String, Option<String>), String> {
    let auth_path = dirs::home_dir()
        .ok_or("No home dir")?
        .join(".codex")
        .join("auth.json");

    let content =
        std::fs::read_to_string(&auth_path).map_err(|_| "Cannot read ~/.codex/auth.json")?;

    if content.len() > 1_000_000 {
        return Err("Auth file exceeds size limit".to_string());
    }

    let auth: serde_json::Value =
        serde_json::from_str(&content).map_err(|_| "Invalid auth.json format")?;

    let account_id = auth["tokens"]["account_id"].as_str().map(|s| s.to_string());

    if let Some(token) = auth["tokens"]["access_token"].as_str() {
        if !token.is_empty() {
            return Ok((token.to_string(), account_id));
        }
    }

    Err("No access_token found in auth.json".to_string())
}

/// Extract text from Codex SSE stream response
/// Looks for "response.output_text.done" event which contains the final text
fn extract_codex_sse_text(sse_body: &str) -> Result<String, String> {
    for line in sse_body.lines() {
        if let Some(json_str) = line.strip_prefix("data: ") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                // Look for response.output_text.done event
                if json["type"].as_str() == Some("response.output_text.done") {
                    if let Some(text) = json["text"].as_str() {
                        return Ok(clean_response(text));
                    }
                }
                // Also check response.completed for final output
                if json["type"].as_str() == Some("response.completed") {
                    if let Some(output) = json["response"]["output"].as_array() {
                        for item in output {
                            if item["type"].as_str() == Some("message") {
                                if let Some(content) = item["content"].as_array() {
                                    for c in content {
                                        if c["type"].as_str() == Some("output_text") {
                                            if let Some(text) = c["text"].as_str() {
                                                return Ok(clean_response(text));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Err("Could not extract text from Codex SSE response".to_string())
}

/// Extract PR number from GitHub PR URLs and return a formatted summary
/// Returns None if no qualifying PR link is found, allowing fallback to LLM summarization
fn extract_pr_summary(text: &str) -> Option<String> {
    // Look for GitHub PR URLs: https://github.com/owner/repo/pull/123
    // Use a simple pattern match rather than regex for performance
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }

    let normalized: Vec<String> = words.iter().map(|word| normalize_token(word)).collect();

    for (idx, word) in words.iter().enumerate() {
        if let Some(pr_part) = word.strip_prefix("https://github.com/") {
            if let Some(pull_idx) = pr_part.find("/pull/") {
                let after_pull = &pr_part[pull_idx + 6..]; // Skip "/pull/"
                let pr_number: String =
                    after_pull.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !pr_number.is_empty() && has_opened_created_context(&normalized, idx) {
                    return Some(format!("PR #{} opened", pr_number));
                }
            }
        }
    }

    None
}

fn normalize_token(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_ascii_lowercase()
}

fn has_opened_created_context(tokens: &[String], url_index: usize) -> bool {
    const WINDOW: usize = 4;
    let start = url_index.saturating_sub(WINDOW);
    let end = (url_index + WINDOW + 1).min(tokens.len());

    for token in &tokens[start..end] {
        if matches!(token.as_str(), "opened" | "created") {
            return true;
        }
    }

    false
}

/// Clean up LLM response (remove quotes, trim whitespace)
fn clean_response(text: &str) -> String {
    text.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

/// Fallback title: Take first 5 words, title case them
fn fallback_title(prompt: &str) -> String {
    prompt
        .split_whitespace()
        .take(5)
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fallback status: Truncate to a short preview with ellipsis
fn truncate_status(text: &str) -> String {
    // Take first line only
    let clean = text.lines().next().unwrap_or(text);
    const MAX_LEN: usize = 41; // 38 chars + "..."
    const PREFIX_LEN: usize = 38;

    if clean.len() <= MAX_LEN {
        clean.to_string()
    } else {
        format!("{}...", &clean[..PREFIX_LEN])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_title() {
        assert_eq!(
            fallback_title("fix the login redirect loop in auth.ts"),
            "Fix The Login Redirect Loop"
        );
        assert_eq!(fallback_title("hello"), "Hello");
        assert_eq!(fallback_title(""), "");
    }

    #[test]
    fn test_truncate_status() {
        assert_eq!(truncate_status("Short text"), "Short text");
        assert_eq!(
            truncate_status("This is a very long status message that exceeds forty characters"),
            "This is a very long status message tha..."
        );
    }

    #[test]
    fn test_clean_response() {
        assert_eq!(clean_response("  \"Hello World\"  "), "Hello World");
        assert_eq!(clean_response("'Test'"), "Test");
    }

    #[test]
    fn test_extract_pr_summary() {
        // Standard GitHub PR URL
        assert_eq!(
            extract_pr_summary("Created PR https://github.com/user/repo/pull/123"),
            Some("PR #123 opened".to_string())
        );

        // PR URL with trailing content
        assert_eq!(
            extract_pr_summary("See https://github.com/org/project/pull/456 for details"),
            None
        );

        // No PR link
        assert_eq!(
            extract_pr_summary("Fixed the bug in auth.ts"),
            None
        );

        // GitHub URL but not a PR
        assert_eq!(
            extract_pr_summary("Check https://github.com/user/repo/issues/123"),
            None
        );

        // Multiple URLs, only explicit opened/created wins
        assert_eq!(
            extract_pr_summary("Reviewed https://github.com/a/b/pull/1 and opened https://github.com/c/d/pull/2"),
            Some("PR #2 opened".to_string())
        );

        // PR URL without opened/created language
        assert_eq!(
            extract_pr_summary("Reviewed PR https://github.com/a/b/pull/7"),
            None
        );
    }
}
