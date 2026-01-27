//! AI-powered branch name and title generation.
//!
//! Uses cheap, fast models to generate descriptive branch names and titles
//! from user prompts. Each agent type uses an appropriate model:
//! - Claude Code: claude-haiku-4-5-20251001 (OAuth)
//! - Codex: gpt-5.1-codex-mini (ChatGPT backend)

use crate::utils::truncate_str;
use crate::worktree::sanitize_branch_name;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Metadata generated for a task run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    /// Human-readable title (3-7 words, Title Case)
    pub title: String,
    /// Git branch name (kebab-case with prefix like feat/, fix/, etc.)
    pub branch_name: String,
}

/// Response format from AI model
#[derive(Debug, Deserialize)]
struct NameGenResponse {
    title: String,
    #[serde(rename = "branchName")]
    branch_name: String,
}

/// The prompt template for generating run metadata
const GENERATION_PROMPT: &str = r#"You create concise run metadata for a coding task.
Return ONLY a JSON object with keys:
- title: short, clear, 3-7 words, Title Case
- branchName: lower-case, kebab-case slug prefixed with one of: feat/, fix/, chore/, test/, docs/, refactor/, perf/.

Choose fix/ when the task is a bug fix, error, regression, crash, or cleanup.
Use the closest match for chores/tests/docs/refactors/perf.
Otherwise use feat/.

Examples:
{"title":"Fix Login Redirect Loop","branchName":"fix/login-redirect-loop"}
{"title":"Add Workspace Home View","branchName":"feat/workspace-home"}
{"title":"Update Dependencies","branchName":"chore/update-deps"}
{"title":"Refactor Auth Module","branchName":"refactor/auth-module"}

Task:
"#;

/// Generate branch name and title from a prompt using a cheap AI model.
///
/// # Arguments
/// * `prompt` - The user's task prompt
/// * `agent_id` - The agent type ("claude-code" or "codex")
/// * `api_key` - The API key for the provider (Anthropic or OpenAI)
///
/// # Returns
/// A `RunMetadata` with the generated title and sanitized branch name.
pub async fn generate_run_metadata(
    prompt: &str,
    agent_id: &str,
    _api_key: Option<&str>,
) -> Result<RunMetadata, String> {
    // Truncate prompt if too long (keep first 500 chars, safe for UTF-8)
    let truncated_prompt = truncate_str(prompt, 500);

    let full_prompt = format!("{}{}", GENERATION_PROMPT, truncated_prompt);

    let response = match agent_id {
        "codex" => generate_with_codex_backend(&full_prompt).await?,
        "claude-code" => generate_with_claude_oauth(&full_prompt).await?,
        _ => {
            // Fallback: generate a simple name from the prompt
            return Ok(generate_fallback(&truncated_prompt));
        }
    };

    // Parse JSON response
    let parsed: NameGenResponse = serde_json::from_str(&response).map_err(|e| {
        format!(
            "Failed to parse AI response: {} (response: {})",
            e, response
        )
    })?;

    // Sanitize the branch name
    let branch_name = sanitize_branch_name(&parsed.branch_name);

    Ok(RunMetadata {
        title: parsed.title,
        branch_name,
    })
}

/// Generate using Claude OAuth token (same setup as title summarization).
async fn generate_with_claude_oauth(prompt: &str) -> Result<String, String> {
    let token = crate::get_claude_oauth_token().ok_or("Claude OAuth token not found")?;

    let payload = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 200,
        "messages": [{"role": "user", "content": prompt}]
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

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
        .map(|s| extract_json_from_text(s))
        .ok_or_else(|| "No content in Claude response".to_string())
}

/// Generate using Codex ChatGPT backend (same setup as title summarization).
async fn generate_with_codex_backend(prompt: &str) -> Result<String, String> {
    let (token, account_id) = get_codex_auth()?;
    let account_id = account_id.ok_or("Codex account_id required for API calls")?;

    let payload = serde_json::json!({
        "model": "gpt-5.1-codex-mini",
        "instructions": "You create concise run metadata for coding tasks.",
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
        let body_preview = truncate_str(&body, 200);
        return Err(format!("Codex API error: {} - {}", status, body_preview));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Read failed: {}", e))?;
    let text = extract_codex_sse_text(&body)?;
    Ok(extract_json_from_text(&text))
}

/// Extract JSON object from potentially wrapped text response.
/// Models sometimes wrap JSON in markdown code blocks or extra text.
fn extract_json_from_text(text: &str) -> String {
    let text = text.trim();

    // Try to find JSON object in the text
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }

    text.to_string()
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

/// Extract text from Codex SSE stream response.
fn extract_codex_sse_text(sse_body: &str) -> Result<String, String> {
    for line in sse_body.lines() {
        if let Some(json_str) = line.strip_prefix("data: ") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                if json["type"].as_str() == Some("response.output_text.done") {
                    if let Some(text) = json["text"].as_str() {
                        return Ok(clean_response(text));
                    }
                }
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

/// Clean up LLM response (remove quotes, trim whitespace)
fn clean_response(text: &str) -> String {
    text.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

/// Generate fallback metadata when AI is unavailable.
fn generate_fallback(prompt: &str) -> RunMetadata {
    // Extract first few words for title
    let words: Vec<&str> = prompt.split_whitespace().take(5).collect();
    let title = if words.is_empty() {
        "New Task".to_string()
    } else {
        // Title case the words
        words
            .iter()
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    // Generate branch name from title
    let branch_name = sanitize_branch_name(&title);

    RunMetadata { title, branch_name }
}

/// Generate run metadata with a timeout.
///
/// Falls back to simple name generation if the AI call times out or fails.
pub async fn generate_run_metadata_with_timeout(
    prompt: &str,
    agent_id: &str,
    api_key: Option<&str>,
    timeout_secs: u64,
) -> RunMetadata {
    let result = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        generate_run_metadata(prompt, agent_id, api_key),
    )
    .await;

    match result {
        Ok(Ok(metadata)) => metadata,
        Ok(Err(e)) => {
            eprintln!("[namegen] AI generation failed: {}", e);
            generate_fallback(prompt)
        }
        Err(_) => {
            eprintln!("[namegen] AI generation timed out after {}s", timeout_secs);
            generate_fallback(prompt)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_fallback() {
        let meta = generate_fallback("fix the login bug in auth module");
        assert!(!meta.title.is_empty());
        assert!(!meta.branch_name.is_empty());
        assert!(meta.branch_name.starts_with("feat/"));
    }

    #[test]
    fn test_extract_json_from_text() {
        assert_eq!(
            extract_json_from_text(r#"{"title":"Test","branchName":"feat/test"}"#),
            r#"{"title":"Test","branchName":"feat/test"}"#
        );

        assert_eq!(
            extract_json_from_text(r#"Here's the JSON: {"title":"Test","branchName":"feat/test"}"#),
            r#"{"title":"Test","branchName":"feat/test"}"#
        );

        assert_eq!(
            extract_json_from_text(
                r#"```json
{"title":"Test","branchName":"feat/test"}
```"#
            ),
            r#"{"title":"Test","branchName":"feat/test"}"#
        );
    }

    #[test]
    fn test_fallback_title_case() {
        let meta = generate_fallback("add user notifications");
        assert_eq!(meta.title, "Add User Notifications");
    }
}
