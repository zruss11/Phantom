mod claude_local_usage;
mod claude_usage_watcher;
mod db;
mod debug_http;
mod discord_bot;
mod local_usage;
mod logger;
mod mcp_server;
mod namegen;
mod summarize;
mod utils;
mod webhook;
mod worktree;

use utils::{resolve_gh_binary, truncate_str};

use chrono::TimeZone;
use phantom_harness_backend::cli::{
    AgentProcessClient, AvailableCommand, ImageContent, StreamingUpdate, TokenUsageInfo,
    UserInputQuestion,
};
use phantom_harness_backend::{
    apply_model_selection, get_agent_models as backend_get_agent_models,
    get_agent_modes as backend_get_agent_modes, get_codex_models as backend_get_codex_models,
    get_codex_models_enriched as backend_get_codex_models_enriched,
    get_codex_modes as backend_get_codex_modes, get_factory_custom_models, AgentLaunchConfig,
    CancellationToken, EnrichedModelOption, ModeOption, ModelOption,
};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Instant;
use tauri::{
    AppHandle, Emitter, LogicalPosition, Manager, Position, State, WebviewUrl, WebviewWindow,
};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri_plugin_updater::UpdaterExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

use debug_http::start_debug_http;
use mcp_server::{start_mcp_server, McpConfig};

/// Model pricing (per million tokens): (model_pattern, input_rate, output_rate)
/// Rates are in USD per 1M tokens
const MODEL_PRICING: &[(&str, f64, f64)] = &[
    // OpenAI models (https://openai.com/api/pricing/)
    ("gpt-5.1-mini", 0.40, 1.60),
    ("gpt-5.1", 2.50, 10.00),
    ("gpt-5", 5.00, 15.00),
    ("o4-mini", 1.10, 4.40),
    ("o3-mini", 1.10, 4.40),
    ("o3", 10.00, 40.00),
    ("gpt-4.1-mini", 0.40, 1.60),
    ("gpt-4.1-nano", 0.10, 0.40),
    ("gpt-4.1", 2.50, 10.00),
    ("gpt-4o-mini", 0.15, 0.60),
    ("gpt-4o", 2.50, 10.00),
    ("gpt-4-turbo", 10.00, 30.00),
    ("gpt-4", 30.00, 60.00),
    ("gpt-3.5-turbo", 0.50, 1.50),
    // Anthropic models (https://www.anthropic.com/pricing)
    ("claude-opus-4", 15.00, 75.00),
    ("claude-sonnet-4", 3.00, 15.00),
    ("claude-3-5-sonnet", 3.00, 15.00),
    ("claude-3-opus", 15.00, 75.00),
    ("claude-3-sonnet", 3.00, 15.00),
    ("claude-3-haiku", 0.25, 1.25),
];

/// Calculate cost from token usage for a given model
fn calculate_cost_from_usage(model: &str, usage: &TokenUsageInfo) -> f64 {
    let (input_rate, output_rate) = get_model_rates(model);
    let input_tokens = usage.last_token_usage.input_tokens;
    let output_tokens = usage.last_token_usage.output_tokens;

    let input_cost = (input_tokens as f64) * input_rate / 1_000_000.0;
    let output_cost = (output_tokens as f64) * output_rate / 1_000_000.0;

    input_cost + output_cost
}

/// Get pricing rates for a model (returns default rates if model not found)
fn get_model_rates(model: &str) -> (f64, f64) {
    let model_lower = model.to_lowercase();
    for (pattern, input, output) in MODEL_PRICING {
        if model_lower.contains(&pattern.to_lowercase()) {
            return (*input, *output);
        }
    }
    // Default rates (roughly GPT-4o rates)
    (2.50, 10.00)
}

/// Check if an Agent error is recoverable (exit code 143/SIGTERM)
/// These errors can be recovered by reconnecting the session
fn is_recoverable_exit(error: &str) -> bool {
    error.contains("exit code: 143")
        || error.contains("Exit code: 143")
        || error.contains("exited with code 143")
        || error.contains("SIGTERM")
        || error.contains("process was terminated")
        || error.contains("terminated by signal 15")
}

/// Check if an error is related to authentication/authorization failure
/// These errors require the user to re-authenticate
fn is_auth_error(error: &str) -> bool {
    let error_lower = error.to_lowercase();
    error_lower.contains("token_expired")
        || error_lower.contains("refresh_token_reused")
        || error_lower.contains("401")
        || error_lower.contains("unauthorized")
        || error_lower.contains("authentication token is expired")
        || error_lower.contains("please log out and sign in again")
        || error_lower.contains("please try signing in again")
        || error_lower.contains("failed to refresh token")
        || error_lower.contains("access token could not be refreshed")
}

/// Format an Agent error for display to the user
/// Returns (formatted_message, error_type) where error_type is "terminated", "auth_expired", or "error"
fn format_agent_error(error: &str) -> (String, &'static str) {
    if is_auth_error(error) {
        (
            "Authentication expired. Please run 'codex login' in your terminal or sign in again from Settings.".to_string(),
            "auth_expired"
        )
    } else if is_recoverable_exit(error) {
        (
            "Agent session was terminated. This may happen due to timeout, permission denial, or system interruption.".to_string(),
            "terminated"
        )
    } else if error.contains("exited with code") {
        (
            format!("Agent process ended unexpectedly. {}", error),
            "error",
        )
    } else {
        (error.to_string(), "error")
    }
}

/// Tracks whether an agent is available for use
#[derive(Debug, Clone, Serialize)]
struct AgentAvailability {
    available: bool,
    error_message: Option<String>,
    last_checked: i64,
}

fn default_search_paths() -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Some(env_paths) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&env_paths));
    }
    if let Some(home) = dirs::home_dir() {
        paths.extend([
            home.join(".amp/bin"),
            home.join(".opencode/bin"),
            home.join(".superset/bin"),
            home.join(".factory/bin"),
            home.join(".npm-global/bin"),
            home.join(".local/bin"),
            home.join(".cargo/bin"),
            home.join("bin"),
        ]);
    }
    if cfg!(target_os = "macos") {
        paths.extend(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/bin",
                "/usr/sbin",
                "/sbin",
            ]
            .iter()
            .map(std::path::PathBuf::from),
        );
    } else if cfg!(target_os = "linux") {
        paths.extend(
            ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"]
                .iter()
                .map(std::path::PathBuf::from),
        );
    }
    paths
}

fn resolve_command_path(command: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() || command.contains('/') || command.contains('\\') {
        return path.is_file().then_some(path.to_path_buf());
    }
    for dir in default_search_paths() {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            for ext in [".exe", ".cmd", ".bat"] {
                if candidate
                    .with_extension(ext.trim_start_matches('.'))
                    .is_file()
                {
                    return Some(candidate.with_extension(ext.trim_start_matches('.')));
                }
            }
        }
    }
    None
}

fn command_exists(command: &str) -> bool {
    resolve_command_path(command).is_some()
}

fn build_agent_availability(config: &AgentsConfig) -> HashMap<String, AgentAvailability> {
    let now = chrono::Utc::now().timestamp();
    let mut map = HashMap::new();
    for agent in &config.agents {
        let command = resolve_agent_command(agent);
        let available = command_exists(&command);
        let error_message = if available {
            None
        } else {
            Some(format!(
                "{} CLI not found. Install {} to enable this agent.",
                agent.command, agent.command
            ))
        };
        if available {
            tracing::info!(
                agent_id = %agent.id,
                command = %command,
                "Agent CLI detected"
            );
        } else {
            let search_paths = default_search_paths();
            let preview_paths: Vec<String> = search_paths
                .iter()
                .take(8)
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            tracing::warn!(
                agent_id = %agent.id,
                command = %command,
                searched_paths = %preview_paths.join(":"),
                total_search_paths = search_paths.len(),
                "Agent CLI missing"
            );
        }
        map.insert(
            agent.id.clone(),
            AgentAvailability {
                available,
                error_message,
                last_checked: now,
            },
        );
    }
    map
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) config: AgentsConfig,
    sessions: Arc<Mutex<HashMap<String, SharedSessionHandle>>>,
    pub(crate) settings: Arc<Mutex<Settings>>,
    pub(crate) db: Arc<StdMutex<rusqlite::Connection>>,
    notification_windows: Arc<StdMutex<Vec<String>>>,
    agent_availability: Arc<StdMutex<HashMap<String, AgentAvailability>>>,
    // Prevent accidental duplicate starts (e.g., user rapid-clicking Start)
    running_tasks: Arc<Mutex<HashSet<String>>>,
    discord_bot: Arc<StdMutex<Option<discord_bot::DiscordBotHandle>>>,
    pending_user_inputs: Arc<Mutex<HashMap<String, PendingUserInput>>>,
    pending_discord_tasks: Arc<Mutex<HashMap<String, PendingDiscordTask>>>,
    codex_command_cache: Arc<StdMutex<HashMap<String, Vec<AvailableCommand>>>>,
    claude_command_cache: Arc<StdMutex<HashMap<String, Vec<AvailableCommand>>>>,
    terminal_sessions: Arc<Mutex<HashMap<String, TerminalSession>>>,
    task_terminal_sessions: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct AgentsConfig {
    #[allow(dead_code)]
    version: Option<u32>,
    #[allow(dead_code)]
    max_parallel: Option<u32>,
    #[serde(default)]
    pub(crate) agents: Vec<AgentConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct AgentConfig {
    pub(crate) id: String,
    #[allow(dead_code)]
    pub(crate) display_name: Option<String>,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    required_env: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) supports_plan: Option<bool>,
    #[serde(default)]
    default_plan_model: Option<String>,
    #[serde(default)]
    default_exec_model: Option<String>,
    #[serde(default)]
    pub(crate) model_source: Option<String>,
    #[serde(default)]
    pub(crate) models: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AttachmentRef {
    id: String,
    #[serde(rename = "relativePath")]
    relative_path: String,
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
}

struct TerminalSession {
    _id: String,
    _task_id: String,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

#[derive(Debug, Serialize)]
struct TerminalSessionInfo {
    session_id: String,
    cwd: String,
}

#[derive(Debug, Clone, Serialize)]
struct TerminalOutputPayload {
    session_id: String,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
struct TerminalExitPayload {
    session_id: String,
}

#[derive(Debug, Serialize)]
struct CodeReviewContext {
    current_branch: String,
    base_branch: String,
    diff: String,
    commit_log: String,
    diff_truncated: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateAgentPayload {
    #[serde(rename = "agentId")]
    pub(crate) agent_id: String,
    pub(crate) prompt: String,
    #[serde(rename = "projectPath")]
    pub(crate) project_path: Option<String>,
    /// Base branch for worktree creation (optional)
    #[serde(rename = "baseBranch", default)]
    pub(crate) base_branch: Option<String>,
    #[serde(rename = "planMode")]
    pub(crate) plan_mode: bool,
    pub(crate) thinking: bool,
    #[serde(rename = "useWorktree")]
    pub(crate) use_worktree: bool,
    #[serde(rename = "permissionMode")]
    pub(crate) permission_mode: String,
    #[serde(rename = "execModel")]
    pub(crate) exec_model: String,
    /// Reasoning effort level for Codex models (low, medium, high)
    #[serde(rename = "reasoningEffort", default)]
    pub(crate) reasoning_effort: Option<String>,
    /// Agent mode for agents that expose modes over ACP
    #[serde(rename = "agentMode", default)]
    pub(crate) agent_mode: Option<String>,
    /// Codex mode (default, plan, pair-programming, execute)
    #[serde(rename = "codexMode", default)]
    pub(crate) codex_mode: Option<String>,
    /// True when creating multiple agent sessions in one action
    #[serde(rename = "multiCreate", default)]
    pub(crate) multi_create: bool,
    #[serde(default)]
    pub(crate) attachments: Vec<AttachmentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Settings {
    #[serde(rename = "Webhook")]
    webhook: Option<String>,
    #[serde(rename = "discordEnabled")]
    discord_enabled: Option<bool>,
    #[serde(rename = "discordBotToken")]
    discord_bot_token: Option<String>,
    #[serde(rename = "discordChannelId")]
    discord_channel_id: Option<String>,
    #[serde(rename = "retryDelay")]
    retry_delay: Option<String>,
    #[serde(rename = "errorDelay")]
    error_delay: Option<String>,
    #[serde(rename = "ignoreDeclines")]
    ignore_declines: Option<bool>,
    #[serde(rename = "openaiApiKey")]
    openai_api_key: Option<String>,
    #[serde(rename = "anthropicApiKey")]
    anthropic_api_key: Option<String>,
    #[serde(rename = "codexAuthMethod")]
    codex_auth_method: Option<String>,
    #[serde(rename = "claudeAuthMethod")]
    claude_auth_method: Option<String>,
    #[serde(rename = "agentNotificationsEnabled")]
    agent_notifications_enabled: Option<bool>,
    #[serde(rename = "agentNotificationStack")]
    agent_notification_stack: Option<bool>,
    #[serde(rename = "agentNotificationTimeout")]
    agent_notification_timeout: Option<u32>,
    // AI-powered summarization for task titles and status
    #[serde(rename = "aiSummariesEnabled")]
    ai_summaries_enabled: Option<bool>,
    // Dedicated agent for summaries (overrides task agent for titles, status, branch names)
    // Values: "auto" (use task agent), "amp", "codex", "claude-code"
    #[serde(rename = "summariesAgent")]
    summaries_agent: Option<String>,
    // Task creation settings (sticky between restarts)
    #[serde(rename = "taskProjectPath")]
    pub(crate) task_project_path: Option<String>,
    #[serde(rename = "taskProjectAllowlist", default)]
    pub(crate) task_project_allowlist: Option<Vec<String>>,
    #[serde(rename = "taskPlanMode")]
    pub(crate) task_plan_mode: Option<bool>,
    #[serde(rename = "taskThinking")]
    pub(crate) task_thinking: Option<bool>,
    #[serde(rename = "taskUseWorktree")]
    pub(crate) task_use_worktree: Option<bool>,
    #[serde(rename = "taskBaseBranch")]
    pub(crate) task_base_branch: Option<String>,
    #[serde(rename = "taskLastAgent")]
    pub(crate) task_last_agent: Option<String>,
    // Per-agent task selections stored as JSON object
    #[serde(rename = "taskAgentModels", default)]
    task_agent_models: Option<std::collections::HashMap<String, AgentModelPrefs>>,
    // MCP server settings
    #[serde(rename = "mcpEnabled")]
    pub(crate) mcp_enabled: Option<bool>,
    #[serde(rename = "mcpPort")]
    pub(crate) mcp_port: Option<u16>,
    #[serde(rename = "mcpToken")]
    pub(crate) mcp_token: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingUserInput {
    request_id: String,
    questions: Vec<UserInputQuestion>,
    answers: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingDiscordTask {
    prompt: String,
    requester_id: u64,
    channel_id: u64,
    project_path: Option<String>,
    agent_id: Option<String>,
    model: Option<String>,
    created_at: i64,
    ephemeral: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageOrigin {
    Ui,
    Discord,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AgentModelPrefs {
    #[serde(rename = "planModel")]
    plan_model: Option<String>,
    #[serde(rename = "execModel")]
    exec_model: Option<String>,
    #[serde(rename = "permissionMode")]
    permission_mode: Option<String>,
    /// Reasoning effort level for Codex models (low, medium, high)
    #[serde(rename = "reasoningEffort")]
    reasoning_effort: Option<String>,
    /// Agent mode for agents that expose modes over ACP
    #[serde(rename = "agentMode")]
    agent_mode: Option<String>,
}

struct SessionHandle {
    agent_id: String,
    session_id: String,
    model: String,
    client: Arc<AgentProcessClient>,
    pending_prompt: Option<String>,
    pending_attachments: Vec<AttachmentRef>,
    messages: Vec<serde_json::Value>,
    /// Real-time cost watcher for Claude Code sessions (None for other agents)
    #[allow(dead_code)] // Kept for future graceful shutdown
    claude_watcher: Option<claude_usage_watcher::WatcherHandle>,
    /// Token to signal cancellation of current generation without killing the session
    cancel_token: CancellationToken,
}

type SharedSessionHandle = Arc<Mutex<SessionHandle>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexAuthStatus {
    authenticated: bool,
    method: Option<String>,
    expires_at: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClaudeAuthStatus {
    authenticated: bool,
    method: Option<String>,
    expires_at: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitWindow {
    used_percent: f64,
    window_duration_mins: i32,
    resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimits {
    primary: Option<RateLimitWindow>,
    secondary: Option<RateLimitWindow>,
    plan_type: Option<String>,
    #[serde(rename = "notAvailable")]
    not_available: Option<bool>,
    #[serde(rename = "errorMessage")]
    error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoBranches {
    branches: Vec<String>,
    default_branch: Option<String>,
    current_branch: Option<String>,
    source: String,
    error: Option<String>,
}

/// Git state for PR creation readiness
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrReadyState {
    current_branch: Option<String>,
    base_branch: Option<String>,
    uncommitted_changes: i32,
    has_upstream: bool,
    ahead_count: i32,
    behind_count: i32,
    pr_template: Option<String>,
    error: Option<String>,
}

/// Information about an existing pull request
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExistingPr {
    number: u32,
    url: String,
    title: String,
    state: String, // "OPEN", "CLOSED", "MERGED"
}

/// Result of checking for an existing PR
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrCheckResult {
    pr: Option<ExistingPr>,
    error: Option<String>,
}

/// Extract email from a JWT access token (base64 decode the payload)
fn extract_email_from_jwt(token: &str) -> Option<String> {
    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    // Decode the payload (second part) - use URL-safe base64
    let payload = parts[1];
    // Add padding if needed
    let padded = match payload.len() % 4 {
        2 => format!("{}==", payload),
        3 => format!("{}=", payload),
        _ => payload.to_string(),
    };

    // Replace URL-safe characters
    let standard = padded.replace('-', "+").replace('_', "/");

    let decoded = match base64_decode(&standard) {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };

    let json_str = match String::from_utf8(decoded) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let payload: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(_) => return None,
    };

    // Email is at https://api.openai.com/profile.email
    payload
        .get("https://api.openai.com/profile")
        .and_then(|p| p.get("email"))
        .and_then(|e| e.as_str())
        .map(String::from)
}

/// Simple base64 decode without external crate
fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits_collected = 0;

    for c in input.bytes() {
        if c == b'=' {
            break;
        }
        let value = match ALPHABET.iter().position(|&x| x == c) {
            Some(v) => v as u32,
            None => return Err("invalid character"),
        };
        buffer = (buffer << 6) | value;
        bits_collected += 6;
        if bits_collected >= 8 {
            bits_collected -= 8;
            output.push((buffer >> bits_collected) as u8);
            buffer &= (1 << bits_collected) - 1;
        }
    }
    Ok(output)
}

fn load_agents_config(config_path: &Path) -> anyhow::Result<AgentsConfig> {
    let raw = std::fs::read_to_string(config_path)?;
    let config: AgentsConfig = toml::from_str(&raw)?;
    Ok(config)
}

fn config_path() -> PathBuf {
    // In debug builds, use the source tree path for hot reload during development
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("backend")
            .join("config")
            .join("agents.toml")
    }

    // In release builds, use the bundled resource path
    #[cfg(not(debug_assertions))]
    {
        // On macOS: AppName.app/Contents/Resources/agents.toml
        // On Linux/Windows: next to the executable
        std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent().map(|dir| {
                    // macOS bundle structure: .app/Contents/MacOS/binary
                    // Resources are at: .app/Contents/Resources/
                    if cfg!(target_os = "macos") {
                        let direct = dir.join("../Resources/agents.toml");
                        if direct.exists() {
                            return direct;
                        }
                        let nested = dir.join("../Resources/backend/config/agents.toml");
                        if nested.exists() {
                            return nested;
                        }
                        let updater_path = dir.join("../Resources/_up_/backend/config/agents.toml");
                        if updater_path.exists() {
                            return updater_path;
                        }
                        dir.join("../Resources/agents.toml")
                    } else {
                        let direct = dir.join("agents.toml");
                        if direct.exists() {
                            return direct;
                        }
                        let nested = dir.join("backend/config/agents.toml");
                        if nested.exists() {
                            return nested;
                        }
                        direct
                    }
                })
            })
            .unwrap_or_else(|| PathBuf::from("agents.toml"))
    }
}

fn settings_path() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| "config dir unavailable".to_string())?;
    let dir = base.join("phantom-harness");
    std::fs::create_dir_all(&dir).map_err(|err| format!("settings dir: {}", err))?;
    Ok(dir.join("settings.json"))
}

const DEFAULT_MCP_PORT: u16 = 43778;

fn generate_mcp_token() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let mut bytes = [0u8; 32];
    let mut rng = rand::rngs::OsRng;
    rng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn ensure_mcp_settings(settings: &mut Settings) -> bool {
    let mut changed = false;
    if settings.mcp_enabled.is_none() {
        settings.mcp_enabled = Some(true);
        changed = true;
    }
    if settings
        .mcp_token
        .as_ref()
        .map(|token| token.trim().is_empty())
        .unwrap_or(true)
    {
        settings.mcp_token = Some(generate_mcp_token());
        changed = true;
    }
    if settings.mcp_port.is_none() {
        settings.mcp_port = Some(DEFAULT_MCP_PORT);
        changed = true;
    }
    changed
}

fn mcp_enabled(settings: &Settings) -> bool {
    settings.mcp_enabled.unwrap_or(true)
}

fn db_path() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| "config dir unavailable".to_string())?;
    let dir = base.join("phantom-harness");
    std::fs::create_dir_all(&dir).map_err(|err| format!("db dir: {}", err))?;
    Ok(dir.join("tasks.db"))
}

fn attachments_dir() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| "config dir unavailable".to_string())?;
    let dir = base.join("phantom-harness").join("attachments");
    std::fs::create_dir_all(&dir).map_err(|err| format!("attachments dir: {}", err))?;
    Ok(dir)
}

fn load_settings_from_disk() -> Settings {
    let path = match settings_path() {
        Ok(path) => path,
        Err(_) => return Settings::default(),
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return Settings::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub(crate) fn persist_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path()?;
    let payload = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("serialize settings: {}", err))?;
    std::fs::write(&path, payload).map_err(|err| format!("write settings: {}", err))?;
    Ok(())
}

fn discord_enabled(settings: &Settings) -> bool {
    settings.discord_enabled.unwrap_or(false)
}

async fn stop_discord_bot(state: &AppState) {
    let handle = {
        let mut guard = match state.discord_bot.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.take()
    };
    if let Some(handle) = handle {
        handle.shutdown().await;
    }
}

async fn ensure_discord_bot(app: &AppHandle, state: &AppState, settings: &Settings) {
    if !discord_enabled(settings) {
        stop_discord_bot(state).await;
        return;
    }

    let should_start = {
        let guard = match state.discord_bot.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.is_none()
    };
    if !should_start {
        return;
    }

    match discord_bot::start_discord_bot(app.clone(), settings).await {
        Ok(handle) => {
            if let Ok(mut guard) = state.discord_bot.lock() {
                *guard = Some(handle);
            }
            println!("[Discord] Bot started");
        }
        Err(err) => {
            println!("[Discord] Failed to start bot: {err}");
        }
    }
}

fn discord_handle(state: &AppState) -> Option<discord_bot::DiscordBotHandle> {
    let guard = state.discord_bot.lock().ok()?;
    guard.clone()
}

fn find_agent<'a>(config: &'a AgentsConfig, agent_id: &str) -> Option<&'a AgentConfig> {
    config.agents.iter().find(|agent| agent.id == agent_id)
}

fn auth_env_for(agent_id: &str, settings: &Settings) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(value) = settings.openai_api_key.as_ref() {
        env.push(("OPENAI_API_KEY".to_string(), value.clone()));
    }
    if let Some(value) = settings.anthropic_api_key.as_ref() {
        env.push(("ANTHROPIC_API_KEY".to_string(), value.clone()));
    }
    if agent_id != "codex" {
        env.retain(|(key, _)| key != "OPENAI_API_KEY");
    }
    if agent_id != "claude-code" {
        env.retain(|(key, _)| key != "ANTHROPIC_API_KEY");
    }
    env
}

fn default_path_entries() -> Vec<String> {
    let mut entries = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        entries.push(format!("{home}/.npm-global/bin"));
    }
    entries.extend(
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ]
        .iter()
        .map(|value| value.to_string()),
    );
    entries
}

fn ensure_path_env(mut env: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut path_value = std::env::var("PATH").unwrap_or_default();
    for entry in default_path_entries() {
        if !path_value.split(':').any(|existing| existing == entry) {
            if !path_value.is_empty() {
                path_value.push(':');
            }
            path_value.push_str(&entry);
        }
    }

    if let Some((_, value)) = env.iter_mut().find(|(key, _)| key == "PATH") {
        if !value.is_empty() {
            value.push(':');
        }
        value.push_str(&path_value);
    } else if !path_value.is_empty() {
        env.push(("PATH".to_string(), path_value));
    }

    env
}

fn build_env(
    required: &[String],
    overrides: &[(String, String)],
    allow_missing: bool,
) -> Result<Vec<(String, String)>, String> {
    let mut env = Vec::new();
    let mut missing = Vec::new();
    for key in required {
        if let Some((_, value)) = overrides.iter().find(|(k, _)| k == key) {
            env.push((key.clone(), value.clone()));
            continue;
        }
        match std::env::var(key) {
            Ok(value) => env.push((key.clone(), value)),
            Err(_) => missing.push(key.clone()),
        }
    }
    for (key, value) in overrides {
        if !env.iter().any(|(k, _)| k == key) {
            env.push((key.clone(), value.clone()));
        }
    }
    if !missing.is_empty() && !allow_missing {
        return Err(format!("Missing env: {}", missing.join(", ")));
    }
    Ok(ensure_path_env(env))
}

fn substitute_args(args: &[String], cwd: &str) -> Vec<String> {
    args.iter()
        .map(|arg| arg.replace("{worktree}", cwd))
        .collect()
}

fn resolve_project_path(project_path: &Option<String>) -> Result<PathBuf, String> {
    if let Some(path) = project_path.as_ref() {
        if path.trim().is_empty() {
            std::env::current_dir().map_err(|err| format!("cwd error: {}", err))
        } else {
            Ok(PathBuf::from(path))
        }
    } else {
        std::env::current_dir().map_err(|err| format!("cwd error: {}", err))
    }
}

fn normalize_allowlist_path(path: &str) -> PathBuf {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return PathBuf::new();
    }
    std::fs::canonicalize(trimmed).unwrap_or_else(|_| PathBuf::from(trimmed))
}

pub(crate) fn project_path_allowed(allowlist: &[String], project_path: &str) -> bool {
    let project_path = normalize_allowlist_path(project_path);
    allowlist.iter().any(|entry| {
        let entry = entry.trim();
        if entry.is_empty() {
            return false;
        }
        let allowed_path = normalize_allowlist_path(entry);
        if allowed_path.as_os_str().is_empty() {
            return false;
        }
        project_path.starts_with(&allowed_path)
    })
}

fn resolve_task_cwd(task: &db::TaskRecord) -> Result<PathBuf, String> {
    if let Some(path) = task.worktree_path.as_ref() {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    if let Some(path) = task.project_path.as_ref() {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    std::env::current_dir().map_err(|err| format!("cwd error: {}", err))
}

pub(crate) async fn resolve_repo_root(path: &Path) -> Option<PathBuf> {
    let repo_path = path.to_path_buf();
    match worktree::run_git_command(&repo_path, &["rev-parse", "--show-toplevel"]).await {
        Ok(output) if !output.trim().is_empty() => Some(PathBuf::from(output.trim())),
        _ => None,
    }
}

fn codex_builtin_commands() -> Vec<AvailableCommand> {
    let scope = Some("global".to_string());
    let commands = [
        ("/model", "Choose model and reasoning effort"),
        ("/approvals", "Configure approval policy"),
        ("/setup-elevated-sandbox", "Set up elevated agent sandbox"),
        ("/experimental", "Toggle beta features"),
        ("/skills", "Use skills to improve task performance"),
        ("/review", "Review current changes and find issues"),
        ("/new", "Start a new chat during a conversation"),
        ("/resume", "Resume a saved chat"),
        ("/fork", "Fork the current chat"),
        ("/init", "Create an AGENTS.md file"),
        (
            "/compact",
            "Summarize conversation to prevent context limit",
        ),
        ("/collab", "Change collaboration mode"),
        ("/diff", "Show git diff (including untracked files)"),
        ("/mention", "Mention a file"),
        ("/status", "Show session configuration and token usage"),
        ("/mcp", "List configured MCP tools"),
        ("/logout", "Log out of Codex"),
        ("/quit", "Exit Codex"),
        ("/exit", "Exit Codex"),
        ("/feedback", "Send logs to maintainers"),
        ("/ps", "List background terminals"),
    ];

    commands
        .iter()
        .map(|(name, description)| AvailableCommand {
            name: (*name).to_string(),
            description: (*description).to_string(),
            scope: scope.clone(),
        })
        .collect()
}

fn first_non_empty_line<'a, I>(lines: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cleaned = trimmed.trim_start_matches('#').trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    None
}

fn parse_codex_prompt_description(contents: &str) -> Option<String> {
    let mut lines = contents.lines();

    let first = lines.next()?;
    if first.trim() == "---" {
        let mut description: Option<String> = None;
        for line in &mut lines {
            let trimmed = line.trim();
            if trimmed == "---" {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("description:") {
                let value = rest.trim().trim_matches('"').trim_matches('\'');
                if !value.is_empty() {
                    description = Some(value.to_string());
                }
            }
        }
        if description.is_some() {
            return description;
        }
        return first_non_empty_line(lines);
    }

    first_non_empty_line(std::iter::once(first).chain(lines))
}

fn read_codex_prompt_commands(
    prompt_dir: &Path,
    scope: &str,
) -> Result<Vec<AvailableCommand>, std::io::Error> {
    if !prompt_dir.exists() {
        return Ok(Vec::new());
    }

    let mut commands = Vec::new();
    for entry in std::fs::read_dir(prompt_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }

        let description = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| parse_codex_prompt_description(&content))
            .unwrap_or_default();

        commands.push(AvailableCommand {
            name: format!("/prompts:{}", name),
            description,
            scope: Some(scope.to_string()),
        });
    }

    commands.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(commands)
}

fn collect_codex_commands(state: &AppState, project_root: &Path) -> Vec<AvailableCommand> {
    let cache_key = project_root.to_string_lossy().to_string();
    let mut commands = codex_builtin_commands();
    let mut had_error = false;

    if let Some(home) = dirs::home_dir() {
        let user_dir = home.join(".codex").join("prompts");
        match read_codex_prompt_commands(&user_dir, "user") {
            Ok(mut user_commands) => commands.append(&mut user_commands),
            Err(err) => {
                eprintln!("[Harness] Failed to read Codex user prompts: {}", err);
                had_error = true;
            }
        }
    }

    let project_dir = project_root.join(".codex").join("prompts");
    match read_codex_prompt_commands(&project_dir, "project") {
        Ok(mut project_commands) => commands.append(&mut project_commands),
        Err(err) => {
            eprintln!("[Harness] Failed to read Codex project prompts: {}", err);
            had_error = true;
        }
    }

    if !had_error {
        if let Ok(mut cache) = state.codex_command_cache.lock() {
            cache.insert(cache_key, commands.clone());
        }
        return commands;
    }

    if let Ok(cache) = state.codex_command_cache.lock() {
        if let Some(cached) = cache.get(&cache_key) {
            return cached.clone();
        }
    }

    commands
}

fn claude_builtin_commands() -> Vec<AvailableCommand> {
    let scope = Some("global".to_string());
    let commands = [
        ("/help", "Show available commands"),
        ("/status", "Show session status"),
        ("/clear", "Clear conversation"),
        ("/compact", "Clear history, keeping summary"),
        ("/config", "View/modify configuration"),
        ("/cost", "Show session costs"),
        ("/doctor", "Diagnose issues"),
        ("/init", "Initialize CLAUDE.md"),
        ("/login", "Sign in to Claude"),
        ("/logout", "Sign out of Claude"),
        ("/model", "Switch or view model"),
        ("/resume", "Resume previous session"),
        ("/add-dir", "Add directory to context"),
        ("/context", "Show/modify context window"),
        ("/memory", "View/edit persistent memory"),
        ("/permissions", "Manage tool permissions"),
        ("/bug", "Report a bug"),
        ("/diff", "Show pending changes"),
        ("/mcp", "MCP server commands"),
        ("/review", "Review code changes"),
        ("/terminal", "Run terminal command"),
        ("/vim", "Toggle vim mode"),
        ("/web", "Search the web"),
    ];

    commands
        .iter()
        .map(|(name, description)| AvailableCommand {
            name: (*name).to_string(),
            description: (*description).to_string(),
            scope: scope.clone(),
        })
        .collect()
}

fn parse_claude_command_metadata(contents: &str) -> (Option<String>, Option<String>) {
    let mut lines = contents.lines();
    let first = match lines.next() {
        Some(line) => line,
        None => return (None, None),
    };

    if first.trim() != "---" {
        let description = first_non_empty_line(std::iter::once(first).chain(lines));
        return (None, description);
    }

    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("name:") {
            let value = rest.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                name = Some(value.to_string());
            }
        }
        if let Some(rest) = trimmed.strip_prefix("command:") {
            let value = rest.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() && name.is_none() {
                name = Some(value.to_string());
            }
        }
        if let Some(rest) = trimmed.strip_prefix("description:") {
            let value = rest.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                description = Some(value.to_string());
            }
        }
    }

    let description = description.or_else(|| first_non_empty_line(lines));
    (name, description)
}

fn command_name_from_path(path: &Path, root: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut parts: Vec<String> = Vec::new();
    for component in relative.components() {
        if let std::path::Component::Normal(value) = component {
            let mut part = value.to_string_lossy().to_string();
            if part.ends_with(".md") {
                part = part.trim_end_matches(".md").to_string();
            }
            if !part.is_empty() {
                parts.push(part);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(":"))
    }
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        files.push(path);
    }

    Ok(())
}

fn read_claude_command_files(
    command_dir: &Path,
    scope: &str,
) -> Result<Vec<AvailableCommand>, std::io::Error> {
    if !command_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_markdown_files(command_dir, &mut files)?;

    let mut commands = Vec::new();
    for path in files {
        let contents = std::fs::read_to_string(&path).ok();
        let (frontmatter_name, description) = contents
            .as_deref()
            .map(parse_claude_command_metadata)
            .unwrap_or((None, None));
        let fallback_name = command_name_from_path(&path, command_dir);
        let raw_name = frontmatter_name.or(fallback_name).unwrap_or_default();
        let raw_name = raw_name.trim();
        if raw_name.is_empty() {
            continue;
        }
        let name = if raw_name.starts_with('/') {
            raw_name.to_string()
        } else {
            format!("/{}", raw_name)
        };

        commands.push(AvailableCommand {
            name,
            description: description.unwrap_or_default(),
            scope: Some(scope.to_string()),
        });
    }

    commands.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(commands)
}

#[derive(Debug, Deserialize)]
struct ClaudeInstalledPlugins {
    plugins: HashMap<String, Vec<ClaudePluginInstall>>,
}

#[derive(Debug, Deserialize)]
struct ClaudePluginInstall {
    scope: Option<String>,
    #[serde(rename = "installPath")]
    install_path: String,
}

fn read_claude_plugin_commands(
    plugins_dir: &Path,
) -> Result<Vec<AvailableCommand>, std::io::Error> {
    let installed_path = plugins_dir.join("installed_plugins.json");
    if !installed_path.exists() {
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(&installed_path)?;
    let installed: ClaudeInstalledPlugins = serde_json::from_str(&contents)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

    let mut commands = Vec::new();
    for installs in installed.plugins.values() {
        for install in installs {
            let scope = install.scope.clone().unwrap_or_else(|| "user".to_string());
            let install_path = PathBuf::from(&install.install_path);
            let command_dirs = [
                install_path.join("commands"),
                install_path.join(".claude").join("commands"),
            ];
            for dir in command_dirs {
                match read_claude_command_files(&dir, &scope) {
                    Ok(mut plugin_commands) => commands.append(&mut plugin_commands),
                    Err(err) => eprintln!(
                        "[Harness] Failed to read Claude plugin commands in {}: {}",
                        dir.display(),
                        err
                    ),
                }
            }
        }
    }

    commands.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(commands)
}

fn dedupe_commands(commands: Vec<AvailableCommand>) -> Vec<AvailableCommand> {
    let mut map: HashMap<String, AvailableCommand> = HashMap::new();
    for command in commands {
        map.insert(command.name.clone(), command);
    }
    let mut deduped: Vec<AvailableCommand> = map.into_values().collect();
    deduped.sort_by(|a, b| a.name.cmp(&b.name));
    deduped
}

fn collect_claude_commands(state: &AppState, project_root: &Path) -> Vec<AvailableCommand> {
    let cache_key = project_root.to_string_lossy().to_string();
    let mut commands = claude_builtin_commands();
    let mut had_error = false;

    if let Some(home) = dirs::home_dir() {
        let plugins_dir = home.join(".claude").join("plugins");
        match read_claude_plugin_commands(&plugins_dir) {
            Ok(mut plugin_commands) => commands.append(&mut plugin_commands),
            Err(err) => {
                eprintln!("[Harness] Failed to read Claude plugins: {}", err);
                had_error = true;
            }
        }

        let user_dir = home.join(".claude").join("commands");
        match read_claude_command_files(&user_dir, "user") {
            Ok(mut user_commands) => commands.append(&mut user_commands),
            Err(err) => {
                eprintln!("[Harness] Failed to read Claude user commands: {}", err);
                had_error = true;
            }
        }
    }

    let project_dir = project_root.join(".claude").join("commands");
    match read_claude_command_files(&project_dir, "project") {
        Ok(mut project_commands) => commands.append(&mut project_commands),
        Err(err) => {
            eprintln!("[Harness] Failed to read Claude project commands: {}", err);
            had_error = true;
        }
    }

    let commands = dedupe_commands(commands);

    if !had_error {
        if let Ok(mut cache) = state.claude_command_cache.lock() {
            cache.insert(cache_key, commands.clone());
        }
        return commands;
    }

    if let Ok(cache) = state.claude_command_cache.lock() {
        if let Some(cached) = cache.get(&cache_key) {
            return cached.clone();
        }
    }

    commands
}

fn emit_available_commands(
    app: &AppHandle,
    task_id: &str,
    agent_id: &str,
    commands: &[AvailableCommand],
) {
    if let Some(main_window) = app.get_webview_window("main") {
        let _ = main_window.emit("AvailableCommands", (task_id, agent_id, commands));
    }
    if let Some(chat_window) = app.get_webview_window(&format!("chat-{}", task_id)) {
        let _ = chat_window.emit("AvailableCommands", (task_id, agent_id, commands));
    }
}

fn parse_github_repo(remote_url: &str) -> Option<(String, String)> {
    let trimmed = remote_url.trim().trim_end_matches(".git");
    let rest = if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("ssh://git@github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("git://github.com/") {
        rest
    } else {
        return None;
    };

    let mut parts = rest.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

async fn run_gh_command(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let gh_path = resolve_gh_binary()?;
    let output = TokioCommand::new(&gh_path)
        .args(args)
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|e| format!("Failed to execute gh: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("gh command failed: {}", stderr))
    }
}

async fn get_repo_branches_via_gh(
    repo_root: &Path,
    owner: &str,
    repo: &str,
) -> Result<RepoBranches, String> {
    let current_branch = worktree::current_branch(&repo_root.to_path_buf())
        .await
        .ok();
    let default_branch = run_gh_command(
        repo_root,
        &[
            "api",
            &format!("repos/{}/{}", owner, repo),
            "--jq",
            ".default_branch",
        ],
    )
    .await
    .ok()
    .filter(|value| !value.trim().is_empty());

    let branches_output = run_gh_command(
        repo_root,
        &[
            "api",
            &format!("repos/{}/{}/branches", owner, repo),
            "--paginate",
            "--jq",
            ".[].name",
        ],
    )
    .await?;

    let mut branches: Vec<String> = branches_output
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    if let Some(ref default_name) = default_branch {
        if !branches.iter().any(|b| b == default_name) {
            branches.insert(0, default_name.clone());
        }
    }

    Ok(RepoBranches {
        branches,
        default_branch,
        current_branch,
        source: "gh".to_string(),
        error: None,
    })
}

#[tauri::command]
async fn get_repo_branches(project_path: Option<String>) -> Result<RepoBranches, String> {
    let cwd = resolve_project_path(&project_path)?;
    let repo_root = match resolve_repo_root(&cwd).await {
        Some(root) => root,
        None => {
            return Ok(RepoBranches {
                branches: Vec::new(),
                default_branch: None,
                current_branch: None,
                source: "none".to_string(),
                error: Some("Not a git repository".to_string()),
            });
        }
    };

    let current_branch = worktree::current_branch(&repo_root)
        .await
        .ok()
        .filter(|value| !value.trim().is_empty());

    let origin_url = worktree::run_git_command(&repo_root, &["remote", "get-url", "origin"])
        .await
        .ok();

    if let Some(url) = origin_url {
        if let Some((owner, repo)) = parse_github_repo(&url) {
            if let Ok(result) = get_repo_branches_via_gh(&repo_root, &owner, &repo).await {
                return Ok(result);
            }
        }
    }

    let branches = worktree::list_branches(&repo_root)
        .await
        .unwrap_or_default();
    Ok(RepoBranches {
        branches,
        default_branch: None,
        current_branch,
        source: "git".to_string(),
        error: None,
    })
}

/// Get git state for PR creation readiness
#[tauri::command]
async fn get_pr_ready_state(project_path: Option<String>) -> Result<PrReadyState, String> {
    let cwd = resolve_project_path(&project_path)?;
    let repo_root = match resolve_repo_root(&cwd).await {
        Some(root) => root,
        None => {
            return Ok(PrReadyState {
                current_branch: None,
                base_branch: None,
                uncommitted_changes: 0,
                has_upstream: false,
                ahead_count: 0,
                behind_count: 0,
                pr_template: None,
                error: Some("Not a git repository".to_string()),
            });
        }
    };

    // Get current branch
    let current_branch = worktree::current_branch(&repo_root)
        .await
        .ok()
        .filter(|v| !v.trim().is_empty());

    // Get default branch via GitHub API if possible
    let base_branch = if let Ok(origin_url) =
        worktree::run_git_command(&repo_root, &["remote", "get-url", "origin"]).await
    {
        if let Some((owner, repo)) = parse_github_repo(&origin_url) {
            get_repo_branches_via_gh(&repo_root, &owner, &repo)
                .await
                .ok()
                .and_then(|r| r.default_branch)
        } else {
            None
        }
    } else {
        None
    };

    // Count uncommitted changes (staged + unstaged + untracked)
    let uncommitted_changes =
        match worktree::run_git_command(&repo_root, &["status", "--porcelain"]).await {
            Ok(output) => output.lines().filter(|l| !l.is_empty()).count() as i32,
            Err(_) => 0,
        };

    // Check for upstream tracking branch
    let has_upstream =
        worktree::run_git_command(&repo_root, &["rev-parse", "--abbrev-ref", "@{u}"])
            .await
            .is_ok();

    // Get ahead/behind counts if upstream exists
    let (ahead_count, behind_count) = if has_upstream {
        match worktree::run_git_command(
            &repo_root,
            &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
        )
        .await
        {
            Ok(output) => {
                let parts: Vec<&str> = output.trim().split_whitespace().collect();
                if parts.len() == 2 {
                    let behind = parts[0].parse::<i32>().unwrap_or(0);
                    let ahead = parts[1].parse::<i32>().unwrap_or(0);
                    (ahead, behind)
                } else {
                    (0, 0)
                }
            }
            Err(_) => (0, 0),
        }
    } else {
        (0, 0)
    };

    // Look for PR template
    let pr_template = find_pr_template(&repo_root).await;

    Ok(PrReadyState {
        current_branch,
        base_branch,
        uncommitted_changes,
        has_upstream,
        ahead_count,
        behind_count,
        pr_template,
        error: None,
    })
}

/// Find PR template in common locations
async fn find_pr_template(repo_root: &std::path::Path) -> Option<String> {
    let template_paths = [
        ".github/PULL_REQUEST_TEMPLATE.md",
        ".github/pull_request_template.md",
        "PULL_REQUEST_TEMPLATE.md",
        "pull_request_template.md",
        "docs/PULL_REQUEST_TEMPLATE.md",
    ];

    for template_path in &template_paths {
        let full_path = repo_root.join(template_path);
        if let Ok(content) = tokio::fs::read_to_string(&full_path).await {
            return Some(content);
        }
    }
    None
}

/// Check if a branch has an existing pull request
#[tauri::command]
async fn check_existing_pr(
    project_path: Option<String>,
    branch: String,
) -> Result<PrCheckResult, String> {
    let cwd = resolve_project_path(&project_path)?;
    let repo_root = match resolve_repo_root(&cwd).await {
        Some(root) => root,
        None => {
            return Ok(PrCheckResult {
                pr: None,
                error: Some("Not a git repository".to_string()),
            });
        }
    };

    async fn run_pr_list(repo_root: &Path, head: &str) -> Result<Option<ExistingPr>, String> {
        let gh_path = resolve_gh_binary()?;
        let output = TokioCommand::new(&gh_path)
            .args(&[
                "pr",
                "list",
                "--head",
                head,
                "--json",
                "number,url,title,state",
                "--limit",
                "1",
            ])
            .current_dir(repo_root)
            .output()
            .await
            .map_err(|e| format!("gh not available: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh command failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let prs = serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
            .map_err(|e| format!("Failed to parse gh output: {}", e))?;

        if let Some(pr) = prs.first() {
            let number = pr["number"].as_u64().unwrap_or(0) as u32;
            let url = pr["url"].as_str().unwrap_or("").to_string();
            let title = pr["title"].as_str().unwrap_or("").to_string();
            let state = pr["state"].as_str().unwrap_or("UNKNOWN").to_string();

            Ok(Some(ExistingPr {
                number,
                url,
                title,
                state,
            }))
        } else {
            Ok(None)
        }
    }

    let mut pr = match run_pr_list(&repo_root, &branch).await {
        Ok(pr) => pr,
        Err(error) => {
            return Ok(PrCheckResult {
                pr: None,
                error: Some(error),
            })
        }
    };

    if pr.is_none() {
        let origin_url = worktree::run_git_command(&repo_root, &["remote", "get-url", "origin"])
            .await
            .ok();

        if let Some(url) = origin_url {
            if let Some((owner, _repo)) = parse_github_repo(&url) {
                let head = format!("{}:{}", owner, branch);
                pr = match run_pr_list(&repo_root, &head).await {
                    Ok(found) => found,
                    Err(error) => {
                        return Ok(PrCheckResult {
                            pr: None,
                            error: Some(error),
                        })
                    }
                };
            }
        }
    }

    Ok(PrCheckResult { pr, error: None })
}

/// Get GitHub PR creation URL for a branch
#[tauri::command]
async fn get_github_pr_url(
    project_path: Option<String>,
    current_branch: String,
    base_branch: Option<String>,
) -> Result<Option<String>, String> {
    let cwd = resolve_project_path(&project_path)?;
    let repo_root = match resolve_repo_root(&cwd).await {
        Some(root) => root,
        None => return Ok(None),
    };

    // Get the remote URL
    let origin_url = worktree::run_git_command(&repo_root, &["remote", "get-url", "origin"])
        .await
        .map_err(|e| format!("Failed to get remote URL: {}", e))?;

    // Parse the GitHub owner/repo from the remote URL
    let (owner, repo) = match parse_github_repo(&origin_url) {
        Some((o, r)) => (o, r),
        None => return Ok(None),
    };

    // Build the compare URL for PR creation
    let base = base_branch.unwrap_or_else(|| "main".to_string());
    let url = format!(
        "https://github.com/{}/{}/compare/{}...{}?expand=1",
        owner, repo, base, current_branch
    );

    Ok(Some(url))
}

/// Open an external URL in the default browser
#[tauri::command]
async fn open_external_url(app_handle: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app_handle
        .opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| format!("Failed to open URL: {}", e))
}

/// Format tool call into a human-readable status message for the task list
fn format_tool_status(name: &str, arguments: &str) -> String {
    let tool_name = if name.is_empty() { "tool" } else { name };
    let arguments = if arguments.is_empty() {
        None
    } else {
        Some(arguments)
    };

    // Parse common tools to show friendly status
    match tool_name {
        "read_file" | "Read" => {
            if let Some(args) = arguments {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(args) {
                    if let Some(path) = parsed
                        .get("path")
                        .or_else(|| parsed.get("file_path"))
                        .and_then(|v| v.as_str())
                    {
                        let filename = std::path::Path::new(path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(path);
                        return format!("Reading: {}", truncate_str(filename, 30));
                    }
                }
            }
            "Reading file...".to_string()
        }
        "write_file" | "Write" => {
            if let Some(args) = arguments {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(args) {
                    if let Some(path) = parsed
                        .get("path")
                        .or_else(|| parsed.get("file_path"))
                        .and_then(|v| v.as_str())
                    {
                        let filename = std::path::Path::new(path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(path);
                        return format!("Writing: {}", truncate_str(filename, 30));
                    }
                }
            }
            "Writing file...".to_string()
        }
        "edit_file" | "Edit" | "MultiEdit" => {
            if let Some(args) = arguments {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(args) {
                    if let Some(path) = parsed
                        .get("path")
                        .or_else(|| parsed.get("file_path"))
                        .and_then(|v| v.as_str())
                    {
                        let filename = std::path::Path::new(path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(path);
                        return format!("Editing: {}", truncate_str(filename, 30));
                    }
                }
            }
            "Editing file...".to_string()
        }
        "search" | "Grep" | "Glob" | "ripgrep" => {
            if let Some(args) = arguments {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(args) {
                    if let Some(pattern) = parsed
                        .get("pattern")
                        .or_else(|| parsed.get("query"))
                        .and_then(|v| v.as_str())
                    {
                        return format!("Searching: {}", truncate_str(pattern, 20));
                    }
                }
            }
            "Searching...".to_string()
        }
        "shell" | "Bash" | "bash" | "execute" => {
            if let Some(args) = arguments {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(args) {
                    if let Some(cmd) = parsed
                        .get("command")
                        .or_else(|| parsed.get("cmd"))
                        .and_then(|v| v.as_str())
                    {
                        // Extract just the command name (first word)
                        let cmd_name = cmd.split_whitespace().next().unwrap_or(cmd);
                        return format!("Running: {}", truncate_str(cmd_name, 20));
                    }
                }
            }
            "Running command...".to_string()
        }
        "web_search" | "WebSearch" | "WebFetch" => "Searching web...".to_string(),
        "list_directory" | "LS" | "ls" => "Listing directory...".to_string(),
        "Task" => "Launching agent...".to_string(),
        "AskUserQuestion" => "Asking question...".to_string(),
        "NotebookEdit" | "NotebookRead" => "Working with notebook...".to_string(),
        _ => {
            // For unknown tools, show a truncated name
            format!("Running: {}", truncate_str(tool_name, 25))
        }
    }
}

fn resolve_codex_command() -> String {
    // Check user-specific paths first
    if let Ok(home) = std::env::var("HOME") {
        // Check ~/.local/bin (common for pip/pipx installs)
        let local_bin = format!("{home}/.local/bin/codex");
        if Path::new(&local_bin).exists() {
            return local_bin;
        }
        // Check npm global
        let npm_global = format!("{home}/.npm-global/bin/codex");
        if Path::new(&npm_global).exists() {
            return npm_global;
        }
    }
    let candidates = [
        "/opt/homebrew/bin/codex",
        "/usr/local/bin/codex",
        "/usr/bin/codex",
    ];
    for candidate in candidates {
        if Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }
    "codex".to_string()
}

fn resolve_claude_command() -> String {
    // Check user-specific paths first
    if let Ok(home) = std::env::var("HOME") {
        // Check ~/.local/bin (common for pip/pipx installs)
        let local_bin = format!("{home}/.local/bin/claude");
        if Path::new(&local_bin).exists() {
            return local_bin;
        }
        // Check npm global
        let npm_global = format!("{home}/.npm-global/bin/claude");
        if Path::new(&npm_global).exists() {
            return npm_global;
        }
    }
    let candidates = [
        "/opt/homebrew/bin/claude",
        "/usr/local/bin/claude",
        "/usr/bin/claude",
    ];
    for candidate in candidates {
        if Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }
    "claude".to_string()
}

fn resolve_agent_command(agent: &AgentConfig) -> String {
    match agent.command.as_str() {
        "claude" => resolve_claude_command(),
        "codex" => resolve_codex_command(),
        other => resolve_command_path(other)
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|| other.to_string()),
    }
}

/// Validate OAuth URLs against known OpenAI providers
fn validate_oauth_url(url: &str) -> bool {
    if let Ok(parsed) = url::Url::parse(url) {
        let allowed_domains = [
            "accounts.openai.com",
            "auth.openai.com",
            "chatgpt.com",
            "auth0.openai.com",
        ];
        if let Some(host) = parsed.host_str() {
            return parsed.scheme() == "https"
                && allowed_domains
                    .iter()
                    .any(|d| host == *d || host.ends_with(&format!(".{}", d)));
        }
    }
    false
}

async fn spawn_agent_client(
    agent: &AgentConfig,
    cwd: &Path,
    env: &[(String, String)],
    args: &[String],
) -> Result<Arc<AgentProcessClient>, String> {
    let command = resolve_agent_command(agent);
    match AgentProcessClient::spawn(&command, args, cwd, env).await {
        Ok(client) => {
            tracing::info!(agent_id = %agent.id, command = %command, "Agent CLI spawned");
            Ok(Arc::new(client))
        }
        Err(err) => {
            tracing::error!(
                agent_id = %agent.id,
                command = %command,
                error = %err,
                "Failed to spawn agent CLI"
            );
            Err(err.to_string())
        }
    }
}

/// Reconnect a session with context restoration using hybrid approach:
/// 1. Try Agent session/load if agent supports loadSession capability
/// 2. Fall back to creating a new session if not supported
/// Returns (client, session_id, used_session_load)
/// Note: The caller is responsible for history injection if used_session_load is false
async fn reconnect_session_with_context(
    agent: &AgentConfig,
    task: &db::TaskRecord,
    cwd: &Path,
    env: &[(String, String)],
    db: &Arc<StdMutex<rusqlite::Connection>>,
) -> Result<(Arc<AgentProcessClient>, String, bool), String> {
    let cwd_str = cwd.to_string_lossy().to_string();
    let args = substitute_args(&agent.args, &cwd_str);

    // Spawn and initialize Agent client
    let client = spawn_agent_client(agent, cwd, env, &args).await?;
    let _capabilities = client
        .initialize("Phantom Harness", "0.1.0")
        .await
        .map_err(|err| format!("initialize failed: {}", err))?;

    // Check if we have a stored Agent session ID and the agent supports session/load
    if client.supports_load_session() {
        if let Some(ref stored_session_id) = task.agent_session_id {
            if stored_session_id.starts_with("local-") {
                println!(
                    "[Harness] Skipping session/load for local session placeholder: {}",
                    stored_session_id
                );
            } else {
                println!(
                    "[Harness] Attempting session/load for task {} with session {}",
                    task.id, stored_session_id
                );

                // Try to load the previous session
                match client
                    .session_load(stored_session_id, &cwd_str, Vec::new())
                    .await
                {
                    Ok(load_result) => {
                        println!(
                            "[Harness] Session restored via session/load: {}",
                            load_result.session_id
                        );
                        return Ok((client, load_result.session_id, true));
                    }
                    Err(e) => {
                        // session/load failed, fall back to history injection
                        eprintln!(
                            "[Harness] session/load failed (falling back to history injection): {}",
                            e
                        );
                    }
                }
            }
        }
    }

    // Fallback: Create new session with history injection
    println!(
        "[Harness] Using history injection fallback for task {}",
        task.id
    );

    let session = client
        .session_new(&cwd_str)
        .await
        .map_err(|err| format!("session/new failed: {}", err))?;

    // Apply model selection if not default
    if task.model != "default" && !task.model.trim().is_empty() {
        let _ = apply_model_selection(client.as_ref(), &session, &task.model).await;
    }

    // Load and format conversation history for context injection
    let history_context = {
        let conn = db.lock().map_err(|e| format!("db lock error: {}", e))?;
        let messages = db::get_message_records(&conn, &task.id)
            .map_err(|e| format!("get messages error: {}", e))?;

        if !messages.is_empty() {
            // Use compaction with a reasonable limit (approx 100k chars, ~25k tokens)
            let (history, was_truncated) =
                db::compact_history(&messages, task.prompt.as_deref(), 100_000);
            if was_truncated {
                println!(
                    "[Harness] History was compacted to fit context window ({} messages)",
                    messages.len()
                );
            }
            Some(history)
        } else {
            None
        }
    };

    // Log if we have history context available (the caller handles actual injection)
    if let Some(ref history) = history_context {
        println!(
            "[Harness] History context available ({} chars), caller will handle injection",
            history.len()
        );
    }

    // Update the stored Agent session ID for future reconnections
    {
        let conn = db.lock().map_err(|e| format!("db lock error: {}", e))?;
        let _ = db::update_task_agent_session_id(&conn, &task.id, &session.session_id);
    }

    Ok((client, session.session_id, false))
}

/// Format a message with conversation history context for agents that don't support session/load
fn format_message_with_history(history: &str, new_message: &str) -> String {
    format!("{}\n[User's new message]\n{}", history, new_message)
}

/// Get cached models from SQLite (instant, for immediate UI display)
#[tauri::command]
fn get_cached_models(
    agent_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModelOption>, String> {
    println!("[Harness] get_cached_models called for: {}", agent_id);

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let cached = db::get_cached_models(&conn, &agent_id).map_err(|e| e.to_string())?;

    let models: Vec<ModelOption> = cached
        .into_iter()
        .map(|m| ModelOption {
            value: m.value,
            name: m.name,
            description: m.description,
        })
        .collect();

    println!(
        "[Harness] Returning {} cached models for {}",
        models.len(),
        agent_id
    );
    Ok(models)
}

/// Get all cached models for all agents (for startup preload)
#[tauri::command]
fn get_all_cached_models(
    state: State<'_, AppState>,
) -> Result<HashMap<String, Vec<ModelOption>>, String> {
    println!("[Harness] get_all_cached_models called");

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let all_cached = db::get_all_cached_models(&conn).map_err(|e| e.to_string())?;

    let mut result: HashMap<String, Vec<ModelOption>> = HashMap::new();
    for (agent_id, models) in all_cached {
        let model_options: Vec<ModelOption> = models
            .into_iter()
            .map(|m| ModelOption {
                value: m.value,
                name: m.name,
                description: m.description,
            })
            .collect();
        println!(
            "[Harness] Cached {} models for {}",
            model_options.len(),
            agent_id
        );
        result.insert(agent_id, model_options);
    }

    Ok(result)
}

/// Get cached modes for an agent
#[tauri::command]
fn get_cached_modes(
    agent_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModeOption>, String> {
    println!("[Harness] get_cached_modes called for: {}", agent_id);

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let cached = db::get_cached_modes(&conn, &agent_id).map_err(|e| e.to_string())?;

    let modes: Vec<ModeOption> = cached
        .into_iter()
        .map(|m| ModeOption {
            value: m.value,
            name: m.name,
            description: m.description,
        })
        .collect();

    println!(
        "[Harness] Returning {} cached modes for {}",
        modes.len(),
        agent_id
    );
    Ok(modes)
}

/// Get all cached modes for all agents (for startup preload)
#[tauri::command]
fn get_all_cached_modes_cmd(
    state: State<'_, AppState>,
) -> Result<HashMap<String, Vec<ModeOption>>, String> {
    println!("[Harness] get_all_cached_modes called");

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let all_cached = db::get_all_cached_modes(&conn).map_err(|e| e.to_string())?;

    let mut result: HashMap<String, Vec<ModeOption>> = HashMap::new();
    for (agent_id, modes) in all_cached {
        let mode_options: Vec<ModeOption> = modes
            .into_iter()
            .map(|m| ModeOption {
                value: m.value,
                name: m.name,
                description: m.description,
            })
            .collect();
        println!(
            "[Harness] Cached {} modes for {}",
            mode_options.len(),
            agent_id
        );
        result.insert(agent_id, mode_options);
    }

    Ok(result)
}

/// Helper to save modes to SQLite cache
fn save_modes_to_cache(
    db: &Arc<StdMutex<rusqlite::Connection>>,
    agent_id: &str,
    modes: &[ModeOption],
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let cached_modes: Vec<db::CachedMode> = modes
        .iter()
        .map(|m| db::CachedMode {
            value: m.value.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
        })
        .collect();
    db::save_cached_modes(&conn, agent_id, &cached_modes)
        .map_err(|e| format!("Failed to cache modes: {}", e))?;
    println!("[Harness] Cached {} modes for {}", modes.len(), agent_id);
    Ok(())
}

/// Fetch fresh modes from agent (slow, updates cache)
#[tauri::command]
async fn refresh_agent_modes(
    agent_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModeOption>, String> {
    println!("[Harness] refresh_agent_modes called for: {}", agent_id);

    // Codex uses the app-server mode/list API
    if agent_id == "codex" {
        let agent = find_agent(&state.config, &agent_id)
            .ok_or_else(|| format!("Unknown agent id: {}", agent_id))?;

        let cwd = std::env::current_dir().map_err(|err| format!("cwd error: {}", err))?;
        let cwd_str = cwd.to_string_lossy().to_string();
        let settings = state.settings.lock().await.clone();
        let overrides = auth_env_for(&agent_id, &settings);
        let allow_missing = settings.codex_auth_method.as_deref() == Some("chatgpt");
        let env = build_env(&agent.required_env, &overrides, allow_missing)
            .map_err(|err| format!("Auth not configured for {}: {}", agent_id, err))?;
        let args = substitute_args(&agent.args, &cwd_str);
        let command = resolve_agent_command(agent);

        let config = AgentLaunchConfig {
            command: command.clone(),
            args,
            env,
            cwd: cwd_str,
        };

        println!("[Harness] Fetching modes from Codex app-server");

        let modes = match backend_get_codex_modes(config).await {
            Ok(modes) => {
                println!("[Harness] Got {} modes from Codex", modes.len());
                modes
            }
            Err(err) => {
                let message = err.to_string();
                println!("[Harness] Codex modes fetch failed: {}", message);
                return Err(format!(
                    "Failed to fetch modes from Codex app-server: {}",
                    message
                ));
            }
        };

        save_modes_to_cache(&state.db, &agent_id, &modes)?;
        return Ok(modes);
    }

    let agent = find_agent(&state.config, &agent_id)
        .ok_or_else(|| format!("Unknown agent id: {}", agent_id))?;

    let cwd = std::env::current_dir().map_err(|err| format!("cwd error: {}", err))?;
    let cwd_str = cwd.to_string_lossy().to_string();
    let settings = state.settings.lock().await.clone();
    let overrides = auth_env_for(&agent_id, &settings);
    let allow_missing = false;

    // Build env - fail if auth is not configured
    let env = build_env(&agent.required_env, &overrides, allow_missing)
        .map_err(|err| format!("Auth not configured for {}: {}", agent_id, err))?;

    let args = substitute_args(&agent.args, &cwd_str);
    let command = resolve_agent_command(agent);
    let config = AgentLaunchConfig {
        command: command.clone(),
        args: args.clone(),
        env,
        cwd: cwd_str,
    };

    println!(
        "[Harness] Fetching modes from agent: {} {}",
        command,
        args.join(" ")
    );

    let modes = match backend_get_agent_modes(config).await {
        Ok(modes) => {
            println!("[Harness] Got {} modes from agent", modes.len());
            modes
        }
        Err(err) => {
            let message = err.to_string();
            println!("[Harness] Agent modes fetch failed: {}", message);
            return Err(format!(
                "Failed to fetch modes from {}: {}",
                agent_id, message
            ));
        }
    };

    // Save to cache
    save_modes_to_cache(&state.db, &agent_id, &modes)?;

    Ok(modes)
}

/// Get modes for an agent - returns cached if available, otherwise fetches
#[tauri::command]
async fn get_agent_modes(
    agent_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModeOption>, String> {
    // Try cache first for instant response
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let cached = db::get_cached_modes(&conn, &agent_id)
            .ok()
            .unwrap_or_default();
        if !cached.is_empty() {
            println!(
                "[Harness] get_agent_modes returning {} cached modes for {}",
                cached.len(),
                agent_id
            );
            return Ok(cached
                .into_iter()
                .map(|m| ModeOption {
                    value: m.value,
                    name: m.name,
                    description: m.description,
                })
                .collect());
        }
    }

    // No cache, do full fetch
    println!("[Harness] No cached modes for {}, fetching fresh", agent_id);
    refresh_agent_modes(agent_id, state).await
}

/// Get cached analytics snapshot for an agent type (codex or claude)
#[tauri::command]
fn get_cached_analytics(
    agent_type: String,
    state: State<'_, AppState>,
) -> Result<Option<serde_json::Value>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    match db::get_analytics_cache(&conn, &agent_type) {
        Ok(Some((json_str, _updated_at))) => {
            let snapshot: serde_json::Value =
                serde_json::from_str(&json_str).map_err(|e| e.to_string())?;
            Ok(Some(snapshot))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Get all cached analytics for startup preload
#[tauri::command]
fn get_all_cached_analytics(
    state: State<'_, AppState>,
) -> Result<HashMap<String, serde_json::Value>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let all_cached = db::get_all_analytics_cache(&conn).map_err(|e| e.to_string())?;

    let mut result: HashMap<String, serde_json::Value> = HashMap::new();
    for (agent_type, json_str, _updated_at) in all_cached {
        if let Ok(snapshot) = serde_json::from_str(&json_str) {
            result.insert(agent_type, snapshot);
        }
    }
    Ok(result)
}

/// Save analytics snapshot to cache
#[tauri::command]
fn save_analytics_cache(
    agent_type: String,
    snapshot: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let json_str = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::save_analytics_cache(&conn, &agent_type, &json_str).map_err(|e| e.to_string())
}

/// Fetch fresh models from agent (slow, updates cache)
#[tauri::command]
async fn refresh_agent_models(
    agent_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModelOption>, String> {
    println!("[Harness] refresh_agent_models called for: {}", agent_id);

    let agent = find_agent(&state.config, &agent_id)
        .ok_or_else(|| format!("Unknown agent id: {}", agent_id))?;

    // If using config-based models, return those and cache them
    if agent.model_source.as_deref() == Some("config") && !agent.models.is_empty() {
        println!("[Harness] Using config models for {}", agent_id);
        let mut models: Vec<ModelOption> = agent
            .models
            .iter()
            .map(|value| ModelOption {
                value: value.clone(),
                name: None,
                description: None,
            })
            .collect();

        // For Factory Droid, also include custom BYOK models from ~/.factory/settings.json
        if agent_id == "droid" || agent_id == "factory-droid" {
            let custom_models = get_factory_custom_models();
            if !custom_models.is_empty() {
                println!(
                    "[Harness] Adding {} custom BYOK models for {}",
                    custom_models.len(),
                    agent_id
                );
                models.extend(custom_models);
            }
        }

        // Cache config models too
        save_models_to_cache(&state.db, &agent_id, &models)?;
        return Ok(models);
    }

    let cwd = std::env::current_dir().map_err(|err| format!("cwd error: {}", err))?;
    let cwd_str = cwd.to_string_lossy().to_string();

    // If using app-server (Codex), fetch models via model/list API
    if agent.model_source.as_deref() == Some("app-server") {
        println!(
            "[Harness] Fetching models from Codex app-server for {}",
            agent_id
        );
        let settings = state.settings.lock().await.clone();
        let overrides = auth_env_for(&agent_id, &settings);
        let allow_missing =
            agent_id == "codex" && settings.codex_auth_method.as_deref() == Some("chatgpt");
        let env = build_env(&agent.required_env, &overrides, allow_missing)
            .map_err(|err| format!("Auth not configured for {}: {}", agent_id, err))?;

        let command = resolve_agent_command(agent);
        let config = AgentLaunchConfig {
            command,
            args: vec![], // app-server doesn't need extra args
            env,
            cwd: cwd_str,
        };

        let models = match backend_get_codex_models(config).await {
            Ok(models) => {
                println!(
                    "[Harness] Got {} models from Codex app-server",
                    models.len()
                );
                models
            }
            Err(err) => {
                let message = err.to_string();
                println!("[Harness] Codex model fetch failed: {}", message);
                return Err(format!(
                    "Failed to fetch models from Codex app-server: {}",
                    message
                ));
            }
        };

        // Save to cache
        save_models_to_cache(&state.db, &agent_id, &models)?;
        return Ok(models);
    }

    // Fallback: fetch models from agent's session/new response (ACP protocol)
    let settings = state.settings.lock().await.clone();
    let overrides = auth_env_for(&agent_id, &settings);
    let allow_missing = (agent_id == "codex"
        && settings.codex_auth_method.as_deref() == Some("chatgpt"))
        || (agent_id == "claude-code"
            && matches!(
                settings.claude_auth_method.as_deref(),
                Some("cli") | Some("oauth")
            ));

    // Build env - fail if auth is not configured
    let env = build_env(&agent.required_env, &overrides, allow_missing)
        .map_err(|err| format!("Auth not configured for {}: {}", agent_id, err))?;

    let args = substitute_args(&agent.args, &cwd_str);
    let command = resolve_agent_command(agent);
    let config = AgentLaunchConfig {
        command: command.clone(),
        args: args.clone(),
        env: env.clone(),
        cwd: cwd_str.clone(),
    };

    println!(
        "[Harness] Fetching models from agent: {} {}",
        command,
        args.join(" ")
    );

    let models = match backend_get_agent_models(config).await {
        Ok(models) => {
            println!("[Harness] Got {} models from agent", models.len());
            models
        }
        Err(err) => {
            let message = err.to_string();
            println!("[Harness] Agent models fetch failed: {}", message);
            return Err(format!(
                "Failed to fetch models from {}: {}",
                agent_id, message
            ));
        }
    };

    // Save to cache
    save_models_to_cache(&state.db, &agent_id, &models)?;

    Ok(models)
}

/// Helper to save models to SQLite cache
fn save_models_to_cache(
    db: &Arc<StdMutex<rusqlite::Connection>>,
    agent_id: &str,
    models: &[ModelOption],
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let cached_models: Vec<db::CachedModel> = models
        .iter()
        .map(|m| db::CachedModel {
            value: m.value.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
        })
        .collect();
    db::save_cached_models(&conn, agent_id, &cached_models)
        .map_err(|e| format!("Failed to cache models: {}", e))?;
    println!("[Harness] Cached {} models for {}", models.len(), agent_id);
    Ok(())
}

/// Legacy alias for compatibility - returns cached if available, otherwise fetches
#[tauri::command]
async fn get_agent_models(
    agent_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModelOption>, String> {
    // Try cache first for instant response
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let cached = db::get_cached_models(&conn, &agent_id)
            .ok()
            .unwrap_or_default();
        if !cached.is_empty() {
            println!(
                "[Harness] get_agent_models returning {} cached models for {}",
                cached.len(),
                agent_id
            );
            return Ok(cached
                .into_iter()
                .map(|m| ModelOption {
                    value: m.value,
                    name: m.name,
                    description: m.description,
                })
                .collect());
        }
    }

    // No cache, do full fetch
    println!(
        "[Harness] No cached models for {}, fetching fresh",
        agent_id
    );
    refresh_agent_models(agent_id, state).await
}

/// Get enriched models with reasoning effort support (for Codex)
#[tauri::command]
async fn get_enriched_models(
    agent_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<EnrichedModelOption>, String> {
    println!("[Harness] get_enriched_models called for: {}", agent_id);

    let agent = find_agent(&state.config, &agent_id)
        .ok_or_else(|| format!("Unknown agent id: {}", agent_id))?;

    // Only Codex app-server supports enriched models
    if agent.model_source.as_deref() != Some("app-server") {
        return Err(format!(
            "Agent {} does not support enriched models (not app-server)",
            agent_id
        ));
    }

    let cwd = std::env::current_dir().map_err(|err| format!("cwd error: {}", err))?;
    let cwd_str = cwd.to_string_lossy().to_string();
    let settings = state.settings.lock().await.clone();
    let overrides = auth_env_for(&agent_id, &settings);
    let allow_missing =
        agent_id == "codex" && settings.codex_auth_method.as_deref() == Some("chatgpt");
    let env = build_env(&agent.required_env, &overrides, allow_missing)
        .map_err(|err| format!("Auth not configured for {}: {}", agent_id, err))?;

    let command = resolve_agent_command(agent);
    let config = AgentLaunchConfig {
        command,
        args: vec![],
        env,
        cwd: cwd_str,
    };

    let models = backend_get_codex_models_enriched(config)
        .await
        .map_err(|e| format!("Failed to fetch enriched models: {}", e))?;

    println!(
        "[Harness] Got {} enriched models from Codex app-server",
        models.len()
    );
    Ok(models)
}

#[tauri::command]
async fn get_codex_commands(
    project_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AvailableCommand>, String> {
    let source_path = resolve_project_path(&project_path)?;
    let repo_root = resolve_repo_root(&source_path).await.unwrap_or(source_path);
    Ok(collect_codex_commands(state.inner(), &repo_root))
}

#[tauri::command]
async fn get_claude_commands(
    project_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AvailableCommand>, String> {
    let source_path = resolve_project_path(&project_path)?;
    let repo_root = resolve_repo_root(&source_path).await.unwrap_or(source_path);
    Ok(collect_claude_commands(state.inner(), &repo_root))
}

#[tauri::command]
async fn pick_project_path(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;

    let result = app.dialog().file().blocking_pick_folder();
    result.map(|file_path| file_path.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CreateAgentResult {
    pub(crate) task_id: String,
    pub(crate) session_id: String,
    #[serde(rename = "worktreePath")]
    pub(crate) worktree_path: Option<String>,
}

#[tauri::command]
async fn create_agent_session(
    app: AppHandle,
    window: WebviewWindow,
    payload: CreateAgentPayload,
    state: State<'_, AppState>,
) -> Result<CreateAgentResult, String> {
    let emit_to_main = window.label() != "main";
    create_agent_session_internal(app, payload, state.inner(), emit_to_main).await
}

pub(crate) async fn create_agent_session_internal(
    app: AppHandle,
    payload: CreateAgentPayload,
    state: &AppState,
    emit_to_main: bool,
) -> Result<CreateAgentResult, String> {
    let agent = find_agent(&state.config, &payload.agent_id)
        .ok_or_else(|| format!("Unknown agent id: {}", payload.agent_id))?;

    let source_path = resolve_project_path(&payload.project_path)?;
    let mut cwd = source_path.clone();
    let mut worktree_path: Option<PathBuf> = None;
    let settings = state.settings.lock().await.clone();

    // Variables for deferred branch rename (populated if worktree is created)
    let mut deferred_branch_rename: Option<(PathBuf, String, PathBuf)> = None; // (repo_root, animal_name, workspace_path)

    if payload.use_worktree {
        let repo_root = resolve_repo_root(&source_path).await;
        let sync_source = repo_root.as_deref().unwrap_or(&source_path);
        let repo_slug = worktree::repo_slug(sync_source);

        if let Some(repo_root) = repo_root.as_ref() {
            let requested_base = payload
                .base_branch
                .clone()
                .or_else(|| settings.task_base_branch.clone())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty() && value != "default");
            let base_branch = match requested_base {
                Some(value) => value,
                None => worktree::current_branch(repo_root)
                    .await
                    .unwrap_or_else(|_| "main".to_string()),
            };

            let base_ref = if worktree::branch_exists(repo_root, &base_branch).await? {
                base_branch.clone()
            } else if worktree::remote_branch_exists(repo_root, "origin", &base_branch).await? {
                format!("origin/{}", base_branch)
            } else {
                base_branch.clone()
            };

            // Create worktree with a unique animal name (base, then -v1, -v2, etc.).
            // The branch will be renamed asynchronously after LLM generates the proper name.
            let (created_path, created_branch) =
                worktree::create_worktree_with_animal_name(repo_root, &repo_slug, &base_ref)
                    .await?;
            if let Err(err) = worktree::apply_uncommitted_changes(sync_source, &created_path).await
            {
                eprintln!(
                    "[worktree] Apply uncommitted changes failed, falling back to full sync: {}",
                    err
                );
                worktree::sync_workspace_from_source(sync_source, &created_path).await?;
            }

            // Store info for deferred branch rename
            deferred_branch_rename = Some((
                repo_root.clone(),
                created_branch.clone(),
                created_path.clone(),
            ));

            worktree_path = Some(created_path.clone());

            // Preserve subdirectory path for monorepos
            if let Ok(relative) = source_path.strip_prefix(repo_root) {
                if relative.as_os_str().len() > 0 {
                    cwd = created_path.join(relative);
                } else {
                    cwd = created_path.clone();
                }
            } else {
                cwd = created_path.clone();
            }
        } else {
            let created_path = worktree::build_workspace_path(&repo_slug)?;
            std::fs::create_dir_all(&created_path)
                .map_err(|err| format!("Failed to create workspace directory: {}", err))?;
            worktree::sync_workspace_from_source(sync_source, &created_path).await?;
            cwd = created_path.clone();
            worktree_path = Some(created_path.clone());
        }
    }

    let cwd_str = cwd.to_string_lossy().to_string();
    let overrides = auth_env_for(&payload.agent_id, &settings);
    let allow_missing = (payload.agent_id == "codex"
        && settings.codex_auth_method.as_deref() == Some("chatgpt"))
        || (payload.agent_id == "claude-code"
            && matches!(
                settings.claude_auth_method.as_deref(),
                Some("cli") | Some("oauth")
            ));
    let env = build_env(&agent.required_env, &overrides, allow_missing)?;
    let args = substitute_args(&agent.args, &cwd_str);
    let client = spawn_agent_client(agent, &cwd, &env, &args)
        .await
        .map_err(|err| format!("spawn failed: {}", err))?;
    client
        .initialize("Phantom Harness", "0.1.0")
        .await
        .map_err(|err| format!("initialize failed: {}", err))?;

    // For Codex, set model, reasoning effort, and mode before session_new (they're passed to thread/start)
    if payload.agent_id == "codex" {
        // Set model on client if specified
        if payload.exec_model != "default" && !payload.exec_model.trim().is_empty() {
            let _ = client
                .set_session_model("", &payload.exec_model)
                .await
                .map_err(|err| format!("set model failed: {}", err))?;
        }
        // Set reasoning effort on client if specified, otherwise fall back to thinking toggle
        if let Some(ref effort) = payload.reasoning_effort {
            if effort != "default" && !effort.trim().is_empty() {
                client.set_reasoning_effort(Some(effort));
                println!("[Harness] Set reasoning effort: {}", effort);
            }
        } else if payload.thinking {
            client.set_reasoning_effort(Some("high"));
            println!("[Harness] Set reasoning effort: high (thinking enabled)");
        } else {
            client.set_reasoning_effort(Some("low"));
            println!("[Harness] Set reasoning effort: low (thinking disabled)");
        }
        // Set Codex mode on client if specified
        if let Some(ref mode) = payload.codex_mode {
            if mode != "default" && !mode.trim().is_empty() {
                client.set_codex_mode(Some(mode));
                println!("[Harness] Set Codex mode: {}", mode);
            }
        }
    }

    let session = client
        .session_new(&cwd_str)
        .await
        .map_err(|err| format!("session/new failed: {}", err))?;

    // Set permission mode if provided and modes are available
    if !payload.permission_mode.is_empty() && payload.permission_mode != "default" {
        if let Err(e) = client
            .session_set_mode(&session.session_id, &payload.permission_mode)
            .await
        {
            eprintln!("[Harness] session/set_mode failed (non-fatal): {}", e);
        }
    }

    // Set agent mode (for agents that expose modes)
    if let Some(ref agent_mode) = payload.agent_mode {
        if !agent_mode.is_empty() && agent_mode != "default" {
            println!("[Harness] Setting agent mode: {}", agent_mode);
            if let Err(e) = client
                .session_set_mode(&session.session_id, agent_mode)
                .await
            {
                eprintln!(
                    "[Harness] session/set_mode (agent_mode) failed (non-fatal): {}",
                    e
                );
            }
        }
    }

    // Select model: use exec_model if specified, otherwise fall back to agent defaults
    let selected = if payload.exec_model == "default" {
        if payload.plan_mode {
            // In plan mode, prefer default_plan_model if available
            agent
                .default_plan_model
                .clone()
                .or_else(|| agent.default_exec_model.clone())
                .unwrap_or_else(|| "default".to_string())
        } else {
            agent
                .default_exec_model
                .clone()
                .unwrap_or_else(|| "default".to_string())
        }
    } else {
        payload.exec_model.clone()
    };

    if selected != "default" && !selected.trim().is_empty() {
        let _ = apply_model_selection(client.as_ref(), &session, &selected)
            .await
            .map_err(|err| format!("set model failed: {}", err))?;
    }

    let task_id = format!(
        "task-{}-{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0000")
    );

    // Spawn Claude usage watcher for real-time cost tracking
    let claude_watcher = if payload.agent_id == "claude-code" {
        Some(claude_usage_watcher::start_watching(
            &session.session_id,
            &task_id,
            app.clone(),
            state.db.clone(),
        ))
    } else {
        None
    };

    let handle = SessionHandle {
        agent_id: payload.agent_id.clone(),
        session_id: session.session_id.clone(),
        model: selected.clone(),
        client,
        pending_prompt: Some(payload.prompt.clone()),
        pending_attachments: payload.attachments.clone(),
        messages: Vec::new(),
        claude_watcher,
        cancel_token: CancellationToken::new(),
    };

    let mut sessions = state.sessions.lock().await;
    sessions.insert(task_id.clone(), Arc::new(Mutex::new(handle)));

    // Persist task to database (including Agent session ID for context restoration)
    let initial_branch = if let Some(path) = worktree_path.as_ref() {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    } else if let Some(repo_root) = resolve_repo_root(&source_path).await {
        worktree::current_branch(&repo_root)
            .await
            .ok()
            .filter(|value| !value.trim().is_empty())
    } else {
        None
    };

    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().timestamp();
        let task = db::TaskRecord {
            id: task_id.clone(),
            agent_id: payload.agent_id.clone(),
            model: selected.clone(),
            prompt: Some(payload.prompt.clone()),
            project_path: payload.project_path.clone(),
            worktree_path: worktree_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            branch: initial_branch,
            status: "Ready".to_string(),
            status_state: "idle".to_string(),
            cost: 0.0,
            created_at: now,
            updated_at: now,
            title_summary: None,
            agent_session_id: Some(session.session_id.clone()),
            total_tokens: None,
            context_window: None,
        };
        db::insert_task(&conn, &task).map_err(|e| e.to_string())?;
    }

    // Generate AI title summary in the background (non-blocking)
    if settings.ai_summaries_enabled.unwrap_or(true) {
        let prompt_clone = payload.prompt.clone();
        let agent_clone = payload.agent_id.clone();
        let task_id_clone = task_id.clone();
        let db_clone = state.db.clone();
        let window_opt = app.get_webview_window("main");
        let summaries_agent = settings.summaries_agent.clone();

        tauri::async_runtime::spawn(async move {
            let title = summarize::summarize_title_with_override(
                &prompt_clone,
                &agent_clone,
                summaries_agent.as_deref(),
            )
            .await;
            println!("[Harness] Generated title summary: {}", title);

            // Update database
            if let Ok(conn) = db_clone.lock() {
                if let Err(e) = db::update_task_title_summary(&conn, &task_id_clone, &title) {
                    eprintln!("[Harness] Failed to save title summary: {}", e);
                }
            }

            // Emit event to frontend
            if let Some(window) = window_opt {
                if let Err(e) = window.emit("TitleUpdate", (&task_id_clone, &title)) {
                    eprintln!("[Harness] Failed to emit TitleUpdate: {}", e);
                }
            }
        });
    }

    // Spawn async branch rename task (deferred branch naming)
    // This runs after worktree creation with animal name, generating the proper LLM branch name
    if let Some((repo_root, animal_name, workspace_path)) = deferred_branch_rename {
        let prompt_clone = payload.prompt.clone();
        let agent_clone = payload.agent_id.clone();
        let task_id_clone = task_id.clone();
        let window_opt = app.get_webview_window("main");
        let multi_create = payload.multi_create;
        let db_clone = state.db.clone();
        let summaries_agent = settings.summaries_agent.clone();
        let api_key = match payload.agent_id.as_str() {
            "codex" => settings.openai_api_key.clone(),
            "claude-code" => settings.anthropic_api_key.clone(),
            _ => settings
                .openai_api_key
                .clone()
                .or(settings.anthropic_api_key.clone()),
        };

        tauri::async_runtime::spawn(async move {
            // Generate proper branch name via LLM (using configured summaries agent)
            let metadata = namegen::generate_run_metadata_with_timeout_and_override(
                &prompt_clone,
                &agent_clone,
                summaries_agent.as_deref(),
                api_key.as_deref(),
                5,
            )
            .await;

            // Build the target branch name (with UUID suffix for multi-create)
            let branch_seed = if multi_create {
                let suffix = uuid::Uuid::new_v4()
                    .to_string()
                    .split('-')
                    .next()
                    .unwrap_or("0000")
                    .to_string();
                format!("{}-{}", metadata.branch_name, suffix)
            } else {
                metadata.branch_name.clone()
            };

            // Make the branch name unique if needed
            let new_branch = match worktree::unique_branch_name(&repo_root, &branch_seed).await {
                Ok(name) => name,
                Err(e) => {
                    eprintln!("[worktree] Failed to generate unique branch name: {}", e);
                    return; // Keep animal name as fallback
                }
            };

            // Skip rename if the generated name is the same as the animal name
            if new_branch == animal_name {
                println!(
                    "[worktree] Branch name unchanged, skipping rename: {}",
                    animal_name
                );
                return;
            }

            // Rename branch in the worktree
            if let Err(e) =
                worktree::rename_worktree_branch(&workspace_path, &animal_name, &new_branch).await
            {
                eprintln!(
                    "[worktree] Branch rename failed (keeping {}): {}",
                    animal_name, e
                );
                return; // Keep animal name as fallback
            }

            println!(
                "[worktree] Renamed branch: {} -> {}",
                animal_name, new_branch
            );

            // Save branch name to database for persistence across restarts
            if let Ok(conn) = db_clone.lock() {
                if let Err(e) = db::update_task_branch(&conn, &task_id_clone, &new_branch) {
                    eprintln!("[worktree] Failed to save branch to DB: {}", e);
                }
            }

            // Emit event to update UI with new branch name
            if let Some(window) = window_opt {
                if let Err(e) = window.emit("BranchUpdate", (&task_id_clone, &new_branch)) {
                    eprintln!("[worktree] Failed to emit BranchUpdate: {}", e);
                }
            }
        });
    }

    println!(
        "[Harness] Agent session created: task_id={} session_id={} (model: {})",
        task_id, session.session_id, selected
    );

    // Emit AddTask to main window so task list updates for non-main origins
    // (e.g., code review from chat log window)
    if emit_to_main {
        if let Some(main_window) = app.get_webview_window("main") {
            let initial_branch = worktree_path
                .as_ref()
                .and_then(|path| path.file_name())
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
            let add_task_payload = serde_json::json!({
                "ID": task_id,
                "agent": payload.agent_id,
                "model": selected,
                "Status": "Ready",
                "statusState": "idle",
                "cost": 0,
                "worktreePath": worktree_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                "projectPath": payload.project_path,
                "branch": initial_branch,
                "totalTokens": serde_json::Value::Null,
                "contextWindow": serde_json::Value::Null,
            });
            let _ = main_window.emit("AddTask", (&task_id, add_task_payload));
        }
    }

    if payload.agent_id == "codex" || payload.agent_id == "claude-code" {
        let command_root = if let Some(ref worktree_path) = worktree_path {
            resolve_repo_root(worktree_path)
                .await
                .unwrap_or_else(|| worktree_path.clone())
        } else {
            resolve_repo_root(&source_path)
                .await
                .unwrap_or_else(|| source_path.clone())
        };
        let commands = if payload.agent_id == "codex" {
            collect_codex_commands(state, &command_root)
        } else {
            collect_claude_commands(state, &command_root)
        };
        emit_available_commands(&app, &task_id, &payload.agent_id, &commands);
    }

    Ok(CreateAgentResult {
        task_id,
        session_id: session.session_id,
        worktree_path: worktree_path.map(|path| path.to_string_lossy().to_string()),
    })
}

pub(crate) async fn create_task_from_discord(
    app: AppHandle,
    state: &AppState,
    prompt: String,
    agent_id: String,
    project_path: String,
    model: String,
) -> Result<String, String> {
    let settings = state.settings.lock().await.clone();
    let agent_models = settings.task_agent_models.clone().unwrap_or_default();
    let prefs = agent_models.get(&agent_id).cloned().unwrap_or_default();

    let plan_mode = settings.task_plan_mode.unwrap_or(false);
    let thinking = settings.task_thinking.unwrap_or(true);
    let use_worktree = true;
    let base_branch = settings.task_base_branch.clone();

    let agents_with_own_permissions = [
        "codex",
        "claude-code",
        "droid",
        "factory-droid",
        "amp",
        "opencode",
    ];
    let permission_mode = if agents_with_own_permissions.contains(&agent_id.as_str()) {
        "bypassPermissions".to_string()
    } else {
        prefs
            .permission_mode
            .clone()
            .unwrap_or_else(|| "default".to_string())
    };

    let reasoning_effort = prefs
        .reasoning_effort
        .clone()
        .filter(|value| value != "default");
    let agent_mode = if agent_id == "opencode" {
        Some(
            prefs
                .agent_mode
                .clone()
                .unwrap_or_else(|| "build".to_string()),
        )
    } else {
        prefs.agent_mode.clone()
    };
    let codex_mode = if agent_id == "codex" {
        let mode = if plan_mode {
            "plan".to_string()
        } else {
            prefs
                .agent_mode
                .clone()
                .unwrap_or_else(|| "default".to_string())
        };
        if mode == "default" {
            None
        } else {
            Some(mode)
        }
    } else {
        None
    };

    let exec_model = if model.trim().is_empty() {
        "default".to_string()
    } else {
        model
    };

    let project_path = if project_path.trim().is_empty() {
        None
    } else {
        Some(project_path)
    };
    let allowlist = settings
        .task_project_allowlist
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    if !allowlist.is_empty() {
        let Some(project_path_value) = project_path
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        else {
            return Err("Discord tasks require a project selection.".to_string());
        };
        if !project_path_allowed(&allowlist, project_path_value) {
            return Err(
                "Project path is not in the allowlist. Update Settings > Project Allowlist."
                    .to_string(),
            );
        }
    }

    let payload = CreateAgentPayload {
        agent_id: agent_id.clone(),
        prompt,
        project_path,
        base_branch,
        plan_mode,
        thinking,
        use_worktree,
        permission_mode,
        exec_model,
        reasoning_effort,
        agent_mode,
        codex_mode,
        attachments: Vec::new(),
        multi_create: false,
    };

    let result = create_agent_session_internal(app.clone(), payload, state, false).await?;
    if let Some(window) = app.get_webview_window("main") {
        let task_snapshot = if let Ok(conn) = state.db.lock() {
            db::list_tasks(&conn)
                .ok()
                .and_then(|tasks| tasks.into_iter().find(|task| task.id == result.task_id))
        } else {
            None
        };

        if let Some(task) = task_snapshot {
            let payload = serde_json::json!({
                "ID": task.id,
                "agent": task.agent_id,
                "model": task.model,
                "Status": task.status,
                "statusState": task.status_state,
                "cost": task.cost,
                "worktreePath": task.worktree_path,
                "totalTokens": task.total_tokens,
                "contextWindow": task.context_window,
                "projectPath": task.project_path,
                "branch": task.branch,
            });
            let _ = window.emit("AddTask", (&result.task_id, payload));
        }
    }
    let window = app.get_webview_window("main");
    start_task_internal(result.task_id.clone(), state, app, window).await?;

    if discord_enabled(&settings) {
        let intro_message = format!("**Discord task started** `{}`", result.task_id);
        let _ = ensure_discord_thread(state, &result.task_id, &intro_message).await;
    }
    Ok(result.task_id)
}

#[tauri::command]
async fn start_task(
    task_id: String,
    state: State<'_, AppState>,
    window: WebviewWindow,
) -> Result<(), String> {
    start_task_internal(
        task_id,
        state.inner(),
        window.app_handle().clone(),
        Some(window),
    )
    .await
}

pub(crate) async fn start_task_internal(
    task_id: String,
    state: &AppState,
    app: AppHandle,
    window: Option<WebviewWindow>,
) -> Result<(), String> {
    // Prevent duplicate starts for the same task id (rapid-clicking Start).
    // If a run is already in-flight, treat this as a no-op.
    struct RunningTaskGuard {
        task_id: String,
        running_tasks: Arc<Mutex<HashSet<String>>>,
    }
    impl Drop for RunningTaskGuard {
        fn drop(&mut self) {
            let task_id = self.task_id.clone();
            let running_tasks = self.running_tasks.clone();
            // best-effort cleanup
            tauri::async_runtime::spawn(async move {
                let mut set = running_tasks.lock().await;
                set.remove(&task_id);
            });
        }
    }

    let window_ref = window.as_ref();
    let emit_status = |message: &str, color: &str, status_state: &str| -> Result<(), String> {
        if let Some(window) = window_ref {
            window
                .emit("StatusUpdate", (&task_id, message, color, status_state))
                .map_err(|e| e.to_string())?;
        } else if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.emit("StatusUpdate", (&task_id, message, color, status_state));
        }
        Ok(())
    };

    {
        let mut set = state.running_tasks.lock().await;
        if set.contains(&task_id) {
            println!(
                "[Harness] start_task ignored (already running): {}",
                task_id
            );
            return Ok(());
        }
        set.insert(task_id.clone());
    }
    let _running_guard = RunningTaskGuard {
        task_id: task_id.clone(),
        running_tasks: state.running_tasks.clone(),
    };
    // Emit initial status
    emit_status("Starting...", "yellow", "running")?;

    // Update DB status to running
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db::update_task_status(&conn, &task_id, "Starting...", "running");
    }

    // Extract session handle without removing it from the map so concurrent requests can access it.
    let handle_ref = {
        let sessions = state.sessions.lock().await;
        sessions.get(&task_id).cloned()
    };

    let handle_ref = if let Some(handle_ref) = handle_ref {
        handle_ref
    } else {
        println!(
            "[Harness] Session not found for start_task, attempting to reconnect: {}",
            task_id
        );

        // Look up task from DB to get reconnection info
        let task = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            db::list_tasks(&conn)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|t| t.id == task_id)
        };

        let task = match task {
            Some(t) => t,
            None => {
                let error_msg = "Task not found in database";
                println!("[Harness] start_task reconnect error: {}", error_msg);
                emit_status(error_msg, "red", "error")?;
                return Err(error_msg.to_string());
            }
        };

        // Find agent config
        let agent = match find_agent(&state.config, &task.agent_id) {
            Some(a) => a,
            None => {
                let error_msg = format!("Unknown agent: {}", task.agent_id);
                println!("[Harness] start_task reconnect error: {}", error_msg);
                emit_status(&error_msg, "red", "error")?;
                return Err(error_msg);
            }
        };

        // Set up working directory
        let cwd = resolve_task_cwd(&task)?;

        // Build environment
        let settings = state.settings.lock().await.clone();
        let overrides = auth_env_for(&task.agent_id, &settings);
        let allow_missing = (task.agent_id == "codex"
            && settings.codex_auth_method.as_deref() == Some("chatgpt"))
            || (task.agent_id == "claude-code"
                && matches!(
                    settings.claude_auth_method.as_deref(),
                    Some("cli") | Some("oauth")
                ));

        let env = match build_env(&agent.required_env, &overrides, allow_missing) {
            Ok(e) => e,
            Err(e) => {
                let error_msg = format!("Auth not configured: {}", e);
                println!("[Harness] start_task reconnect error: {}", error_msg);
                emit_status(&error_msg, "red", "error")?;
                return Err(error_msg);
            }
        };

        // Reconnect with context restoration (hybrid: session/load or history injection)
        println!(
            "[Harness] Reconnecting Agent session for start_task: {}",
            task_id
        );
        emit_status("Reconnecting...", "yellow", "running")?;

        let (client, session_id, used_session_load) =
            match reconnect_session_with_context(agent, &task, &cwd, &env, &state.db).await {
                Ok(result) => result,
                Err(e) => {
                    let error_msg = format!("Failed to reconnect: {}", e);
                    println!("[Harness] start_task reconnect error: {}", error_msg);
                    emit_status(&error_msg, "red", "error")?;
                    return Err(error_msg);
                }
            };

        let model = task.model.clone();
        let resume_prompt = if task.status == "Stopped" {
            Some("Continue".to_string())
        } else {
            task.prompt.clone()
        };

        // Prepare the prompt with history context if needed
        // For start_task, we're re-running the original prompt, so inject history before it
        let prompt_with_context = if !used_session_load {
            // Load history for context injection
            let history_opt = {
                let conn = state.db.lock().map_err(|e| e.to_string())?;
                let messages =
                    db::get_message_records(&conn, &task.id).map_err(|e| e.to_string())?;
                if !messages.is_empty() {
                    let (history, _) = db::compact_history(&messages, None, 100_000);
                    Some(history)
                } else {
                    None
                }
            };

            if let Some(history) = history_opt {
                if let Some(ref base_prompt) = resume_prompt {
                    Some(format_message_with_history(&history, base_prompt))
                } else {
                    resume_prompt.clone()
                }
            } else {
                resume_prompt.clone()
            }
        } else {
            // session/load restored context, use original prompt
            resume_prompt.clone()
        };

        println!(
            "[Harness] Session reconnected for start_task: task_id={} session_id={} (used_session_load={})",
            task_id, session_id, used_session_load
        );

        // Spawn Claude usage watcher for reconnected sessions
        let claude_watcher = if task.agent_id == "claude-code" {
            Some(claude_usage_watcher::start_watching(
                &session_id,
                &task_id,
                app.clone(),
                state.db.clone(),
            ))
        } else {
            None
        };

        let handle = SessionHandle {
            agent_id: task.agent_id.clone(),
            session_id,
            model: model.clone(),
            client,
            pending_prompt: prompt_with_context,
            pending_attachments: Vec::new(),
            messages: Vec::new(),
            claude_watcher,
            cancel_token: CancellationToken::new(),
        };

        let handle_ref = Arc::new(Mutex::new(handle));
        let mut sessions = state.sessions.lock().await;
        sessions.insert(task_id.clone(), handle_ref.clone());

        if task.agent_id == "codex" || task.agent_id == "claude-code" {
            let command_root = resolve_repo_root(&cwd).await.unwrap_or_else(|| cwd.clone());
            let commands = if task.agent_id == "codex" {
                collect_codex_commands(state, &command_root)
            } else {
                collect_claude_commands(state, &command_root)
            };
            emit_available_commands(&app, &task_id, &task.agent_id, &commands);
        }
        handle_ref
    };

    let user_timestamp = chrono::Utc::now().to_rfc3339();
    let (agent_id, model, prompt, attachments, client, session_id, cancel_token) = {
        let mut handle = handle_ref.lock().await;
        let prompt = handle
            .pending_prompt
            .take()
            .ok_or("No prompt pending - task may have already started")?;
        let attachments = std::mem::take(&mut handle.pending_attachments);
        handle.messages.push(serde_json::json!({
            "message_type": "user_message",
            "content": prompt,
            "timestamp": user_timestamp
        }));
        // Create a fresh cancellation token for this generation
        handle.cancel_token = CancellationToken::new();
        (
            handle.agent_id.clone(),
            handle.model.clone(),
            prompt,
            attachments,
            handle.client.clone(),
            handle.session_id.clone(),
            handle.cancel_token.clone(),
        )
    };

    // Load images from attachments
    let images: Vec<ImageContent> = {
        let mut loaded = Vec::new();
        if !attachments.is_empty() {
            let base_dir = attachments_dir().map_err(|e| e.to_string())?;
            for att in &attachments {
                let file_path = base_dir.join(&att.relative_path);
                if file_path.exists() {
                    match std::fs::read(&file_path) {
                        Ok(data) => {
                            use base64::Engine;
                            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                            let media_type = att
                                .mime_type
                                .clone()
                                .unwrap_or_else(|| "image/png".to_string());
                            loaded.push(ImageContent {
                                media_type,
                                data: encoded,
                            });
                        }
                        Err(e) => {
                            eprintln!(
                                "[Harness] Failed to read attachment {}: {}",
                                att.relative_path, e
                            );
                        }
                    }
                }
            }
        }
        loaded
    };

    // Get chat window label for emitting streaming updates
    let chat_window_label = format!("chat-{}", task_id);

    // Persist user message before sending so it renders first in history
    let message_id = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::save_message(
            &conn,
            &task_id,
            "user_message",
            Some(&prompt),
            None,
            None,
            None,
            None,
            &user_timestamp,
        )
        .map_err(|e| e.to_string())?
    };
    if !attachments.is_empty() {
        let attachment_records: Vec<db::AttachmentRecord> = attachments
            .iter()
            .map(|att| db::AttachmentRecord {
                id: att.id.clone(),
                file_name: None,
                mime_type: att.mime_type.clone(),
                relative_path: att.relative_path.clone(),
                byte_size: 0,
            })
            .collect();
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db::save_message_attachments(&conn, &task_id, message_id, &attachment_records);
    }

    // Build attachment info with data URLs for chat display
    let chat_attachments: Vec<serde_json::Value> = attachments
        .iter()
        .zip(images.iter())
        .map(|(att, img)| {
            let data_url = format!("data:{};base64,{}", img.media_type, img.data);
            let file_name = att
                .relative_path
                .split('/')
                .last()
                .unwrap_or(&att.relative_path);
            serde_json::json!({
                "id": att.id,
                "fileName": file_name,
                "mimeType": att.mime_type,
                "dataUrl": data_url
            })
        })
        .collect();

    if let Some(chat_window) = app.get_webview_window(&chat_window_label) {
        let user_chat_msg = if chat_attachments.is_empty() {
            serde_json::json!({
                "message_type": "user_message",
                "content": prompt.clone(),
                "timestamp": user_timestamp
            })
        } else {
            serde_json::json!({
                "message_type": "user_message",
                "content": prompt.clone(),
                "timestamp": user_timestamp,
                "attachments": chat_attachments
            })
        };
        let _ = chat_window.emit("ChatLogUpdate", (&task_id, user_chat_msg));
    }

    post_discord_user_message(state, &task_id, &agent_id, &prompt).await;

    // Send prompt to agent
    emit_status("Sending to agent...", "yellow", "running")?;

    let chat_window_label_streaming = chat_window_label.clone();
    let agent_id_for_stream = agent_id.clone();

    // Set up channel for streaming updates
    let (stream_tx, stream_rx) = std::sync::mpsc::channel::<StreamingUpdate>();

    // Spawn a task to emit streaming updates to the chat window AND main window status
    let app_handle = app.clone();
    let task_id_clone = task_id.clone();
    let db_for_stream = state.db.clone();
    let state_for_stream: AppState = state.clone();
    let stream_emit_handle = tokio::task::spawn_blocking(move || {
        use std::time::{Duration, Instant};

        // Throttle status updates to max ~10/sec for performance with multiple concurrent agents
        let throttle_duration = Duration::from_millis(100);
        let mut last_status_update = Instant::now()
            .checked_sub(throttle_duration)
            .unwrap_or_else(Instant::now);

        while let Ok(update) = stream_rx.recv() {
            // Emit status update to main window (throttled for non-tool updates)
            let should_emit_status = match &update {
                // Tool calls are always important - show immediately
                StreamingUpdate::ToolCall { .. } | StreamingUpdate::ToolReturn { .. } => true,
                // Status messages are always important
                StreamingUpdate::Status { .. } => true,
                // Permission requests are always important - user needs to see and respond
                StreamingUpdate::PermissionRequest { .. } => true,
                // User input requests are always important - user needs to answer
                StreamingUpdate::UserInputRequest { .. } => true,
                // Plan updates should surface immediately
                StreamingUpdate::PlanUpdate { .. } => true,
                // Plan content should surface immediately
                StreamingUpdate::PlanContent { .. } => true,
                // Text/reasoning chunks are throttled to prevent UI overload
                StreamingUpdate::TextChunk { .. } | StreamingUpdate::ReasoningChunk { .. } => {
                    last_status_update.elapsed() >= throttle_duration
                }
                // Commands don't need main window status
                StreamingUpdate::AvailableCommands { .. } => false,
            };

            if should_emit_status {
                if let Some(main_window) = app_handle.get_webview_window("main") {
                    let (status_text, color) = match &update {
                        StreamingUpdate::ToolCall { name, arguments } => {
                            (format_tool_status(name, arguments), "yellow")
                        }
                        StreamingUpdate::ToolReturn { .. } => {
                            ("Tool completed".to_string(), "white")
                        }
                        StreamingUpdate::ReasoningChunk { .. } => {
                            ("Thinking...".to_string(), "white")
                        }
                        StreamingUpdate::TextChunk { .. } => ("Responding...".to_string(), "white"),
                        StreamingUpdate::Status { message } => (message.clone(), "yellow"),
                        StreamingUpdate::PermissionRequest { tool_name, .. } => {
                            (format!("Waiting for permission: {}", tool_name), "#4ade80")
                        }
                        StreamingUpdate::UserInputRequest { .. } => {
                            ("Waiting for input...".to_string(), "#4ade80")
                        }
                        StreamingUpdate::PlanUpdate { .. } => ("Plan updated".to_string(), "white"),
                        StreamingUpdate::PlanContent { .. } => {
                            ("Plan content".to_string(), "white")
                        }
                        StreamingUpdate::AvailableCommands { .. } => {
                            // Handled separately, won't reach here due to should_emit_status check
                            continue;
                        }
                    };
                    let _ = main_window.emit(
                        "StatusUpdate",
                        (&task_id_clone, &status_text, color, "running"),
                    );
                    last_status_update = Instant::now();
                }
            }

            // Persist streaming updates for structural events only (tool calls, permissions, etc.)
            // NOTE: TextChunk and ReasoningChunk are NOT persisted here - they accumulate
            // during streaming and the final complete message is saved after streaming completes.
            // This prevents duplicate messages in the chat history.
            {
                let ts = chrono::Utc::now().to_rfc3339();
                if let Ok(conn) = db_for_stream.lock() {
                    let _ = match &update {
                        // Skip TextChunk - final message saved after streaming completes
                        StreamingUpdate::TextChunk { .. } => Ok(0),
                        // Skip ReasoningChunk - final message saved after streaming completes
                        StreamingUpdate::ReasoningChunk { .. } => Ok(0),
                        StreamingUpdate::ToolCall { name, arguments } => db::save_message(
                            &conn,
                            &task_id_clone,
                            "tool_call",
                            None,
                            None,
                            Some(name),
                            Some(arguments),
                            None,
                            &ts,
                        ),
                        StreamingUpdate::ToolReturn { output } => db::save_message(
                            &conn,
                            &task_id_clone,
                            "tool_return",
                            None,
                            None,
                            None,
                            None,
                            Some(output),
                            &ts,
                        ),
                        // Skip Status - these are transient UI updates, not conversation history
                        StreamingUpdate::Status { .. } => Ok(0),
                        StreamingUpdate::PermissionRequest {
                            tool_name,
                            description,
                            raw_input,
                            ..
                        } => db::save_message(
                            &conn,
                            &task_id_clone,
                            "permission_request",
                            description.as_deref(),
                            None,
                            Some(tool_name),
                            raw_input.as_deref(),
                            None,
                            &ts,
                        ),
                        StreamingUpdate::UserInputRequest {
                            request_id,
                            questions,
                        } => {
                            // Persist request_id + questions payload so we can reconstruct state on reload.
                            let payload = serde_json::json!({
                                "requestId": request_id,
                                "questions": questions,
                            });
                            let questions_json =
                                serde_json::to_string(&payload).unwrap_or_default();
                            db::save_message(
                                &conn,
                                &task_id_clone,
                                "user_input_request",
                                Some(&questions_json),
                                None,
                                None,
                                None,
                                None,
                                &ts,
                            )
                        }
                        StreamingUpdate::PlanUpdate {
                            turn_id,
                            explanation,
                            steps,
                        } => {
                            let payload = serde_json::json!({
                                "turnId": turn_id,
                                "explanation": explanation,
                                "plan": steps
                            });
                            let content = serde_json::to_string(&payload).unwrap_or_default();
                            db::save_message(
                                &conn,
                                &task_id_clone,
                                "plan_update",
                                Some(&content),
                                None,
                                None,
                                None,
                                None,
                                &ts,
                            )
                        }
                        StreamingUpdate::PlanContent { file_path, content } => {
                            let payload = serde_json::json!({
                                "file_path": file_path,
                                "content": content
                            });
                            let content_str = serde_json::to_string(&payload).unwrap_or_default();
                            db::save_message(
                                &conn,
                                &task_id_clone,
                                "plan_content",
                                Some(&content_str),
                                None,
                                None,
                                None,
                                None,
                                &ts,
                            )
                        }
                        StreamingUpdate::AvailableCommands { .. } => Ok(0),
                    };
                }
            }

            if let StreamingUpdate::UserInputRequest {
                request_id,
                questions,
            } = &update
            {
                let pending = PendingUserInput {
                    request_id: request_id.clone(),
                    questions: questions.clone(),
                    answers: std::collections::HashMap::new(),
                };
                let state_clone = state_for_stream.clone();
                let task_id = task_id_clone.clone();
                let request_id = request_id.clone();
                let questions = questions.clone();
                tauri::async_runtime::spawn(async move {
                    {
                        let mut guard: tokio::sync::MutexGuard<
                            '_,
                            std::collections::HashMap<String, PendingUserInput>,
                        > = state_clone.pending_user_inputs.lock().await;
                        guard.insert(task_id.clone(), pending);
                    }
                    post_discord_user_input_request(
                        &state_clone,
                        &task_id,
                        &request_id,
                        &questions,
                    )
                    .await;
                });
            }

            // Emit to chat window (always, for streaming display)
            if let Some(chat_window) = app_handle.get_webview_window(&chat_window_label_streaming) {
                // Convert StreamingUpdate to a chat message format
                let chat_msg = match &update {
                    StreamingUpdate::TextChunk { text, item_id } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "text_chunk",
                        "content": text,
                        "item_id": item_id
                    }),
                    StreamingUpdate::ReasoningChunk { text } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "reasoning_chunk",
                        "content": text
                    }),
                    StreamingUpdate::ToolCall { name, arguments } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "tool_call",
                        "name": name,
                        "arguments": arguments
                    }),
                    StreamingUpdate::ToolReturn { output } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "tool_return",
                        "content": output
                    }),
                    StreamingUpdate::Status { message } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "status",
                        "content": message
                    }),
                    StreamingUpdate::AvailableCommands { commands } => {
                        // Emit available commands to all windows for slash command autocomplete
                        if agent_id_for_stream == "codex" || agent_id_for_stream == "claude-code" {
                            continue;
                        }
                        emit_available_commands(
                            &app_handle,
                            &task_id_clone,
                            &agent_id_for_stream,
                            commands,
                        );
                        continue;
                    }
                    StreamingUpdate::PermissionRequest {
                        request_id,
                        tool_name,
                        description,
                        raw_input,
                        options,
                    } => {
                        serde_json::json!({
                            "type": "streaming",
                            "message_type": "permission_request",
                            "request_id": request_id,
                            "tool_name": tool_name,
                            "description": description,
                            "raw_input": raw_input,
                            "options": options
                        })
                    }
                    StreamingUpdate::UserInputRequest {
                        request_id,
                        questions,
                    } => {
                        serde_json::json!({
                            "type": "streaming",
                            "message_type": "user_input_request",
                            "request_id": request_id,
                            "questions": questions
                        })
                    }
                    StreamingUpdate::PlanUpdate {
                        turn_id,
                        explanation,
                        steps,
                    } => {
                        serde_json::json!({
                            "type": "streaming",
                            "message_type": "plan_update",
                            "turn_id": turn_id,
                            "explanation": explanation,
                            "plan": steps
                        })
                    }
                    StreamingUpdate::PlanContent { file_path, content } => {
                        serde_json::json!({
                            "type": "streaming",
                            "message_type": "plan_content",
                            "file_path": file_path,
                            "content": content
                        })
                    }
                };
                let _ = chat_window.emit("ChatLogStreaming", (&task_id_clone, chat_msg));
            }
        }
    });

    // Use streaming version of session_prompt (with images if present)
    // Wrapped in retry logic for recoverable errors (exit code 143/SIGTERM)
    const MAX_RECONNECT_ATTEMPTS: u32 = 2;
    let mut attempt = 0;
    let mut client = client;
    let mut session_id = session_id;

    let response = loop {
        attempt += 1;

        let result = if images.is_empty() {
            client
                .session_prompt_streaming_with_cancellation(
                    &session_id,
                    &prompt,
                    |update| {
                        let _ = stream_tx.send(update);
                    },
                    Some(&cancel_token),
                )
                .await
        } else {
            println!("[Harness] Sending prompt with {} image(s)", images.len());
            client
                .session_prompt_streaming_with_images_and_cancellation(
                    &session_id,
                    &prompt,
                    &images,
                    |update| {
                        let _ = stream_tx.send(update);
                    },
                    Some(&cancel_token),
                )
                .await
        };

        match result {
            Ok(response) => break response,
            Err(e) => {
                let error_str = e.to_string();
                println!(
                    "[Harness] session/prompt error (attempt {}): {}",
                    attempt, error_str
                );

                // Check if this is a recoverable exit (SIGTERM/exit code 143)
                if is_recoverable_exit(&error_str) && attempt < MAX_RECONNECT_ATTEMPTS {
                    println!("[Harness] Detected recoverable exit, attempting reconnection...");

                    // Emit reconnection status
                    let _ =
                        emit_status("Session terminated, reconnecting...", "#FFA500", "running");

                    // Look up task from DB to get reconnection info
                    let task = {
                        let conn = state.db.lock().map_err(|e| e.to_string())?;
                        db::list_tasks(&conn)
                            .map_err(|e| e.to_string())?
                            .into_iter()
                            .find(|t| t.id == task_id)
                    };

                    let task = match task {
                        Some(t) => t,
                        None => {
                            let (formatted_error, _) = format_agent_error(&error_str);
                            return Err(format!("session/prompt failed: {}", formatted_error));
                        }
                    };

                    // Find agent config
                    let agent = match find_agent(&state.config, &task.agent_id) {
                        Some(a) => a,
                        None => {
                            let (formatted_error, _) = format_agent_error(&error_str);
                            return Err(format!("session/prompt failed: {}", formatted_error));
                        }
                    };

                    // Set up working directory
                    let cwd = resolve_task_cwd(&task)?;

                    // Build environment
                    let settings = state.settings.lock().await.clone();
                    let overrides = auth_env_for(&task.agent_id, &settings);
                    let allow_missing = (task.agent_id == "codex"
                        && settings.codex_auth_method.as_deref() == Some("chatgpt"))
                        || (task.agent_id == "claude-code"
                            && matches!(
                                settings.claude_auth_method.as_deref(),
                                Some("cli") | Some("oauth")
                            ));

                    let env = match build_env(&agent.required_env, &overrides, allow_missing) {
                        Ok(e) => e,
                        Err(e) => {
                            return Err(format!("Reconnection failed - auth error: {}", e));
                        }
                    };

                    // Reconnect with context restoration
                    match reconnect_session_with_context(agent, &task, &cwd, &env, &state.db).await
                    {
                        Ok((new_client, new_session_id, _used_session_load)) => {
                            println!(
                                "[Harness] Session reconnected after termination: {}",
                                new_session_id
                            );

                            {
                                let mut handle = handle_ref.lock().await;
                                handle.client = new_client.clone();
                                handle.session_id = new_session_id.clone();

                                // Also update Claude watcher if applicable
                                if task.agent_id == "claude-code" {
                                    handle.claude_watcher =
                                        Some(claude_usage_watcher::start_watching(
                                            &new_session_id,
                                            &task_id,
                                            app.clone(),
                                            state.db.clone(),
                                        ));
                                }
                            }

                            client = new_client;
                            session_id = new_session_id.clone();

                            // Emit status and retry
                            let _ = emit_status("Reconnected, retrying...", "#4ade80", "running");
                            continue; // Retry the prompt
                        }
                        Err(reconnect_err) => {
                            let (formatted_error, _) = format_agent_error(&error_str);
                            return Err(format!(
                                "Session terminated and reconnection failed: {} (reconnect error: {})",
                                formatted_error, reconnect_err
                            ));
                        }
                    }
                } else {
                    // Not recoverable or max attempts reached
                    let (formatted_error, _) = format_agent_error(&error_str);
                    return Err(format!("session/prompt failed: {}", formatted_error));
                }
            }
        }
    };

    // Drop the sender to signal completion, then wait for emit task
    drop(stream_tx);
    let _ = stream_emit_handle.await;

    // Store and process response messages
    for msg in &response.messages {
        let msg_timestamp = chrono::Utc::now().to_rfc3339();

        // Build chat message JSON
        let chat_msg = serde_json::json!({
            "message_type": msg.message_type,
            "content": msg.content,
            "reasoning": msg.reasoning,
            "tool_call": msg.name.as_ref().map(|name| serde_json::json!({
                "name": name,
                "arguments": msg.arguments
            })),
            "tool_return": msg.tool_return,
            "timestamp": msg_timestamp
        });

        // Store message in memory
        {
            let mut handle = handle_ref.lock().await;
            handle.messages.push(serde_json::json!({
                "message_type": msg.message_type,
                "content": msg.content,
                "reasoning": msg.reasoning,
                "name": msg.name,
                "arguments": msg.arguments,
                "tool_return": msg.tool_return,
                "timestamp": msg_timestamp
            }));
        }

        // Persist to DB
        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db::save_message(
                &conn,
                &task_id,
                &msg.message_type,
                msg.content.as_deref(),
                msg.reasoning.as_deref(),
                msg.name.as_deref(),
                msg.arguments.as_deref(),
                msg.tool_return.as_deref(),
                &msg_timestamp,
            );
        }

        // Emit to chat window
        if let Some(chat_window) = app.get_webview_window(&chat_window_label) {
            let _ = chat_window.emit("ChatLogUpdate", (&task_id, &chat_msg));
        }

        let status = match msg.message_type.as_str() {
            "assistant_message" => msg.content.clone().unwrap_or_default(),
            "reasoning_message" => format!("Thinking: {}", msg.reasoning.as_deref().unwrap_or("")),
            "tool_call_message" => format!("Running: {}", msg.name.as_deref().unwrap_or("tool")),
            "tool_return_message" => "Tool completed".to_string(),
            _ => continue,
        };
        let color = if msg.message_type == "tool_call_message" {
            "yellow"
        } else {
            "white"
        };
        emit_status(&status, color, "running")?;
    }

    // Process token usage and update cost
    if let Some(usage) = &response.token_usage {
        let cost = calculate_cost_from_usage(&model, usage);
        if cost > 0.0 {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let current_cost = db::get_task_cost(&conn, &task_id).unwrap_or(0.0);
            let new_total = current_cost + cost;
            let _ = db::update_task_cost(&conn, &task_id, new_total);
            if let Some(window) = window_ref {
                window
                    .emit("CostUpdate", (&task_id, new_total))
                    .map_err(|e| e.to_string())?;
            } else if let Some(main_window) = app.get_webview_window("main") {
                let _ = main_window.emit("CostUpdate", (&task_id, new_total));
            }
        }
        // Emit token usage for context indicator and save to database
        if let Some(window) = window_ref {
            window
                .emit("TokenUsageUpdate", (&task_id, usage))
                .map_err(|e| e.to_string())?;
        } else if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.emit("TokenUsageUpdate", (&task_id, usage));
        }
        // Persist token usage to database for restart recovery
        let total_tokens = usage.total_token_usage.total_tokens;
        let context_window = usage.model_context_window;
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db::update_task_token_usage(&conn, &task_id, total_tokens, context_window);
    }

    if let Some(new_session_id) = response.session_id.as_ref() {
        let mut handle = handle_ref.lock().await;
        if new_session_id != &handle.session_id {
            handle.session_id = new_session_id.clone();
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db::update_task_agent_session_id(&conn, &task_id, new_session_id);
            if agent_id == "claude-code" {
                handle.claude_watcher = Some(claude_usage_watcher::start_watching(
                    new_session_id,
                    &task_id,
                    app.clone(),
                    state.db.clone(),
                ));
            }
        }
    }

    // Check if the generation was cancelled (soft stop)
    let was_cancelled = cancel_token.is_cancelled();

    if was_cancelled {
        // Generation was stopped by user - emit GenerationStopped and set status to Ready
        println!(
            "[Harness] Generation was cancelled for task_id={}, emitting GenerationStopped",
            task_id
        );

        emit_status("Ready", "#04d885", "idle")?;

        // Emit GenerationStopped to chat window
        if let Some(chat_window) = app.get_webview_window(&chat_window_label) {
            let _ = chat_window.emit("GenerationStopped", &task_id);
            let _ = chat_window.emit("ChatLogStatus", (&task_id, "Ready", "idle"));
        }

        // Update DB status to Ready (session still alive)
        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db::update_task_status(&conn, &task_id, "Ready", "idle");
        }
    } else {
        // Normal completion
        let final_status = response
            .messages
            .iter()
            .filter(|m| m.message_type == "assistant_message")
            .filter_map(|m| m.content.as_ref())
            .last()
            .map(|s| truncate_str(s, 50))
            .unwrap_or_else(|| "Completed".to_string());
        let preview_source = response
            .messages
            .iter()
            .filter(|m| m.message_type == "assistant_message")
            .filter_map(|m| m.content.as_ref())
            .last()
            .cloned()
            .unwrap_or_else(|| final_status.clone());

        let summary_status =
            summarize_status_for_notifications(state, &agent_id, &preview_source, &final_status)
                .await;

        post_discord_assistant_message(state, &task_id, &preview_source).await;

        emit_status(&summary_status, "#04d885", "completed")?;

        // Emit completion status to chat window
        if let Some(chat_window) = app.get_webview_window(&chat_window_label) {
            let _ = chat_window.emit("ChatLogStatus", (&task_id, &summary_status, "completed"));
        }

        // Update DB status to completed (with summary)
        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db::update_task_status(&conn, &task_id, &summary_status, "completed");
        }

        let _ =
            maybe_show_agent_notification(&app, state, &task_id, &agent_id, &summary_status).await;
    }

    Ok(())
}

#[tauri::command]
async fn stop_task(
    task_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    stop_task_internal(task_id, state.inner(), app).await
}

pub(crate) async fn stop_task_internal(
    task_id: String,
    state: &AppState,
    app: tauri::AppHandle,
) -> Result<(), String> {
    println!("[Harness] stop_task: task_id={}", task_id);

    let handle_ref = {
        let mut sessions = state.sessions.lock().await;
        sessions.remove(&task_id)
    };

    if let Some(handle_ref) = handle_ref {
        let (client, claude_watcher) = {
            let mut handle = handle_ref.lock().await;
            (handle.client.clone(), handle.claude_watcher.take())
        };
        if let Some(watcher) = claude_watcher {
            watcher.stop().await;
        }
        let _ = client.shutdown().await;
    }

    {
        let mut pending = state.pending_user_inputs.lock().await;
        pending.remove(&task_id);
    }

    {
        let mut running = state.running_tasks.lock().await;
        running.remove(&task_id);
    }

    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db::update_task_status(&conn, &task_id, "Stopped", "idle");
    }

    let window_label = format!(
        "chat-{}",
        task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_")
    );
    if let Some(window) = app.get_webview_window(&window_label) {
        let _ = window.emit("ChatLogStatus", (&task_id, "Stopped", "idle"));
    }
    if let Some(main_window) = app.get_webview_window("main") {
        let _ = main_window.emit("StatusUpdate", (&task_id, "Stopped", "red", "idle"));
    }

    Ok(())
}

/// Soft stop: cancel the current generation without killing the session.
/// This allows users to stop the current response but continue chatting later.
#[tauri::command]
async fn soft_stop_task(
    task_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    soft_stop_task_internal(task_id, state.inner(), app).await
}

pub(crate) async fn soft_stop_task_internal(
    task_id: String,
    state: &AppState,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    println!("[Harness] soft_stop_task: task_id={}", task_id);

    // Get the session handle without removing it (session stays alive)
    let handle_ref = {
        let sessions = state.sessions.lock().await;
        sessions.get(&task_id).cloned()
    };

    if let Some(handle_ref) = handle_ref {
        // Cancel the current generation
        let handle = handle_ref.lock().await;
        handle.cancel_token.cancel();
        println!("[Harness] Cancelled generation for task_id={}", task_id);
    } else {
        println!(
            "[Harness] soft_stop_task: no session found for task_id={}",
            task_id
        );
    }

    // Note: We don't update status or emit events here.
    // The start_task_internal function will detect the cancellation and handle:
    // - Emitting GenerationStopped to the chat window
    // - Updating the status to "Ready"
    // - Removing from running_tasks
    // This ensures the in-flight response completes before we update the UI.

    Ok(())
}

/// Start a pending prompt from the chat log window.
/// This is called when user clicks "Start Session & Send" on a draft message.
/// Reuses start_task logic - it handles session creation/reconnection and sending the pending prompt.
#[tauri::command]
async fn start_pending_prompt(
    task_id: String,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    println!("[Harness] start_pending_prompt: task_id={}", task_id);
    // Reuse start_task logic - it already handles:
    // - Session creation/reconnection
    // - Sending pending prompt
    // - Streaming responses
    // - Emitting to the calling window
    start_task_internal(
        task_id,
        state.inner(),
        window.app_handle().clone(),
        Some(window),
    )
    .await
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
async fn save_settings(
    settings: Settings,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let prev = state.settings.lock().await.clone();
    let mut next = settings;
    if next.codex_auth_method.is_none()
        && next
            .openai_api_key
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    {
        next.codex_auth_method = Some("api".to_string());
    }
    if next.claude_auth_method.is_none()
        && next
            .anthropic_api_key
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    {
        next.claude_auth_method = Some("api".to_string());
    }

    // Validate and normalize summaries_agent setting
    // "auto" means use the task agent, which is represented as None internally
    const VALID_SUMMARIES_AGENTS: &[&str] = &["amp", "codex", "claude-code"];
    if let Some(ref agent) = next.summaries_agent {
        let normalized = agent.trim().to_lowercase();
        if normalized == "auto" || normalized.is_empty() {
            // Normalize "auto" to None (use task agent)
            next.summaries_agent = None;
        } else if !VALID_SUMMARIES_AGENTS.contains(&normalized.as_str()) {
            return Err(format!(
                "Invalid summaries_agent '{}'. Valid values are: auto, {}",
                agent,
                VALID_SUMMARIES_AGENTS.join(", ")
            ));
        } else {
            // Store the normalized (lowercase) value
            next.summaries_agent = Some(normalized);
        }
    }

    ensure_mcp_settings(&mut next);

    persist_settings(&next)?;
    {
        let mut locked = state.settings.lock().await;
        *locked = next.clone();
    }

    let discord_config_changed = prev.discord_enabled != next.discord_enabled
        || prev.discord_bot_token != next.discord_bot_token
        || prev.discord_channel_id != next.discord_channel_id;
    if discord_config_changed {
        stop_discord_bot(state.inner()).await;
        ensure_discord_bot(&app, state.inner(), &next).await;
    }

    Ok(())
}

#[tauri::command]
async fn test_webhook(state: State<'_, AppState>) -> Result<String, String> {
    let settings = state.settings.lock().await;
    let webhook_url = settings
        .webhook
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "No webhook URL configured. Add one in Settings.".to_string())?
        .clone();
    drop(settings); // Release lock before async call

    let payload = webhook::build_test_payload();
    webhook::send_webhook(&webhook_url, &payload).await?;
    Ok("Webhook sent successfully!".to_string())
}

#[tauri::command]
async fn test_discord(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<String, String> {
    let settings = state.settings.lock().await.clone();
    if !discord_enabled(&settings) {
        return Err("Discord bot is disabled in settings".to_string());
    }
    ensure_discord_bot(&app, state.inner(), &settings).await;
    let handle =
        discord_handle(state.inner()).ok_or_else(|| "Discord bot not running".to_string())?;
    handle
        .send_channel_message("Phantom Harness Discord test message")
        .await?;
    Ok("Discord test sent".to_string())
}

/// Get the availability status of all agents
#[tauri::command]
fn get_agent_availability(
    state: State<'_, AppState>,
) -> Result<HashMap<String, AgentAvailability>, String> {
    let latest = build_agent_availability(&state.config);
    let mut avail = state.agent_availability.lock().map_err(|e| e.to_string())?;
    *avail = latest.clone();
    Ok(latest)
}

/// Refresh agent availability and emit UI updates
#[tauri::command]
fn refresh_agent_availability(
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<HashMap<String, AgentAvailability>, String> {
    let latest = build_agent_availability(&state.config);
    {
        let mut avail = state.agent_availability.lock().map_err(|e| e.to_string())?;
        *avail = latest.clone();
    }
    for (agent_id, status) in &latest {
        let _ = window.emit(
            "AgentAvailabilityUpdate",
            (
                agent_id.clone(),
                status.available,
                status.error_message.clone(),
            ),
        );
    }
    Ok(latest)
}

/// Get the display name for an agent from the config
fn get_agent_display_name(config: &AgentsConfig, agent_id: &str) -> String {
    config
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .and_then(|a| a.display_name.clone())
        .unwrap_or_else(|| agent_id.to_string())
}

/// Represents an agent skill parsed from a SKILL.md file
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentSkill {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    triggers: Option<Vec<String>>,
    source: String,   // "personal" or "project"
    enabled: bool,    // Whether the skill is currently enabled
    path: String,     // Full path to the skill directory (for move operations)
    can_toggle: bool, // false for project skills (read-only)
}

/// Parse YAML frontmatter from a SKILL.md file
fn parse_skill_frontmatter(content: &str) -> Option<AgentSkill> {
    // SKILL.md files use YAML frontmatter between --- markers
    if !content.starts_with("---") {
        return None;
    }

    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() < 3 {
        return None;
    }

    let yaml_content = parts[1].trim();

    // Parse basic fields from YAML
    let mut name = None;
    let mut description = None;
    let mut triggers = Vec::new();
    let mut in_triggers = false;

    for line in yaml_content.lines() {
        let line = line.trim();
        if line.starts_with("name:") {
            name = Some(line.trim_start_matches("name:").trim().to_string());
            in_triggers = false;
        } else if line.starts_with("description:") {
            description = Some(line.trim_start_matches("description:").trim().to_string());
            in_triggers = false;
        } else if line.starts_with("triggers:") {
            in_triggers = true;
        } else if in_triggers && line.starts_with("- ") {
            triggers.push(line.trim_start_matches("- ").trim().to_string());
        } else if !line.starts_with("- ") && !line.is_empty() {
            in_triggers = false;
        }
    }

    Some(AgentSkill {
        name: name?,
        description: description.unwrap_or_default(),
        triggers: if triggers.is_empty() {
            None
        } else {
            Some(triggers)
        },
        source: String::new(), // Will be set by caller
        enabled: true,         // Will be set by caller
        path: String::new(),   // Will be set by caller
        can_toggle: true,      // Will be set by caller
    })
}

/// Scan a directory for SKILL.md files
fn scan_skills_directory(
    dir: &Path,
    source: &str,
    enabled: bool,
    can_toggle: bool,
) -> Vec<AgentSkill> {
    let mut skills = Vec::new();

    if !dir.exists() {
        return skills;
    }

    // Skills are in subdirectories: ~/.claude/skills/{skill-name}/SKILL.md
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&skill_file) {
                        if let Some(mut skill) = parse_skill_frontmatter(&content) {
                            skill.source = source.to_string();
                            skill.enabled = enabled;
                            skill.path = path.to_string_lossy().to_string();
                            skill.can_toggle = can_toggle;
                            skills.push(skill);
                        }
                    }
                }
            }
        }
    }

    skills
}

/// Get the directory for storing disabled skills for an agent
fn disabled_skills_dir(agent_id: &str) -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir().ok_or("Could not determine config directory")?;
    Ok(config_dir
        .join("phantom-harness")
        .join("disabled-skills")
        .join(agent_id))
}

/// Get the primary skills directory for an agent
fn get_skills_dirs(agent_id: &str, home: &Path) -> Vec<PathBuf> {
    match agent_id {
        "claude" => vec![
            home.join(".claude").join("skills"),
            home.join(".factory").join("skills"),
        ],
        "codex" => vec![home.join(".codex").join("skills")],
        _ => vec![],
    }
}

/// Scan the disabled skills directory for an agent
fn scan_disabled_skills_directory(agent_id: &str) -> Vec<AgentSkill> {
    let disabled_dir = match disabled_skills_dir(agent_id) {
        Ok(dir) => dir,
        Err(_) => return Vec::new(),
    };

    // Disabled skills have enabled=false, and they're all personal (can_toggle=true)
    scan_skills_directory(&disabled_dir, "personal", false, true)
}

fn read_claude_plugin_skills(plugins_dir: &Path) -> Result<Vec<AgentSkill>, std::io::Error> {
    let installed_path = plugins_dir.join("installed_plugins.json");
    if !installed_path.exists() {
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(&installed_path)?;
    let installed: ClaudeInstalledPlugins = serde_json::from_str(&contents)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

    let mut skills = Vec::new();
    for installs in installed.plugins.values() {
        for install in installs {
            let scope = install.scope.as_deref().unwrap_or("user").to_string();
            let source = if scope == "project" {
                "project"
            } else {
                "personal"
            };
            let install_path = PathBuf::from(&install.install_path);
            let skill_dirs = [
                install_path.join("skills"),
                install_path.join(".claude").join("skills"),
            ];
            for dir in skill_dirs {
                skills.extend(scan_skills_directory(&dir, source, true, false));
            }
        }
    }

    Ok(skills)
}

#[tauri::command]
async fn get_agent_skills(
    agent_id: String,
    project_path: Option<String>,
) -> Result<Vec<AgentSkill>, String> {
    let mut all_skills = Vec::new();
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;

    // Skill directories vary by agent
    match agent_id.as_str() {
        "claude" => {
            // Claude Code skills
            // Personal: ~/.claude/skills/, ~/.factory/skills/
            all_skills.extend(scan_skills_directory(
                &home.join(".claude").join("skills"),
                "personal",
                true,
                true,
            ));
            all_skills.extend(scan_skills_directory(
                &home.join(".factory").join("skills"),
                "personal",
                true,
                true,
            ));

            // Project: .claude/skills/, .factory/skills/
            if let Some(ref proj_path) = project_path {
                let proj = PathBuf::from(proj_path);
                all_skills.extend(scan_skills_directory(
                    &proj.join(".claude").join("skills"),
                    "project",
                    true,
                    false,
                ));
                all_skills.extend(scan_skills_directory(
                    &proj.join(".factory").join("skills"),
                    "project",
                    true,
                    false,
                ));
            }

            match read_claude_plugin_skills(&home.join(".claude").join("plugins")) {
                Ok(mut plugin_skills) => all_skills.append(&mut plugin_skills),
                Err(err) => eprintln!("[Harness] Failed to read Claude plugin skills: {}", err),
            }

            // Disabled skills
            all_skills.extend(scan_disabled_skills_directory("claude"));
        }
        "codex" => {
            // Codex skills
            // Personal: ~/.codex/skills/
            all_skills.extend(scan_skills_directory(
                &home.join(".codex").join("skills"),
                "personal",
                true,
                true,
            ));

            // Project: .codex/skills/
            if let Some(ref proj_path) = project_path {
                let proj = PathBuf::from(proj_path);
                all_skills.extend(scan_skills_directory(
                    &proj.join(".codex").join("skills"),
                    "project",
                    true,
                    false,
                ));
            }

            // Disabled skills
            all_skills.extend(scan_disabled_skills_directory("codex"));
        }
        _ => {
            // Unknown agent - no skills support
            return Ok(all_skills);
        }
    }

    // Deduplicate by name (keep first occurrence, which prioritizes project over personal due to order)
    let mut seen = std::collections::HashSet::new();
    all_skills.retain(|skill| seen.insert(skill.name.clone()));

    // Sort by name for consistent display
    all_skills.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(all_skills)
}

/// Toggle a skill on or off by moving it between the skills directory and disabled-skills directory
#[tauri::command]
async fn toggle_skill(agent_id: String, skill_name: String, enabled: bool) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    let disabled_dir = disabled_skills_dir(&agent_id)?;

    // Get the primary skills directories for this agent
    let skills_dirs = get_skills_dirs(&agent_id, &home);
    if skills_dirs.is_empty() {
        return Err(format!("Unknown agent: {}", agent_id));
    }

    if enabled {
        // Moving from disabled to enabled
        // The skill is currently in the disabled directory
        let source_path = disabled_dir.join(&skill_name);
        if !source_path.exists() {
            return Err(format!(
                "Skill '{}' not found in disabled skills",
                skill_name
            ));
        }

        // Move to the first skills directory for this agent
        let target_dir = &skills_dirs[0];
        std::fs::create_dir_all(target_dir)
            .map_err(|e| format!("Failed to create skills directory: {}", e))?;

        let target_path = target_dir.join(&skill_name);

        // Move the skill folder
        std::fs::rename(&source_path, &target_path)
            .map_err(|e| format!("Failed to move skill: {}", e))?;

        println!(
            "[Skills] Enabled skill '{}' for agent '{}': {:?} -> {:?}",
            skill_name, agent_id, source_path, target_path
        );
    } else {
        // Moving from enabled to disabled
        // Find the skill in any of the skills directories
        let mut source_path = None;
        for dir in &skills_dirs {
            let candidate = dir.join(&skill_name);
            if candidate.exists() {
                source_path = Some(candidate);
                break;
            }
        }

        let source_path = source_path
            .ok_or_else(|| format!("Skill '{}' not found in any skills directory", skill_name))?;

        // Create disabled directory if needed
        std::fs::create_dir_all(&disabled_dir)
            .map_err(|e| format!("Failed to create disabled skills directory: {}", e))?;

        let target_path = disabled_dir.join(&skill_name);

        // Move the skill folder
        std::fs::rename(&source_path, &target_path)
            .map_err(|e| format!("Failed to move skill: {}", e))?;

        println!(
            "[Skills] Disabled skill '{}' for agent '{}': {:?} -> {:?}",
            skill_name, agent_id, source_path, target_path
        );
    }

    Ok(())
}

/// Get list of tasks that are currently running
#[tauri::command]
fn get_running_tasks(state: State<'_, AppState>) -> Result<Vec<db::TaskRecord>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let all_tasks = db::list_tasks(&conn).map_err(|e| e.to_string())?;

    // Filter to only running tasks
    let running: Vec<db::TaskRecord> = all_tasks
        .into_iter()
        .filter(|t| t.status_state == "running")
        .collect();

    Ok(running)
}

/// Restart all agents by terminating their sessions
/// This is used after skill changes to ensure agents reload their skills
#[tauri::command]
async fn restart_all_agents(
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<Vec<String>, String> {
    println!("[Harness] Restarting all agents to apply skill changes...");

    let mut restarted_task_ids = Vec::new();

    // Get all active sessions and remove them
    let shutdown_targets: Vec<(String, SharedSessionHandle)> = {
        let mut sessions = state.sessions.lock().await;
        let task_ids: Vec<String> = sessions.keys().cloned().collect();
        let mut targets = Vec::new();

        for task_id in task_ids {
            if let Some(handle_ref) = sessions.remove(&task_id) {
                targets.push((task_id, handle_ref));
            }
        }

        targets
    };

    for (task_id, handle_ref) in shutdown_targets {
        let (agent_id, client) = {
            let handle = handle_ref.lock().await;
            (handle.agent_id.clone(), handle.client.clone())
        };

        println!(
            "[Harness] Terminating session for task: {} (agent: {})",
            task_id, agent_id
        );

        // Emit status update for this task
        let _ = window.emit(
            "StatusUpdate",
            (
                &task_id,
                "Restarting for skill changes...",
                "yellow",
                "idle",
            ),
        );

        // Update task status in DB
        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db::update_task_status(&conn, &task_id, "Ready (skills updated)", "idle");
        }

        restarted_task_ids.push(task_id);

        let _ = client.shutdown().await;
    }

    // Emit AgentAvailabilityUpdate to trigger UI refresh
    let _ = window.emit("AgentAvailabilityUpdate", ());

    println!(
        "[Harness] Restarted {} agent sessions",
        restarted_task_ids.len()
    );

    Ok(restarted_task_ids)
}

#[tauri::command]
async fn codex_login(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<CodexAuthStatus, String> {
    let codex_cmd = resolve_codex_command();
    println!("[Codex Login] Starting login with command: {}", codex_cmd);
    let app_handle = app.clone();
    let settings_handle = state.settings.clone();

    let mut child = tokio::process::Command::new(&codex_cmd)
        .arg("login")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            println!("[Codex Login] Failed to spawn: {}", err);
            format!("codex login failed: {}", err)
        })?;

    println!("[Codex Login] Process spawned successfully");

    // Codex writes the OAuth URL to stderr, not stdout
    let stderr = child.stderr.take();

    // Open browser when OAuth URL is printed
    if let Some(stderr) = stderr {
        let mut lines = BufReader::new(stderr).lines();
        let mut opened = false;
        while let Ok(Some(line)) = lines.next_line().await {
            println!("[Codex Login] stderr line: {}", line);
            if opened {
                continue;
            }
            for token in line.split_whitespace() {
                // Security: Validate URL against known OAuth providers
                if token.starts_with("https://") {
                    println!("[Codex Login] Found URL: {}", token);
                    if validate_oauth_url(token) {
                        println!("[Codex Login] URL validated, opening browser");
                        use tauri_plugin_opener::OpenerExt;
                        match app_handle.opener().open_url(token, None::<&str>) {
                            Ok(_) => println!("[Codex Login] Browser opened successfully"),
                            Err(e) => println!("[Codex Login] Failed to open browser: {:?}", e),
                        }
                        opened = true;
                        break;
                    } else {
                        println!("[Codex Login] URL failed validation");
                    }
                }
            }
        }
    } else {
        println!("[Codex Login] No stderr available");
    }

    // Wait with 5-minute timeout to prevent indefinite hangs
    let result = timeout(Duration::from_secs(300), child.wait()).await;

    match result {
        Ok(Ok(status)) if status.success() => {
            // Update settings with auth method
            let mut settings = settings_handle.lock().await;
            settings.codex_auth_method = Some("chatgpt".to_string());
            drop(settings); // Release lock before persist

            let settings = settings_handle.lock().await;
            persist_settings(&settings)?;
            drop(settings);

            // Return fresh auth status
            check_codex_auth().await
        }
        Ok(Ok(_)) => Err("Login cancelled or failed".to_string()),
        Ok(Err(e)) => Err(format!("Process error: {}", e)),
        Err(_) => {
            // Timeout - kill the process
            let _ = child.kill().await;
            Err("Login timed out after 5 minutes".to_string())
        }
    }
}

#[tauri::command]
async fn claude_login(
    state: State<'_, AppState>,
    _app: tauri::AppHandle,
) -> Result<ClaudeAuthStatus, String> {
    let settings_handle = state.settings.clone();

    // First check if already authenticated
    let current_status = check_claude_auth().await?;
    if current_status.authenticated {
        // Already logged in, just update settings and return
        let mut settings = settings_handle.lock().await;
        settings.claude_auth_method = Some("oauth".to_string());
        persist_settings(&settings)?;
        return Ok(current_status);
    }

    let claude_cmd = resolve_claude_command();

    // Open Terminal with claude setup-token for interactive login
    // This allows the user to complete the OAuth flow in a visible terminal
    let script = format!(
        r#"tell application "Terminal"
            do script "{} setup-token"
        end tell"#,
        claude_cmd.replace("\"", "\\\"")
    );

    tokio::process::Command::new("osascript")
        .args(["-e", &script])
        .spawn()
        .map_err(|err| format!("Failed to open Terminal: {}", err))?;

    // Wait a bit then poll for auth status (user completes flow in terminal)
    // Poll every 2 seconds for up to 5 minutes
    for _ in 0..150 {
        tokio::time::sleep(Duration::from_secs(2)).await;

        let status = check_claude_auth().await?;
        if status.authenticated {
            // Update settings
            let mut settings = settings_handle.lock().await;
            settings.claude_auth_method = Some("oauth".to_string());
            persist_settings(&settings)?;
            return Ok(status);
        }
    }

    // Timeout - user didn't complete login
    Ok(ClaudeAuthStatus {
        authenticated: false,
        method: None,
        expires_at: None,
        email: None,
    })
}

#[tauri::command]
async fn check_codex_auth() -> Result<CodexAuthStatus, String> {
    let auth_path = dirs::home_dir()
        .ok_or("home dir unavailable")?
        .join(".codex")
        .join("auth.json");

    if !auth_path.exists() {
        return Ok(CodexAuthStatus {
            authenticated: false,
            method: None,
            expires_at: None,
            email: None,
        });
    }

    // Security: Verify file permissions on Unix (should be 0600)
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(metadata) = std::fs::metadata(&auth_path) {
            let mode = metadata.mode() & 0o777;
            if mode != 0o600 {
                eprintln!("[Security] auth.json has insecure permissions: {:o}", mode);
            }
        }
    }

    let content =
        std::fs::read_to_string(&auth_path).map_err(|e| format!("read auth.json: {}", e))?;

    // Security: Size limit to prevent DoS from malformed files
    if content.len() > 1_000_000 {
        return Err("auth.json exceeds size limit".to_string());
    }

    // Parse auth.json to check validity - don't leak parse details
    let auth: serde_json::Value =
        serde_json::from_str(&content).map_err(|_| "Invalid auth.json format")?;

    // Check for tokens.access_token (the actual codex auth.json structure)
    let access_token = auth
        .get("tokens")
        .and_then(|t| t.get("access_token"))
        .and_then(|v| v.as_str());

    let has_token = access_token.map(|s| !s.is_empty()).unwrap_or(false);

    // Extract email from the JWT access token
    let email = access_token.and_then(extract_email_from_jwt);

    Ok(CodexAuthStatus {
        authenticated: has_token,
        method: Some("chatgpt".to_string()),
        expires_at: auth
            .get("last_refresh")
            .and_then(|v| v.as_str())
            .map(String::from),
        email,
    })
}

#[tauri::command]
async fn codex_rate_limits(_state: State<'_, AppState>) -> Result<RateLimits, String> {
    println!("[Harness] codex_rate_limits called");

    // Read auth.json to get access token and account ID
    let auth_path = dirs::home_dir()
        .ok_or("home dir unavailable")?
        .join(".codex")
        .join("auth.json");

    if !auth_path.exists() {
        return Ok(RateLimits {
            primary: None,
            secondary: None,
            plan_type: None,
            not_available: Some(true),
            error_message: Some("Not authenticated with Codex".to_string()),
        });
    }

    let content =
        std::fs::read_to_string(&auth_path).map_err(|e| format!("read auth.json: {}", e))?;

    let auth: serde_json::Value =
        serde_json::from_str(&content).map_err(|_| "Invalid auth.json format")?;

    let access_token = auth
        .get("tokens")
        .and_then(|t| t.get("access_token"))
        .and_then(|v| v.as_str())
        .ok_or("No access token found")?;

    let account_id = auth
        .get("tokens")
        .and_then(|t| t.get("account_id"))
        .and_then(|v| v.as_str())
        .ok_or("No account ID found")?;

    println!("[Harness] Fetching rate limits from ChatGPT API...");

    // Call ChatGPT backend API directly
    let client = reqwest::Client::new();
    let response = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "codex-cli")
        .header("ChatGPT-Account-Id", account_id)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        println!("[Harness] Rate limits API returned status: {}", status);
        return Ok(RateLimits {
            primary: None,
            secondary: None,
            plan_type: None,
            not_available: Some(true),
            error_message: Some(format!("API returned status {}", status)),
        });
    }

    let api_result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Parse the ChatGPT API response format
    let rate_limit = api_result.get("rate_limit");
    let plan_type = api_result
        .get("plan_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let primary = rate_limit
        .and_then(|r| r.get("primary_window"))
        .and_then(|p| {
            let used_percent = p.get("used_percent")?.as_f64()?;
            let window_secs = p.get("limit_window_seconds")?.as_i64()?;
            let reset_at = p.get("reset_at").and_then(|v| v.as_i64());

            Some(RateLimitWindow {
                used_percent,
                window_duration_mins: (window_secs / 60) as i32,
                resets_at: reset_at.map(|ts| {
                    // Convert Unix timestamp to ISO string
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| ts.to_string())
                }),
            })
        });

    let secondary = rate_limit
        .and_then(|r| r.get("secondary_window"))
        .and_then(|s| {
            let used_percent = s.get("used_percent")?.as_f64()?;
            let window_secs = s.get("limit_window_seconds")?.as_i64()?;
            let reset_at = s.get("reset_at").and_then(|v| v.as_i64());

            Some(RateLimitWindow {
                used_percent,
                window_duration_mins: (window_secs / 60) as i32,
                resets_at: reset_at.map(|ts| {
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| ts.to_string())
                }),
            })
        });

    let result = RateLimits {
        primary,
        secondary,
        plan_type,
        not_available: None,
        error_message: None,
    };

    println!(
        "[Harness] Returning rate limits: primary={:?}, secondary={:?}, plan={:?}",
        result.primary, result.secondary, result.plan_type
    );

    Ok(result)
}

#[tauri::command]
async fn codex_logout(state: State<'_, AppState>) -> Result<(), String> {
    let codex_cmd = resolve_codex_command();

    tokio::process::Command::new(&codex_cmd)
        .arg("logout")
        .status()
        .await
        .map_err(|e| format!("logout failed: {}", e))?;

    let mut settings = state.settings.lock().await;
    settings.codex_auth_method = None;
    persist_settings(&settings)?;

    Ok(())
}

fn get_claude_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".claude.json"))
}

#[tauri::command]
async fn check_claude_auth() -> Result<ClaudeAuthStatus, String> {
    // Primary check: ~/.claude.json with oauthAccount
    if let Some(config_path) = get_claude_config_path() {
        if config_path.exists() {
            let content = match std::fs::read_to_string(&config_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[Claude Auth] Failed to read config: {}", e);
                    return Ok(ClaudeAuthStatus {
                        authenticated: false,
                        method: None,
                        expires_at: None,
                        email: None,
                    });
                }
            };

            // Security: Size limit (config can be larger than credentials)
            if content.len() > 10_000_000 {
                return Err("Config file exceeds size limit".to_string());
            }

            let config: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[Claude Auth] Failed to parse config: {}", e);
                    return Ok(ClaudeAuthStatus {
                        authenticated: false,
                        method: None,
                        expires_at: None,
                        email: None,
                    });
                }
            };

            // Check for oauthAccount.emailAddress - this indicates successful OAuth login
            let email = config
                .get("oauthAccount")
                .and_then(|o| o.get("emailAddress"))
                .and_then(|v| v.as_str())
                .map(String::from);

            if let Some(email_addr) = email {
                if !email_addr.is_empty() {
                    return Ok(ClaudeAuthStatus {
                        authenticated: true,
                        method: Some("oauth".to_string()),
                        expires_at: None, // OAuth tokens managed by Claude Code
                        email: Some(email_addr),
                    });
                }
            }

            // Also check hasAvailableSubscription as a secondary indicator
            let has_subscription = config
                .get("hasAvailableSubscription")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if has_subscription {
                return Ok(ClaudeAuthStatus {
                    authenticated: true,
                    method: Some("oauth".to_string()),
                    expires_at: None,
                    email: None,
                });
            }
        }
    }

    Ok(ClaudeAuthStatus {
        authenticated: false,
        method: None,
        expires_at: None,
        email: None,
    })
}

/// How often to re-check credentials from disk/keychain (5 minutes)
const CLAUDE_TOKEN_CACHE_TTL_SECS: u64 = 300;

/// Buffer before expiration to trigger refresh (5 minutes)
const TOKEN_EXPIRY_BUFFER_SECS: i64 = 300;

#[derive(Clone)]
struct ClaudeOAuthTokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>, // Unix timestamp in seconds
}

struct ClaudeTokenCache {
    tokens: Option<ClaudeOAuthTokens>,
    last_checked: Option<Instant>,
}

static CLAUDE_TOKEN_CACHE: OnceLock<StdMutex<ClaudeTokenCache>> = OnceLock::new();

fn claude_token_cache() -> &'static StdMutex<ClaudeTokenCache> {
    CLAUDE_TOKEN_CACHE.get_or_init(|| {
        StdMutex::new(ClaudeTokenCache {
            tokens: None,
            last_checked: None,
        })
    })
}

/// Invalidate the Claude token cache (call after refresh or when token is known to be invalid)
fn invalidate_claude_token_cache() {
    if let Ok(mut cache) = claude_token_cache().lock() {
        cache.tokens = None;
        cache.last_checked = None;
    }
}

/// Get Claude OAuth token from credentials file or keychain (cached).
/// Will automatically refresh expired tokens if a refresh token is available.
pub(crate) fn get_claude_oauth_token() -> Option<String> {
    let now = Instant::now();
    let current_time = chrono::Utc::now().timestamp();

    // Check cache first
    if let Ok(cache) = claude_token_cache().lock() {
        if let Some(last_checked) = cache.last_checked {
            if last_checked.elapsed() < Duration::from_secs(CLAUDE_TOKEN_CACHE_TTL_SECS) {
                if let Some(ref tokens) = cache.tokens {
                    // Check if token is still valid (with buffer)
                    if let Some(expires_at) = tokens.expires_at {
                        if current_time < expires_at - TOKEN_EXPIRY_BUFFER_SECS {
                            return Some(tokens.access_token.clone());
                        }
                        // Token expired or about to expire - need refresh
                        println!(
                            "[Harness] Claude token expired or expiring soon (expires_at: {}, now: {})",
                            expires_at, current_time
                        );
                    } else {
                        // No expiration info, assume valid
                        return Some(tokens.access_token.clone());
                    }
                }
            }
        }
    }

    // Fetch fresh tokens from disk/keychain
    let tokens = fetch_claude_oauth_tokens();

    if let Some(ref tokens) = tokens {
        // Check if we need to refresh
        if let Some(expires_at) = tokens.expires_at {
            if current_time >= expires_at - TOKEN_EXPIRY_BUFFER_SECS {
                println!("[Harness] Token needs refresh, attempting...");
                if let Some(ref refresh_token) = tokens.refresh_token {
                    match refresh_claude_oauth_token(refresh_token) {
                        Ok(new_tokens) => {
                            // Update cache with new tokens
                            if let Ok(mut cache) = claude_token_cache().lock() {
                                let access_token = new_tokens.access_token.clone();
                                cache.tokens = Some(new_tokens);
                                cache.last_checked = Some(now);
                                return Some(access_token);
                            }
                        }
                        Err(e) => {
                            println!("[Harness] Token refresh failed: {}", e);
                            // Fall through to return expired token (will get 401)
                        }
                    }
                } else {
                    println!("[Harness] No refresh token available");
                }
            }
        }
    }

    // Update cache
    if let Ok(mut cache) = claude_token_cache().lock() {
        let access_token = tokens.as_ref().map(|t| t.access_token.clone());
        cache.tokens = tokens;
        cache.last_checked = Some(now);
        return access_token;
    }

    tokens.map(|t| t.access_token)
}

/// Refresh Claude OAuth token using the refresh token
fn refresh_claude_oauth_token(refresh_token: &str) -> Result<ClaudeOAuthTokens, String> {
    println!("[Harness] Refreshing Claude OAuth token...");

    // Use blocking reqwest since this is called from sync context
    let client = reqwest::blocking::Client::new();

    let response = client
        .post("https://console.anthropic.com/v1/oauth/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .map_err(|e| format!("Refresh request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(format!(
            "Token refresh failed with status {}: {}",
            status, body
        ));
    }

    let json: serde_json::Value = response
        .json()
        .map_err(|e| format!("Failed to parse refresh response: {}", e))?;

    let access_token = json
        .get("access_token")
        .and_then(|v: &serde_json::Value| v.as_str())
        .ok_or("No access_token in refresh response")?
        .to_string();

    let new_refresh_token = json
        .get("refresh_token")
        .and_then(|v: &serde_json::Value| v.as_str())
        .map(String::from);

    // Calculate expires_at from expires_in (typically 3600 seconds = 1 hour)
    let expires_at = json
        .get("expires_in")
        .and_then(|v: &serde_json::Value| v.as_i64())
        .map(|secs| chrono::Utc::now().timestamp() + secs);

    let new_tokens = ClaudeOAuthTokens {
        access_token: access_token.clone(),
        refresh_token: new_refresh_token
            .clone()
            .or_else(|| Some(refresh_token.to_string())),
        expires_at,
    };

    // Update keychain with new tokens
    if let Err(e) = update_claude_keychain_tokens(&new_tokens) {
        println!("[Harness] Warning: Failed to update keychain: {}", e);
    }

    println!(
        "[Harness] Token refresh successful, new expiry: {:?}",
        expires_at
    );
    Ok(new_tokens)
}

/// Update Claude tokens in macOS keychain
fn update_claude_keychain_tokens(tokens: &ClaudeOAuthTokens) -> Result<(), String> {
    let username = std::env::var("USER").map_err(|_| "USER env not set")?;

    // First read existing keychain data
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-a",
            &username,
            "-w",
        ])
        .output()
        .map_err(|e| format!("Failed to read keychain: {}", e))?;

    if !output.status.success() {
        return Err("Keychain entry not found".to_string());
    }

    let existing_data = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut keychain_json: serde_json::Value = serde_json::from_str(&existing_data)
        .map_err(|e| format!("Failed to parse keychain data: {}", e))?;

    // Update the claudeAiOauth section
    if let Some(oauth) = keychain_json.get_mut("claudeAiOauth") {
        oauth["accessToken"] = serde_json::json!(tokens.access_token);
        if let Some(ref rt) = tokens.refresh_token {
            oauth["refreshToken"] = serde_json::json!(rt);
        }
        if let Some(exp) = tokens.expires_at {
            oauth["expiresAt"] = serde_json::json!(exp);
        }
    } else {
        // Create claudeAiOauth section if it doesn't exist
        keychain_json["claudeAiOauth"] = serde_json::json!({
            "accessToken": tokens.access_token,
            "refreshToken": tokens.refresh_token,
            "expiresAt": tokens.expires_at,
        });
    }

    let updated_json = serde_json::to_string(&keychain_json)
        .map_err(|e| format!("Failed to serialize keychain data: {}", e))?;

    // Delete existing entry first (security command doesn't have an update)
    let _ = std::process::Command::new("security")
        .args([
            "delete-generic-password",
            "-s",
            "Claude Code-credentials",
            "-a",
            &username,
        ])
        .output();

    // Add updated entry
    let add_output = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-s",
            "Claude Code-credentials",
            "-a",
            &username,
            "-w",
            &updated_json,
            "-U", // Update if exists
        ])
        .output()
        .map_err(|e| format!("Failed to update keychain: {}", e))?;

    if !add_output.status.success() {
        return Err(format!(
            "Keychain update failed: {}",
            String::from_utf8_lossy(&add_output.stderr)
        ));
    }

    // Invalidate cache so next read gets fresh data
    invalidate_claude_token_cache();

    Ok(())
}

/// Fetch Claude OAuth tokens from credentials file or keychain
/// Returns full token structure including refresh token and expiration
fn fetch_claude_oauth_tokens() -> Option<ClaudeOAuthTokens> {
    let home = dirs::home_dir()?;

    // First try ~/.claude/.credentials.json (more common location)
    let credentials_path = home.join(".claude").join(".credentials.json");
    if credentials_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&credentials_path) {
            if content.len() <= 1_000_000 {
                if let Ok(creds) = serde_json::from_str::<serde_json::Value>(&content) {
                    // Try claudeAiOauth structure first
                    if let Some(oauth) = creds.get("claudeAiOauth") {
                        if let Some(token) = oauth.get("accessToken").and_then(|v| v.as_str()) {
                            if !token.is_empty() {
                                return Some(ClaudeOAuthTokens {
                                    access_token: token.to_string(),
                                    refresh_token: oauth
                                        .get("refreshToken")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    expires_at: oauth.get("expiresAt").and_then(|v| v.as_i64()),
                                });
                            }
                        }
                    }
                    // Try flat structure
                    if let Some(token) = creds.get("accessToken").and_then(|v| v.as_str()) {
                        if !token.is_empty() {
                            return Some(ClaudeOAuthTokens {
                                access_token: token.to_string(),
                                refresh_token: creds
                                    .get("refreshToken")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                expires_at: creds.get("expiresAt").and_then(|v| v.as_i64()),
                            });
                        }
                    }
                }
            }
        }
    }

    // Try ~/.claude.json (alternative location)
    let config_path = home.join(".claude.json");
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if content.len() <= 10_000_000 {
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(oauth) = config.get("oauthAccount") {
                        if let Some(token) = oauth.get("accessToken").and_then(|v| v.as_str()) {
                            if !token.is_empty() {
                                return Some(ClaudeOAuthTokens {
                                    access_token: token.to_string(),
                                    refresh_token: oauth
                                        .get("refreshToken")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    expires_at: oauth.get("expiresAt").and_then(|v| v.as_i64()),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: Try macOS Keychain using security command
    // Service: "Claude Code-credentials", Account: current username
    let username = std::env::var("USER").unwrap_or_else(|_| "".to_string());
    if !username.is_empty() {
        let output = std::process::Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-a",
                &username,
                "-w", // Output password to stdout
            ])
            .output()
            .ok();

        if let Some(output) = output {
            if output.status.success() {
                let password = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !password.is_empty() {
                    if let Ok(keychain_data) = serde_json::from_str::<serde_json::Value>(&password)
                    {
                        // Try claudeAiOauth structure first
                        if let Some(oauth) = keychain_data.get("claudeAiOauth") {
                            if let Some(token) = oauth.get("accessToken").and_then(|v| v.as_str()) {
                                return Some(ClaudeOAuthTokens {
                                    access_token: token.to_string(),
                                    refresh_token: oauth
                                        .get("refreshToken")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    expires_at: oauth.get("expiresAt").and_then(|v| v.as_i64()),
                                });
                            }
                        }
                        // Try flat structure
                        if let Some(token) =
                            keychain_data.get("accessToken").and_then(|v| v.as_str())
                        {
                            return Some(ClaudeOAuthTokens {
                                access_token: token.to_string(),
                                refresh_token: keychain_data
                                    .get("refreshToken")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                expires_at: keychain_data.get("expiresAt").and_then(|v| v.as_i64()),
                            });
                        }
                    } else {
                        // Raw token string (no JSON structure)
                        return Some(ClaudeOAuthTokens {
                            access_token: password,
                            refresh_token: None,
                            expires_at: None,
                        });
                    }
                }
            }
        }
    }

    None
}

#[tauri::command]
async fn claude_rate_limits(_state: State<'_, AppState>) -> Result<RateLimits, String> {
    println!("[Harness] claude_rate_limits called");

    // Get OAuth token
    let access_token = match get_claude_oauth_token() {
        Some(token) => token,
        None => {
            return Ok(RateLimits {
                primary: None,
                secondary: None,
                plan_type: None,
                not_available: Some(true),
                error_message: Some(
                    "Claude Code credentials not found. Sign in to Claude Code first.".to_string(),
                ),
            });
        }
    };

    println!("[Harness] Fetching rate limits from Anthropic OAuth API...");

    // Call Anthropic OAuth usage API
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "phantom-harness")
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        println!(
            "[Harness] Claude rate limits API returned status: {}",
            status
        );

        // Check if token expired
        if status.as_u16() == 401 {
            return Ok(RateLimits {
                primary: None,
                secondary: None,
                plan_type: None,
                not_available: Some(true),
                error_message: Some(
                    "Claude Code token expired. Re-authenticate with Claude Code.".to_string(),
                ),
            });
        }

        return Ok(RateLimits {
            primary: None,
            secondary: None,
            plan_type: None,
            not_available: Some(true),
            error_message: Some(format!("API returned status {}", status)),
        });
    }

    let api_result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    println!("[Harness] Claude rate limits result: {:?}", api_result);

    // Parse the Anthropic OAuth usage response
    // API returns: { five_hour: {utilization, resets_at}, seven_day: {utilization, resets_at}, ... }

    let primary = api_result.get("five_hour").and_then(|window| {
        // API uses "utilization" not "used_percent"
        let used_percent = window.get("utilization")?.as_f64()?;
        let reset_at = window
            .get("resets_at")
            .and_then(|v| v.as_str())
            .map(String::from);
        Some(RateLimitWindow {
            used_percent,
            window_duration_mins: 300, // 5 hours
            resets_at: reset_at,
        })
    });

    let secondary = api_result.get("seven_day").and_then(|window| {
        // API uses "utilization" not "used_percent"
        let used_percent = window.get("utilization")?.as_f64()?;
        let reset_at = window
            .get("resets_at")
            .and_then(|v| v.as_str())
            .map(String::from);
        Some(RateLimitWindow {
            used_percent,
            window_duration_mins: 10080, // 7 days
            resets_at: reset_at,
        })
    });

    // Determine plan type from response if available
    let plan_type = api_result
        .get("plan")
        .or_else(|| api_result.get("tier"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let result = RateLimits {
        primary,
        secondary,
        plan_type,
        not_available: None,
        error_message: None,
    };

    println!(
        "[Harness] Returning Claude rate limits: primary={:?}, secondary={:?}, plan={:?}",
        result.primary, result.secondary, result.plan_type
    );

    Ok(result)
}

#[tauri::command]
async fn claude_logout(state: State<'_, AppState>) -> Result<(), String> {
    let claude_cmd = resolve_claude_command();

    // Try running claude logout
    let _ = tokio::process::Command::new(&claude_cmd)
        .arg("/logout")
        .status()
        .await;

    // Update settings regardless of logout command success
    let mut settings = state.settings.lock().await;
    settings.claude_auth_method = None;
    persist_settings(&settings)?;

    Ok(())
}

#[tauri::command]
fn load_tasks(state: State<'_, AppState>) -> Result<Vec<db::TaskRecord>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::list_tasks(&conn).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UncommittedChangesResult {
    has_changes: bool,
    worktree_path: Option<String>,
}

#[tauri::command]
async fn check_task_uncommitted_changes(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<UncommittedChangesResult, String> {
    // Fetch task from database
    let task_snapshot = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let tasks = db::list_tasks(&conn).map_err(|e| e.to_string())?;
        tasks.into_iter().find(|t| t.id == task_id)
    };

    // If no task or no worktree, no changes to worry about
    let Some(task) = task_snapshot else {
        return Ok(UncommittedChangesResult {
            has_changes: false,
            worktree_path: None,
        });
    };
    let Some(path) = task.worktree_path else {
        return Ok(UncommittedChangesResult {
            has_changes: false,
            worktree_path: None,
        });
    };

    let worktree_path = PathBuf::from(&path);
    if !worktree_path.exists() {
        return Ok(UncommittedChangesResult {
            has_changes: false,
            worktree_path: Some(path),
        });
    }

    let has_changes = worktree::has_uncommitted_changes(&worktree_path).await?;
    Ok(UncommittedChangesResult {
        has_changes,
        worktree_path: Some(path),
    })
}

#[derive(Debug, serde::Serialize)]
struct DiffStats {
    additions: u64,
    deletions: u64,
    files: u64,
}

#[tauri::command]
async fn get_task_diff_stats(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<DiffStats, String> {
    let task = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let tasks = db::list_tasks(&conn).map_err(|e| e.to_string())?;
        tasks.into_iter().find(|t| t.id == task_id)
    };

    let task = task.ok_or_else(|| "Task not found".to_string())?;
    let repo_path = task
        .worktree_path
        .or(task.project_path)
        .ok_or_else(|| "Task has no path".to_string())?;

    let repo = std::path::PathBuf::from(repo_path);
    let repo_root = resolve_repo_root(&repo).await.unwrap_or(repo);
    let (additions, deletions, files) = worktree::diff_stats(&repo_root).await?;
    Ok(DiffStats {
        additions,
        deletions,
        files,
    })
}

// Review Center (diff viewer) - stubs for now.
// TODO(review): implement real diff listing/rendering and comment storage.

#[derive(Debug, serde::Serialize)]
struct ReviewDiffFile {
    path: String,
    additions: u64,
    deletions: u64,
}

#[derive(Debug, serde::Serialize)]
struct ReviewDiffFilesResult {
    files: Vec<ReviewDiffFile>,
}

#[tauri::command]
async fn get_task_diff_files(
    task_id: String,
    compare: Option<String>,
    _state: State<'_, AppState>,
) -> Result<ReviewDiffFilesResult, String> {
    let _ = (task_id, compare);
    // Placeholder so frontend wiring can land first.
    Ok(ReviewDiffFilesResult { files: Vec::new() })
}

#[derive(Debug, serde::Serialize)]
struct ReviewSplitLine {
    number: Option<u32>,
    text: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct ReviewSplitDiff {
    left: Vec<ReviewSplitLine>,
    right: Vec<ReviewSplitLine>,
}

#[derive(Debug, serde::Serialize)]
struct ReviewFileDiffResult {
    diff: serde_json::Value,
}

#[tauri::command]
async fn get_task_file_diff(
    task_id: String,
    file_path: String,
    compare: Option<String>,
    view: Option<String>,
    _state: State<'_, AppState>,
) -> Result<ReviewFileDiffResult, String> {
    let _ = (task_id, file_path, compare);

    // Placeholder payloads for the two supported views.
    let view_mode = view.unwrap_or_else(|| "split".to_string());
    if view_mode == "unified" {
        return Ok(ReviewFileDiffResult {
            diff: serde_json::Value::String(
                "// TODO(review): unified diff rendering is not implemented yet.".to_string(),
            ),
        });
    }

    let split = ReviewSplitDiff {
        left: vec![ReviewSplitLine {
            number: Some(1),
            text: "// base".to_string(),
            kind: None,
        }],
        right: vec![ReviewSplitLine {
            number: Some(1),
            text: "// TODO(review): split diff rendering is not implemented yet.".to_string(),
            kind: Some("add".to_string()),
        }],
    };

    Ok(ReviewFileDiffResult {
        diff: serde_json::to_value(split).unwrap_or_else(|_| serde_json::json!({})),
    })
}

#[tauri::command]
async fn delete_task(
    task_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    delete_task_internal(task_id, state.inner(), app).await
}

pub(crate) async fn delete_task_internal(
    task_id: String,
    state: &AppState,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let safe_task_id = task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_");
    let chat_window_label = format!("chat-{}", safe_task_id);
    if let Some(chat_window) = app.get_webview_window(&chat_window_label) {
        let _ = chat_window.close();
    }

    let notification_prefix = format!("notification-{}-", safe_task_id);
    for (label, window) in app.webview_windows() {
        if label.starts_with(&notification_prefix) {
            let _ = window.close();
        }
    }
    {
        let mut notification_windows = state
            .notification_windows
            .lock()
            .map_err(|e| e.to_string())?;
        notification_windows.retain(|label| {
            !label.starts_with(&notification_prefix) && app.get_webview_window(label).is_some()
        });
    }
    // Remove from sessions HashMap (cleanup runtime)
    let handle_ref = {
        let mut sessions = state.sessions.lock().await;
        sessions.remove(&task_id)
    };
    if let Some(handle_ref) = handle_ref {
        let client = {
            let handle = handle_ref.lock().await;
            handle.client.clone()
        };
        let _ = client.shutdown().await;
    }
    // Fetch task info without holding DB lock across await points.
    let task_snapshot = {
        if let Ok(conn) = state.db.lock() {
            if let Ok(tasks) = db::list_tasks(&conn) {
                tasks.into_iter().find(|t| t.id == task_id)
            } else {
                None
            }
        } else {
            None
        }
    };

    // Attempt to remove git worktree / workspace directory if present.
    if let Some(task) = task_snapshot {
        if let Some(path) = task.worktree_path {
            let worktree_path = PathBuf::from(path);
            let repo_root = if let Some(project_path) = task.project_path.as_ref() {
                resolve_repo_root(Path::new(project_path)).await
            } else {
                resolve_repo_root(&worktree_path).await
            };

            let mut removed = false;
            if let Some(repo_root) = repo_root {
                match worktree::remove_worktree(&repo_root, &worktree_path).await {
                    Ok(_) => {
                        removed = true;
                    }
                    Err(err) => {
                        eprintln!("[Harness] Failed to remove git worktree: {}", err);
                    }
                }
            }

            if !removed {
                if let Err(err) = worktree::remove_workspace_dir(&worktree_path) {
                    eprintln!("[Harness] Failed to remove workspace: {}", err);
                }
            }
        }
    }
    // Delete from DB
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_task(&conn, &task_id).map_err(|e| e.to_string())
}

async fn detect_base_branch(path: &PathBuf) -> String {
    // Try symbolic-ref first (most reliable for detecting remote default)
    if let Ok(output) = worktree::run_git_command(
        path,
        &["symbolic-ref", "refs/remotes/origin/HEAD", "--short"],
    )
    .await
    {
        let trimmed = output.trim();
        if let Some(branch) = trimmed.strip_prefix("origin/") {
            return branch.to_string();
        }
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if worktree::branch_exists(path, "main").await.unwrap_or(false) {
        return "main".to_string();
    }
    if worktree::branch_exists(path, "master")
        .await
        .unwrap_or(false)
    {
        return "master".to_string();
    }
    "main".to_string()
}

#[tauri::command]
async fn gather_code_review_context(project_path: String) -> Result<CodeReviewContext, String> {
    let path = std::path::PathBuf::from(&project_path);
    if !path.exists() {
        return Err(format!("Project path does not exist: {}", project_path));
    }

    let current_branch = worktree::current_branch(&path)
        .await
        .unwrap_or_else(|_| "HEAD".to_string());

    let base_branch = detect_base_branch(&path).await;

    // Get merge-base for accurate diff
    let merge_base_result = worktree::run_git_command(
        &path,
        &["merge-base", &format!("origin/{}", base_branch), "HEAD"],
    )
    .await;

    let merge_base = match merge_base_result {
        Ok(s) => s.trim().to_string(),
        Err(_) => {
            // Fallback: try without origin/ prefix
            worktree::run_git_command(&path, &["merge-base", &base_branch, "HEAD"])
                .await
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "HEAD~10".to_string())
        }
    };

    // Get committed diff (merge-base..HEAD)
    let committed_diff = worktree::run_git_command(&path, &["diff", &merge_base, "HEAD"])
        .await
        .unwrap_or_default();

    // Get uncommitted changes (staged + unstaged)
    let uncommitted = worktree::run_git_command(&path, &["diff", "HEAD"])
        .await
        .unwrap_or_default();

    // Combine diffs
    let mut full_diff = committed_diff;
    if !uncommitted.trim().is_empty() {
        full_diff.push_str("\n\n--- Uncommitted Changes ---\n");
        full_diff.push_str(&uncommitted);
    }

    // Truncate large diffs (~100KB)
    let max_bytes: usize = 100_000;
    let diff_truncated = full_diff.len() > max_bytes;
    if diff_truncated {
        let mut truncate_at = max_bytes;
        while truncate_at > 0 && !full_diff.is_char_boundary(truncate_at) {
            truncate_at -= 1;
        }
        full_diff.truncate(truncate_at);
        full_diff.push_str("\n\n[Diff truncated at ~100KB]");
    }

    // Recent commit log
    let commit_log = worktree::run_git_command(
        &path,
        &[
            "log",
            "--format=%h %s (%an, %ar)",
            &format!("{}..HEAD", merge_base),
        ],
    )
    .await
    .unwrap_or_default();

    Ok(CodeReviewContext {
        current_branch,
        base_branch,
        diff: full_diff,
        commit_log,
        diff_truncated,
    })
}

#[tauri::command]
async fn get_task_history(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    get_task_history_internal(&task_id, state.inner()).await
}

pub(crate) async fn get_task_history_internal(
    task_id: &str,
    state: &AppState,
) -> Result<serde_json::Value, String> {
    println!("[Harness] get_task_history: task_id={}", task_id);

    // Load messages from database (persisted across restarts)
    let (task, mut messages) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let task = db::list_tasks(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|t| t.id == task_id);
        let messages = db::get_messages(&conn, task_id).map_err(|e| e.to_string())?;
        (task, messages)
    };

    // Extract task fields for pending prompt detection and paths
    let (
        agent_id,
        pending_prompt,
        status_state,
        title_summary,
        created_at,
        worktree_path,
        project_path,
        branch,
    ) = match &task {
        Some(t) => (
            t.agent_id.clone(),
            t.prompt.clone(),
            t.status_state.clone(),
            t.title_summary.clone(),
            Some(t.created_at),
            t.worktree_path.clone(),
            t.project_path.clone(),
            t.branch.clone(),
        ),
        None => (
            "Agent".to_string(),
            None,
            "idle".to_string(),
            None,
            None,
            None,
            None,
            None,
        ),
    };

    let mut resolved_branch = branch.clone();
    let branch_missing = resolved_branch
        .as_deref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true);
    if branch_missing {
        let fallback_path = worktree_path.clone().or_else(|| project_path.clone());
        if let Some(path) = fallback_path {
            if let Some(repo_root) = resolve_repo_root(Path::new(&path)).await {
                if let Ok(current_branch) = worktree::current_branch(&repo_root).await {
                    let trimmed = current_branch.trim().to_string();
                    if !trimmed.is_empty() {
                        resolved_branch = Some(trimmed.clone());
                        if let Ok(conn) = state.db.lock() {
                            let _ = db::update_task_branch(&conn, task_id, &trimmed);
                        }
                    }
                }
            }
        }
    }

    println!(
        "[Harness] get_task_history: loaded {} messages from DB, status_state={}",
        messages.len(),
        status_state
    );

    // Ensure the initial prompt is the first rendered message when available.
    if let Some(ref prompt) = pending_prompt {
        if !prompt.trim().is_empty() && !messages.is_empty() {
            if let Some(idx) = messages.iter().position(|msg| {
                msg.get("message_type").and_then(|v| v.as_str()) == Some("user_message")
                    && msg.get("content").and_then(|v| v.as_str()) == Some(prompt.as_str())
            }) {
                if idx != 0 {
                    let msg = messages.remove(idx);
                    messages.insert(0, msg);
                }
            } else {
                let ts = created_at
                    .and_then(|t| chrono::Utc.timestamp_opt(t, 0).single())
                    .unwrap_or_else(chrono::Utc::now)
                    .to_rfc3339();
                messages.insert(
                    0,
                    serde_json::json!({
                        "message_type": "user_message",
                        "content": prompt,
                        "timestamp": ts
                    }),
                );
            }
        }
    }

    // Show pending prompt if: idle state AND has prompt AND no messages yet
    let show_pending = status_state == "idle" && pending_prompt.is_some() && messages.is_empty();

    Ok(serde_json::json!({
        "task_id": task_id,
        "agent_id": agent_id,
        "messages": messages,
        "pending_prompt": if show_pending { pending_prompt } else { None },
        "status_state": status_state,
        "title_summary": title_summary,
        "worktree_path": worktree_path,
        "project_path": project_path,
        "branch": resolved_branch
    }))
}

/// Open a directory in an external app (Ghostty, VS Code, etc.)
/// Accepts path directly from frontend (CodexMonitor pattern) for portability.
#[tauri::command]
async fn open_task_directory(
    path: String,
    target: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let cwd = PathBuf::from(&path);

    if !cwd.exists() {
        return Err(format!("Path does not exist: {}", cwd.display()));
    }

    #[cfg(target_os = "macos")]
    {
        let _ = &app_handle;
        let app_name = match target.as_str() {
            "ghostty" => "Ghostty",
            "vscode" => "Visual Studio Code",
            "cursor" => "Cursor",
            "zed" => "Zed",
            "antigravity" => "Antigravity",
            "finder" => "Finder",
            _ => return Err(format!("Unknown open target: {}", target)),
        };

        let status = Command::new("open")
            .args(["-a", app_name, cwd.to_string_lossy().as_ref()])
            .status()
            .map_err(|err| format!("Failed to open {}: {}", app_name, err))?;

        if !status.success() {
            return Err(format!(
                "Open command failed for {} (status: {})",
                app_name, status
            ));
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app_handle
            .opener()
            .open_path(cwd, None::<&str>)
            .map_err(|err| format!("Failed to open path: {:?}", err))?;
    }

    Ok(())
}

fn default_terminal_command() -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        if command_exists("pwsh") {
            return (
                "pwsh".to_string(),
                vec!["-NoLogo".to_string(), "-NoProfile".to_string()],
            );
        }
        if command_exists("powershell.exe") {
            return (
                "powershell.exe".to_string(),
                vec!["-NoLogo".to_string(), "-NoProfile".to_string()],
            );
        }
        return ("cmd.exe".to_string(), Vec::new());
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(shell) = std::env::var("SHELL") {
            if !shell.trim().is_empty() {
                // -i for interactive mode (gives us a prompt)
                // -l for login shell (loads profile/rc files)
                return (shell, vec!["-i".to_string(), "-l".to_string()]);
            }
        }
        for candidate in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
            if Path::new(candidate).exists() {
                return (
                    candidate.to_string(),
                    vec!["-i".to_string(), "-l".to_string()],
                );
            }
        }
        ("sh".to_string(), vec!["-i".to_string()])
    }
}

fn chat_window_label(task_id: &str) -> String {
    format!(
        "chat-{}",
        task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_")
    )
}

async fn cleanup_terminal_session_by_task(state: &AppState, task_id: &str) {
    let session_id = {
        let task_sessions = state.task_terminal_sessions.lock().await;
        task_sessions.get(task_id).cloned()
    };
    if let Some(session_id) = session_id {
        {
            let mut sessions = state.terminal_sessions.lock().await;
            if let Some(mut session) = sessions.remove(&session_id) {
                let _ = session.child.kill();
            }
        }
        {
            let mut task_sessions = state.task_terminal_sessions.lock().await;
            if let Some(existing) = task_sessions.get(task_id) {
                if existing == &session_id {
                    task_sessions.remove(task_id);
                }
            }
        }
    }
}

#[tauri::command]
async fn start_terminal_session(
    task_id: String,
    cwd: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<TerminalSessionInfo, String> {
    let cwd_path = PathBuf::from(&cwd);
    if !cwd_path.exists() {
        return Err(format!("Path does not exist: {}", cwd_path.display()));
    }

    let existing_session_id = {
        let task_sessions = state.task_terminal_sessions.lock().await;
        task_sessions.get(&task_id).cloned()
    };
    if let Some(session_id) = existing_session_id {
        let sessions = state.terminal_sessions.lock().await;
        if sessions.contains_key(&session_id) {
            return Ok(TerminalSessionInfo {
                session_id,
                cwd: cwd.clone(),
            });
        }
        let mut task_sessions = state.task_terminal_sessions.lock().await;
        task_sessions.remove(&task_id);
    }

    let (command, args) = default_terminal_command();
    println!(
        "[Harness] start_terminal_session: starting shell {} with args {:?}",
        command, args
    );
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| err.to_string())?;

    let mut cmd = CommandBuilder::new(&command);
    for arg in &args {
        cmd.arg(arg);
    }
    cmd.cwd(&cwd_path);

    println!(
        "[Harness] start_terminal_session: spawning command in {:?}",
        cwd_path
    );
    let child = pair.slave.spawn_command(cmd).map_err(|err| {
        println!("[Harness] start_terminal_session: spawn failed: {}", err);
        err.to_string()
    })?;
    println!("[Harness] start_terminal_session: shell spawned successfully");
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| err.to_string())?;
    let writer = pair.master.take_writer().map_err(|err| err.to_string())?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let app_handle = app.clone();
    let sessions = state.terminal_sessions.clone();
    let task_sessions = state.task_terminal_sessions.clone();
    let session_id_clone = session_id.clone();
    let task_id_clone = task_id.clone();
    let window_label_clone = chat_window_label(&task_id);

    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        let emit_output = |data: String| {
            if data.is_empty() {
                return;
            }
            if let Some(window) = app_handle.get_webview_window(&window_label_clone) {
                let _ = window.emit(
                    "TerminalOutput",
                    TerminalOutputPayload {
                        session_id: session_id_clone.clone(),
                        data,
                    },
                );
            }
        };

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    loop {
                        match std::str::from_utf8(&pending) {
                            Ok(valid) => {
                                emit_output(valid.to_string());
                                pending.clear();
                                break;
                            }
                            Err(err) => {
                                let valid_up_to = err.valid_up_to();
                                if valid_up_to > 0 {
                                    let valid = String::from_utf8_lossy(&pending[..valid_up_to]);
                                    emit_output(valid.to_string());
                                }
                                if let Some(err_len) = err.error_len() {
                                    // Skip invalid bytes and emit a replacement character.
                                    emit_output("\u{FFFD}".to_string());
                                    pending.drain(0..(valid_up_to + err_len));
                                    continue;
                                }
                                // Incomplete UTF-8 sequence; keep the remaining bytes for next read.
                                pending.drain(0..valid_up_to);
                                break;
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }

        if let Some(window) = app_handle.get_webview_window(&window_label_clone) {
            let _ = window.emit(
                "TerminalExit",
                TerminalExitPayload {
                    session_id: session_id_clone.clone(),
                },
            );
        }

        tauri::async_runtime::block_on(async {
            let mut sessions = sessions.lock().await;
            sessions.remove(&session_id_clone);
            let mut task_sessions = task_sessions.lock().await;
            if let Some(existing) = task_sessions.get(&task_id_clone) {
                if existing == &session_id_clone {
                    task_sessions.remove(&task_id_clone);
                }
            }
        });
    });

    let session = TerminalSession {
        _id: session_id.clone(),
        _task_id: task_id.clone(),
        master: pair.master,
        writer,
        child,
    };

    {
        let mut sessions = state.terminal_sessions.lock().await;
        sessions.insert(session_id.clone(), session);
    }
    {
        let mut task_sessions = state.task_terminal_sessions.lock().await;
        task_sessions.insert(task_id, session_id.clone());
    }

    Ok(TerminalSessionInfo { session_id, cwd })
}

#[tauri::command]
async fn terminal_write(
    session_id: String,
    data: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    println!(
        "[Harness] terminal_write called: session_id={} bytes={}",
        session_id,
        data.len()
    );
    let mut sessions = state.terminal_sessions.lock().await;
    let session = sessions.get_mut(&session_id).ok_or_else(|| {
        println!(
            "[Harness] terminal_write: session not found: {}",
            session_id
        );
        "Terminal session not found".to_string()
    })?;
    session.writer.write_all(data.as_bytes()).map_err(|err| {
        println!("[Harness] terminal_write: write failed: {}", err);
        err.to_string()
    })?;
    session.writer.flush().map_err(|err| {
        println!("[Harness] terminal_write: flush failed: {}", err);
        err.to_string()
    })?;
    println!("[Harness] terminal_write success");
    Ok(())
}

#[tauri::command]
async fn terminal_resize(
    session_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sessions = state.terminal_sessions.lock().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| "Terminal session not found".to_string())?;
    session
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
async fn terminal_close(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let task_id = {
        let mut sessions = state.terminal_sessions.lock().await;
        if let Some(mut session) = sessions.remove(&session_id) {
            let _ = session.child.kill();
            Some(session._task_id)
        } else {
            None
        }
    };
    if let Some(task_id) = task_id {
        cleanup_terminal_session_by_task(state.inner(), &task_id).await;
    }
    Ok(())
}

fn notification_enabled(settings: &Settings) -> bool {
    settings.agent_notifications_enabled.unwrap_or(true)
}

fn notification_stack(settings: &Settings) -> bool {
    settings.agent_notification_stack.unwrap_or(true)
}

/// Returns the notification auto-dismiss timeout in seconds (0 = disabled).
fn notification_timeout(settings: &Settings) -> u32 {
    settings.agent_notification_timeout.unwrap_or(0)
}

fn format_notification_preview(text: &str) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<&str>>().join(" ");
    let max_chars = 160usize;
    let mut preview: String = cleaned.chars().take(max_chars).collect();
    if cleaned.chars().count() > max_chars {
        preview.push_str("...");
    }
    if preview.is_empty() {
        "Agent finished and is waiting for your reply.".to_string()
    } else {
        preview
    }
}

fn notification_position(app: &AppHandle, index: usize) -> Position {
    let width = 360.0;
    let height = 140.0;
    let margin = 18.0;
    let gap = 12.0;

    if let Ok(Some(monitor)) = app.primary_monitor() {
        let pos = monitor.position();
        let size = monitor.size();
        let scale = monitor.scale_factor();

        let logical_pos_x = pos.x as f64 / scale;
        let logical_pos_y = pos.y as f64 / scale;
        let logical_width = size.width as f64 / scale;
        let logical_height = size.height as f64 / scale;

        let mut x = logical_pos_x + logical_width - width - margin;
        let mut y = logical_pos_y + margin + (index as f64) * (height + gap);
        let max_y = logical_pos_y + logical_height - height - margin;
        if y > max_y {
            y = max_y.max(logical_pos_y + margin);
        }
        if x < logical_pos_x + margin {
            x = logical_pos_x + margin;
        }
        return Position::Logical(LogicalPosition { x, y });
    }

    Position::Logical(LogicalPosition { x: 100.0, y: 100.0 })
}

/// Close all notification windows associated with a specific task.
#[tauri::command]
async fn dismiss_notifications_for_task(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let safe_task_id = task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_");
    let notification_prefix = format!("notification-{}-", safe_task_id);

    // Close all notification windows for this task
    for (label, window) in app.webview_windows() {
        if label.starts_with(&notification_prefix) {
            let _ = window.close();
        }
    }

    // Remove from tracking vector
    if let Ok(mut notification_windows) = state.notification_windows.lock() {
        notification_windows.retain(|label| {
            !label.starts_with(&notification_prefix) && app.get_webview_window(label).is_some()
        });
    }

    Ok(())
}

fn build_notification_url(
    task_id: &str,
    agent_id: &str,
    preview: &str,
    timeout_secs: u32,
) -> Result<WebviewUrl, String> {
    let encoded_task_id = urlencoding::encode(task_id);
    let encoded_agent = urlencoding::encode(agent_id);
    let encoded_preview = urlencoding::encode(preview);

    #[cfg(debug_assertions)]
    let window_url = {
        let url = format!(
            "http://127.0.0.1:8000/agent_notification.html?taskId={}&agent={}&preview={}&timeout={}",
            encoded_task_id, encoded_agent, encoded_preview, timeout_secs
        );
        tauri::WebviewUrl::External(url.parse().map_err(|e| format!("Invalid URL: {}", e))?)
    };

    #[cfg(not(debug_assertions))]
    let window_url = {
        let path = format!(
            "agent_notification.html?taskId={}&agent={}&preview={}&timeout={}",
            encoded_task_id, encoded_agent, encoded_preview, timeout_secs
        );
        tauri::WebviewUrl::App(path.into())
    };

    Ok(window_url)
}

fn build_discord_thread_name(_task_id: &str, title: &str) -> String {
    let mut base = title.trim();
    let lower = base.to_lowercase();
    if lower.starts_with("task") {
        base = base.trim_start_matches(|c: char| c == 't' || c == 'T');
        base = base.trim_start_matches(|c: char| c == 'a' || c == 'A');
        base = base.trim_start_matches(|c: char| c == 's' || c == 'S');
        base = base.trim_start_matches(|c: char| c == 'k' || c == 'K');
        base = base.trim_start_matches(|c: char| c == '-' || c == ':' || c == ' ');
    }
    let base = if base.is_empty() { "Agent Task" } else { base };
    let combined = format!("{}", base);
    truncate_str(&combined, 90)
}

fn get_task_metadata(state: &AppState, task_id: &str) -> (String, String) {
    let conn_guard = state.db.lock().ok();
    let tasks = conn_guard
        .as_ref()
        .and_then(|conn| db::list_tasks(conn).ok())
        .unwrap_or_default();
    tasks
        .iter()
        .find(|t| t.id == task_id)
        .map(|t| {
            (
                t.title_summary
                    .clone()
                    .unwrap_or_else(|| "Agent Task".to_string()),
                t.project_path.clone().unwrap_or_default(),
            )
        })
        .unwrap_or_else(|| ("Agent Task".to_string(), String::new()))
}

async fn ensure_discord_thread(
    state: &AppState,
    task_id: &str,
    intro_message: &str,
) -> Option<serenity::model::id::ChannelId> {
    let handle = discord_handle(state)?;
    let (task_title, _) = get_task_metadata(state, task_id);
    let thread_name = build_discord_thread_name(task_id, &task_title);
    discord_bot::ensure_thread_for_task(
        &handle,
        state.db.clone(),
        task_id,
        &thread_name,
        intro_message,
    )
    .await
    .ok()
}

async fn post_discord_user_message(state: &AppState, task_id: &str, agent_id: &str, content: &str) {
    if content.trim().is_empty() {
        return;
    }
    let settings = state.settings.lock().await.clone();
    if !discord_enabled(&settings) {
        return;
    }
    let handle = discord_handle(state);
    if handle.is_none() {
        return;
    }
    let intro = format!("**User message for {} `{}`**", agent_id, task_id);
    let _ = ensure_discord_thread(state, task_id, &intro).await;
    if let Some(handle) = handle {
        let _ = discord_bot::post_to_thread(&handle, state.db.clone(), task_id, content).await;
    }
}

async fn post_discord_assistant_message(state: &AppState, task_id: &str, content: &str) {
    if content.trim().is_empty() {
        return;
    }
    let settings = state.settings.lock().await.clone();
    if !discord_enabled(&settings) {
        return;
    }
    let handle = discord_handle(state);
    if handle.is_none() {
        return;
    }
    let intro = format!("**Assistant reply for task `{}`**", task_id);
    let _ = ensure_discord_thread(state, task_id, &intro).await;
    if let Some(handle) = handle {
        let _ = discord_bot::post_to_thread(&handle, state.db.clone(), task_id, content).await;
    }
}

async fn post_discord_user_input_request(
    state: &AppState,
    task_id: &str,
    request_id: &str,
    questions: &[UserInputQuestion],
) {
    let settings = state.settings.lock().await.clone();
    if !discord_enabled(&settings) {
        return;
    }
    let handle = discord_handle(state);
    if handle.is_none() {
        return;
    }
    let intro = format!("**User input requested for task `{}`**", task_id);
    let _ = ensure_discord_thread(state, task_id, &intro).await;

    let mut body = String::new();
    body.push_str(&format!(
        "**Input needed** (`request_id`: `{}`)\n",
        request_id
    ));
    for q in questions {
        body.push_str(&format!(
            "- **{}** ({})\n  {}\n",
            q.header, q.id, q.question
        ));
        if let Some(options) = q.options.as_ref() {
            for opt in options {
                body.push_str(&format!("  - {} — {}\n", opt.label, opt.description));
            }
        }
    }
    body.push_str("\nUse the buttons below when available, or reply with `question_id: answer` per line (or a single answer for one question).");

    if let Some(handle) = handle {
        let _ = discord_bot::post_to_thread(&handle, state.db.clone(), task_id, &body).await;
        for q in questions {
            if q.options.as_ref().map(|o| !o.is_empty()).unwrap_or(false) {
                let _ = discord_bot::post_user_input_question(
                    &handle,
                    state.db.clone(),
                    task_id,
                    request_id,
                    q,
                )
                .await;
            }
        }
    }
}

async fn maybe_show_agent_notification(
    app: &AppHandle,
    state: &AppState,
    task_id: &str,
    agent_id: &str,
    preview: &str,
) -> Result<(), String> {
    let settings = state.settings.lock().await.clone();
    if !notification_enabled(&settings) {
        return Ok(());
    }

    let chat_window_label = format!("chat-{}", task_id);
    if let Some(chat_window) = app.get_webview_window(&chat_window_label) {
        if chat_window.is_focused().unwrap_or(false) {
            return Ok(());
        }
    }

    let mut notification_windows = state
        .notification_windows
        .lock()
        .map_err(|e| e.to_string())?;
    notification_windows.retain(|label| app.get_webview_window(label).is_some());

    if !notification_stack(&settings) {
        for label in notification_windows.iter() {
            if let Some(window) = app.get_webview_window(label) {
                let _ = window.close();
            }
        }
        notification_windows.clear();
    }

    let safe_task_id = task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_");
    let label = format!("notification-{}-{}", safe_task_id, uuid::Uuid::new_v4());
    let preview_text = format_notification_preview(preview);
    let timeout_secs = notification_timeout(&settings);
    let window_url = build_notification_url(task_id, agent_id, &preview_text, timeout_secs)?;

    let notification_window = tauri::WebviewWindowBuilder::new(app, &label, window_url)
        .title("Agent Notification")
        .inner_size(360.0, 140.0)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .devtools(false)
        .build()
        .map_err(|e| format!("Failed to create notification window: {}", e))?;

    let position = notification_position(app, notification_windows.len());
    let _ = notification_window.set_position(position);

    notification_windows.push(label);

    if discord_enabled(&settings) {
        if let Some(handle) = discord_handle(state) {
            let (task_title, _project_path) = get_task_metadata(state, task_id);
            let preview_for_discord = preview.to_string();
            let thread_name = build_discord_thread_name(task_id, &task_title);
            let message = if preview_for_discord.trim().is_empty() {
                "Agent finished and is waiting for your reply.".to_string()
            } else {
                preview_for_discord
            };
            let db = state.db.clone();
            let task_id = task_id.to_string();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = discord_bot::post_task_notification(
                    &handle,
                    db,
                    &task_id,
                    &thread_name,
                    &message,
                )
                .await
                {
                    println!("[Discord] Failed to send agent notification: {}", e);
                }
            });
        }
    } else if let Some(webhook_url) = settings.webhook.as_ref().filter(|s| !s.is_empty()) {
        let webhook_url = webhook_url.clone();
        let agent_display_name = get_agent_display_name(&state.config, agent_id);

        // Fetch task details from database
        let (task_title, project_path) = get_task_metadata(state, task_id);

        let preview_for_webhook = preview.to_string();

        // Spawn async task to send webhook (non-blocking)
        tauri::async_runtime::spawn(async move {
            let payload = webhook::build_agent_notification_payload(
                &agent_display_name,
                &task_title,
                &project_path,
                &preview_for_webhook,
            );
            if let Err(e) = webhook::send_webhook(&webhook_url, &payload).await {
                println!("[Webhook] Failed to send agent notification: {}", e);
            } else {
                println!("[Webhook] Agent notification sent successfully");
            }
        });
    }

    Ok(())
}

async fn summarize_status_for_notifications(
    state: &AppState,
    task_agent_id: &str,
    full_text: &str,
    fallback: &str,
) -> String {
    let settings = state.settings.lock().await.clone();
    if !settings.ai_summaries_enabled.unwrap_or(true) {
        return fallback.to_string();
    }

    let summary = summarize::summarize_status_with_override(
        full_text,
        task_agent_id,
        settings.summaries_agent.as_deref(),
    )
    .await;
    if summary.trim().is_empty() {
        fallback.to_string()
    } else {
        summary
    }
}

#[tauri::command]
async fn open_chat_window(
    task_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let window_label = format!(
        "chat-{}",
        task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_")
    );

    println!(
        "[Harness] open_chat_window called: task_id={} label={}",
        task_id, window_label
    );

    // Check if window already exists
    if let Some(existing) = app.get_webview_window(&window_label) {
        println!("[Harness] Window exists, focusing: {}", window_label);
        // Focus existing window
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Get task info for window title from database (doesn't block on sessions lock)
    let agent_name = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::list_tasks(&conn)
            .ok()
            .and_then(|tasks| tasks.into_iter().find(|t| t.id == task_id))
            .map(|t| t.agent_id)
            .unwrap_or_else(|| "Agent".to_string())
    };

    // Build URL - in dev mode, use the dev server URL; in production, use App path
    let encoded_task_id = urlencoding::encode(&task_id);

    // Check if we're in dev mode by checking for the dev server
    // In production builds, use WindowUrl::App; in dev, use External
    #[cfg(debug_assertions)]
    let window_url = {
        let url = format!(
            "http://127.0.0.1:8000/agent_chat_log.html?taskId={}",
            encoded_task_id
        );
        println!("[Harness] Dev mode - using external URL: {}", url);
        tauri::WebviewUrl::External(url.parse().map_err(|e| format!("Invalid URL: {}", e))?)
    };

    #[cfg(not(debug_assertions))]
    let window_url = {
        let path = format!("agent_chat_log.html?taskId={}", encoded_task_id);
        println!("[Harness] Release mode - using app path: {}", path);
        tauri::WebviewUrl::App(path.into())
    };

    println!(
        "[Harness] Creating window: {} with title: {} Chat - {}",
        window_label, agent_name, task_id
    );

    let devtools_enabled = cfg!(debug_assertions)
        || std::env::var("PHANTOM_CHAT_DEVTOOLS").ok().as_deref() == Some("1");

    tauri::WebviewWindowBuilder::new(&app, &window_label, window_url)
        .title(format!("{} Chat - {}", agent_name, task_id))
        .inner_size(650.0, 750.0)
        .decorations(false)
        .transparent(true)
        .resizable(true)
        .center()
        .devtools(devtools_enabled)
        .build()
        .map_err(|e| format!("Failed to create chat window: {}", e))?;

    println!("[Harness] Opened chat window for task: {}", task_id);
    Ok(())
}

pub(crate) async fn send_chat_message_internal(
    task_id: String,
    message: String,
    state: &AppState,
    app: tauri::AppHandle,
    origin: MessageOrigin,
) -> Result<(), String> {
    println!(
        "[Harness] send_chat_message: task={} message_len={}",
        task_id,
        message.len()
    );
    let from_discord = origin == MessageOrigin::Discord;

    // Emit status update to chat window
    let window_label = format!(
        "chat-{}",
        task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_")
    );
    if let Some(window) = app.get_webview_window(&window_label) {
        let _ = window.emit("ChatLogStatus", (&task_id, "Working...", "running"));
    }

    // Extract session handle without removing it from the map.
    let handle_ref = {
        let sessions = state.sessions.lock().await;
        sessions.get(&task_id).cloned()
    };

    let handle_ref = if let Some(handle_ref) = handle_ref {
        handle_ref
    } else {
        println!(
            "[Harness] Session not found, attempting to reconnect: {}",
            task_id
        );

        // Look up task from DB to get reconnection info
        let task = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            db::list_tasks(&conn)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|t| t.id == task_id)
        };

        let task = match task {
            Some(t) => t,
            None => {
                let error_msg = "Task not found in database";
                println!("[Harness] send_chat_message error: {}", error_msg);
                if let Some(window) = app.get_webview_window(&window_label) {
                    let _ = window.emit("ChatLogStatus", (&task_id, error_msg, "error"));
                }
                return Err(error_msg.to_string());
            }
        };

        // Find agent config
        let agent = match find_agent(&state.config, &task.agent_id) {
            Some(a) => a,
            None => {
                let error_msg = format!("Unknown agent: {}", task.agent_id);
                println!("[Harness] send_chat_message error: {}", error_msg);
                if let Some(window) = app.get_webview_window(&window_label) {
                    let _ = window.emit("ChatLogStatus", (&task_id, &error_msg, "error"));
                }
                return Err(error_msg);
            }
        };

        // Set up working directory
        let cwd = resolve_task_cwd(&task)?;

        // Build environment
        let settings = state.settings.lock().await.clone();
        let overrides = auth_env_for(&task.agent_id, &settings);
        let allow_missing = (task.agent_id == "codex"
            && settings.codex_auth_method.as_deref() == Some("chatgpt"))
            || (task.agent_id == "claude-code"
                && matches!(
                    settings.claude_auth_method.as_deref(),
                    Some("cli") | Some("oauth")
                ));

        let env = match build_env(&agent.required_env, &overrides, allow_missing) {
            Ok(e) => e,
            Err(e) => {
                let error_msg = format!("Auth not configured: {}", e);
                println!("[Harness] send_chat_message error: {}", error_msg);
                if let Some(window) = app.get_webview_window(&window_label) {
                    let _ = window.emit("ChatLogStatus", (&task_id, &error_msg, "error"));
                }
                return Err(error_msg);
            }
        };

        // Reconnect with context restoration (hybrid: session/load or history injection)
        println!("[Harness] Reconnecting Agent session for task: {}", task_id);
        if let Some(window) = app.get_webview_window(&window_label) {
            let _ = window.emit("ChatLogStatus", (&task_id, "Reconnecting...", "running"));
        }

        let (client, session_id, used_session_load) =
            match reconnect_session_with_context(agent, &task, &cwd, &env, &state.db).await {
                Ok(result) => result,
                Err(e) => {
                    let error_msg = format!("Failed to reconnect: {}", e);
                    println!("[Harness] send_chat_message error: {}", error_msg);
                    if let Some(window) = app.get_webview_window(&window_label) {
                        let _ = window.emit("ChatLogStatus", (&task_id, &error_msg, "error"));
                    }
                    return Err(error_msg);
                }
            };

        let model = task.model.clone();

        println!(
            "[Harness] Session reconnected: task_id={} session_id={} (used_session_load={})",
            task_id, session_id, used_session_load
        );

        // Spawn Claude usage watcher for reconnected chat sessions
        let claude_watcher = if task.agent_id == "claude-code" {
            Some(claude_usage_watcher::start_watching(
                &session_id,
                &task_id,
                app.clone(),
                state.db.clone(),
            ))
        } else {
            None
        };

        // Store whether we need history injection for this session
        // (will be used when sending the actual message)
        let needs_history_injection = !used_session_load;

        // Prepare the message with history context if needed
        let message_with_context = if needs_history_injection {
            let history_opt = {
                let conn = state.db.lock().map_err(|e| e.to_string())?;
                let messages_db =
                    db::get_message_records(&conn, &task.id).map_err(|e| e.to_string())?;
                if !messages_db.is_empty() {
                    let (history, _) =
                        db::compact_history(&messages_db, task.prompt.as_deref(), 100_000);
                    Some(history)
                } else {
                    None
                }
            };

            if let Some(history) = history_opt {
                format_message_with_history(&history, &message)
            } else {
                message.clone()
            }
        } else {
            message.clone()
        };

        let handle = SessionHandle {
            agent_id: task.agent_id.clone(),
            session_id,
            model: model.clone(),
            client,
            pending_prompt: Some(message_with_context),
            pending_attachments: Vec::new(),
            messages: Vec::new(),
            claude_watcher,
            cancel_token: CancellationToken::new(),
        };

        let handle_ref = Arc::new(Mutex::new(handle));
        let mut sessions = state.sessions.lock().await;
        sessions.insert(task_id.clone(), handle_ref.clone());

        if task.agent_id == "codex" || task.agent_id == "claude-code" {
            let command_root = resolve_repo_root(&cwd).await.unwrap_or_else(|| cwd.clone());
            let commands = if task.agent_id == "codex" {
                collect_codex_commands(state, &command_root)
            } else {
                collect_claude_commands(state, &command_root)
            };
            emit_available_commands(&app, &task_id, &task.agent_id, &commands);
        }
        handle_ref
    };

    let user_timestamp = chrono::Utc::now().to_rfc3339();
    let (agent_id, model, client, session_id, effective_message, cancel_token) = {
        let mut handle = handle_ref.lock().await;
        let effective_message = handle
            .pending_prompt
            .take()
            .unwrap_or_else(|| message.clone());
        handle.messages.push(serde_json::json!({
            "message_type": "user_message",
            "content": message,
            "timestamp": user_timestamp
        }));
        // Create a fresh cancellation token for this generation
        handle.cancel_token = CancellationToken::new();
        (
            handle.agent_id.clone(),
            handle.model.clone(),
            handle.client.clone(),
            handle.session_id.clone(),
            effective_message,
            handle.cancel_token.clone(),
        )
    };

    // Load any pending attachments for this task (e.g., pasted images in chat log)
    let attachments: Vec<db::AttachmentRecord> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        match db::get_pending_attachments(&conn, &task_id) {
            Ok(list) => {
                if !list.is_empty() {
                    let _ = db::clear_pending_attachments(&conn, &task_id);
                }
                list
            }
            Err(_) => Vec::new(),
        }
    };

    // Load images from attachments (if any)
    let images: Vec<ImageContent> = {
        let mut loaded = Vec::new();
        if !attachments.is_empty() {
            let base_dir = attachments_dir().map_err(|e| e.to_string())?;
            for att in &attachments {
                let file_path = base_dir.join(&att.relative_path);
                if file_path.exists() {
                    match std::fs::read(&file_path) {
                        Ok(data) => {
                            use base64::Engine;
                            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                            let media_type = att
                                .mime_type
                                .clone()
                                .unwrap_or_else(|| "image/png".to_string());
                            loaded.push(ImageContent {
                                media_type,
                                data: encoded,
                            });
                        }
                        Err(e) => {
                            eprintln!(
                                "[Harness] Failed to read attachment {}: {}",
                                att.relative_path, e
                            );
                        }
                    }
                }
            }
        }
        loaded
    };

    // Set up channel for streaming updates (same pattern as start_task)
    let (stream_tx, stream_rx) = std::sync::mpsc::channel::<StreamingUpdate>();

    // Spawn a task to emit streaming updates to the chat window AND main window status
    let app_handle = app.clone();
    let task_id_streaming = task_id.clone();
    let window_label_streaming = window_label.clone();
    let agent_id_for_stream = agent_id.clone();
    let state_for_stream: AppState = state.clone();
    let stream_emit_handle = tokio::task::spawn_blocking(move || {
        use std::time::{Duration, Instant};

        // Throttle status updates to max ~10/sec for performance
        let throttle_duration = Duration::from_millis(100);
        let mut last_status_update = Instant::now()
            .checked_sub(throttle_duration)
            .unwrap_or_else(Instant::now);

        while let Ok(update) = stream_rx.recv() {
            // Emit status update to main window (throttled for non-tool updates)
            let should_emit_status = match &update {
                StreamingUpdate::ToolCall { .. } | StreamingUpdate::ToolReturn { .. } => true,
                StreamingUpdate::Status { .. } => true,
                StreamingUpdate::PermissionRequest { .. } => true,
                StreamingUpdate::UserInputRequest { .. } => true,
                StreamingUpdate::PlanUpdate { .. } => true,
                StreamingUpdate::PlanContent { .. } => true,
                StreamingUpdate::TextChunk { .. } | StreamingUpdate::ReasoningChunk { .. } => {
                    last_status_update.elapsed() >= throttle_duration
                }
                StreamingUpdate::AvailableCommands { .. } => false,
            };

            if should_emit_status {
                if let Some(main_window) = app_handle.get_webview_window("main") {
                    let (status_text, color) = match &update {
                        StreamingUpdate::ToolCall { name, arguments } => {
                            (format_tool_status(name, arguments), "yellow")
                        }
                        StreamingUpdate::ToolReturn { .. } => {
                            ("Tool completed".to_string(), "white")
                        }
                        StreamingUpdate::ReasoningChunk { .. } => {
                            ("Thinking...".to_string(), "white")
                        }
                        StreamingUpdate::TextChunk { .. } => ("Responding...".to_string(), "white"),
                        StreamingUpdate::Status { message } => (message.clone(), "yellow"),
                        StreamingUpdate::PermissionRequest { tool_name, .. } => {
                            (format!("Waiting for permission: {}", tool_name), "#4ade80")
                        }
                        StreamingUpdate::UserInputRequest { .. } => {
                            ("Waiting for input...".to_string(), "#4ade80")
                        }
                        StreamingUpdate::PlanUpdate { .. } => ("Plan updated".to_string(), "white"),
                        StreamingUpdate::PlanContent { .. } => {
                            ("Plan content".to_string(), "white")
                        }
                        StreamingUpdate::AvailableCommands { .. } => continue,
                    };
                    let _ = main_window.emit(
                        "StatusUpdate",
                        (&task_id_streaming, &status_text, color, "running"),
                    );
                    last_status_update = Instant::now();
                }
            }

            if let StreamingUpdate::UserInputRequest {
                request_id,
                questions,
            } = &update
            {
                let pending = PendingUserInput {
                    request_id: request_id.clone(),
                    questions: questions.clone(),
                    answers: std::collections::HashMap::new(),
                };
                let state_clone = state_for_stream.clone();
                let task_id = task_id_streaming.clone();
                let request_id = request_id.clone();
                let questions = questions.clone();
                tauri::async_runtime::spawn(async move {
                    {
                        let mut guard: tokio::sync::MutexGuard<
                            '_,
                            std::collections::HashMap<String, PendingUserInput>,
                        > = state_clone.pending_user_inputs.lock().await;
                        guard.insert(task_id.clone(), pending);
                    }
                    post_discord_user_input_request(
                        &state_clone,
                        &task_id,
                        &request_id,
                        &questions,
                    )
                    .await;
                });
            }

            // Emit to chat window (always, for streaming display)
            if let Some(chat_window) = app_handle.get_webview_window(&window_label_streaming) {
                let chat_msg = match &update {
                    StreamingUpdate::TextChunk { text, item_id } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "text_chunk",
                        "content": text,
                        "item_id": item_id
                    }),
                    StreamingUpdate::ReasoningChunk { text } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "reasoning_chunk",
                        "content": text
                    }),
                    StreamingUpdate::ToolCall { name, arguments } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "tool_call",
                        "name": name,
                        "arguments": arguments
                    }),
                    StreamingUpdate::ToolReturn { output } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "tool_return",
                        "content": output
                    }),
                    StreamingUpdate::Status { message } => serde_json::json!({
                        "type": "streaming",
                        "message_type": "status",
                        "content": message
                    }),
                    StreamingUpdate::AvailableCommands { commands } => {
                        // Emit available commands to all windows for slash command autocomplete
                        if agent_id_for_stream == "codex" || agent_id_for_stream == "claude-code" {
                            continue;
                        }
                        emit_available_commands(
                            &app_handle,
                            &task_id_streaming,
                            &agent_id_for_stream,
                            commands,
                        );
                        continue;
                    }
                    StreamingUpdate::PermissionRequest {
                        request_id,
                        tool_name,
                        description,
                        raw_input,
                        options,
                    } => {
                        serde_json::json!({
                            "type": "streaming",
                            "message_type": "permission_request",
                            "request_id": request_id,
                            "tool_name": tool_name,
                            "description": description,
                            "raw_input": raw_input,
                            "options": options
                        })
                    }
                    StreamingUpdate::UserInputRequest {
                        request_id,
                        questions,
                    } => {
                        serde_json::json!({
                            "type": "streaming",
                            "message_type": "user_input_request",
                            "request_id": request_id,
                            "questions": questions
                        })
                    }
                    StreamingUpdate::PlanUpdate {
                        turn_id,
                        explanation,
                        steps,
                    } => {
                        serde_json::json!({
                            "type": "streaming",
                            "message_type": "plan_update",
                            "turn_id": turn_id,
                            "explanation": explanation,
                            "plan": steps
                        })
                    }
                    StreamingUpdate::PlanContent { file_path, content } => {
                        serde_json::json!({
                            "type": "streaming",
                            "message_type": "plan_content",
                            "file_path": file_path,
                            "content": content
                        })
                    }
                };
                let _ = chat_window.emit("ChatLogStreaming", (&task_id_streaming, chat_msg));
            }
        }
    });

    // Persist user message before sending so reload ordering is correct
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let message_id = db::save_message(
            &conn,
            &task_id,
            "user_message",
            Some(&message),
            None,
            None,
            None,
            None,
            &user_timestamp,
        )
        .map_err(|e| e.to_string())?;
        let _ = db::save_message_attachments(&conn, &task_id, message_id, &attachments);
    }

    if from_discord {
        if let Some(window) = app.get_webview_window(&window_label) {
            let user_chat_msg = serde_json::json!({
                "message_type": "user_message",
                "content": message.clone(),
                "timestamp": user_timestamp
            });
            let _ = window.emit("ChatLogUpdate", (&task_id, user_chat_msg));
        }
    } else {
        post_discord_user_message(state, &task_id, &agent_id, &message).await;
    }

    // Send prompt to agent using STREAMING version (with images if present)
    // Wrapped in retry logic for recoverable errors (exit code 143/SIGTERM)
    const MAX_RECONNECT_ATTEMPTS: u32 = 2;
    let mut attempt = 0;
    let mut client = client;
    let mut session_id = session_id;

    let response = loop {
        attempt += 1;

        let result = if images.is_empty() {
            client
                .session_prompt_streaming_with_cancellation(
                    &session_id,
                    &effective_message,
                    |update| {
                        let _ = stream_tx.send(update);
                    },
                    Some(&cancel_token),
                )
                .await
        } else {
            println!("[Harness] send_chat_message with {} image(s)", images.len());
            client
                .session_prompt_streaming_with_images_and_cancellation(
                    &session_id,
                    &effective_message,
                    &images,
                    |update| {
                        let _ = stream_tx.send(update);
                    },
                    Some(&cancel_token),
                )
                .await
        };

        match result {
            Ok(response) => break response,
            Err(e) => {
                let error_str = e.to_string();
                println!(
                    "[Harness] send_chat_message error (attempt {}): {}",
                    attempt, error_str
                );

                // Check if this is a recoverable exit (SIGTERM/exit code 143)
                if is_recoverable_exit(&error_str) && attempt < MAX_RECONNECT_ATTEMPTS {
                    println!(
                        "[Harness] Detected recoverable exit in chat, attempting reconnection..."
                    );

                    // Emit reconnection status
                    if let Some(window) = app.get_webview_window(&window_label) {
                        let _ = window.emit(
                            "ChatLogStatus",
                            (&task_id, "Session terminated, reconnecting...", "running"),
                        );
                    }

                    // Look up task from DB to get reconnection info
                    let task = {
                        let conn = state.db.lock().map_err(|e| e.to_string())?;
                        db::list_tasks(&conn)
                            .map_err(|e| e.to_string())?
                            .into_iter()
                            .find(|t| t.id == task_id)
                    };

                    let task = match task {
                        Some(t) => t,
                        None => {
                            let (formatted_error, _) = format_agent_error(&error_str);
                            if let Some(window) = app.get_webview_window(&window_label) {
                                let _ = window
                                    .emit("ChatLogStatus", (&task_id, &formatted_error, "error"));
                            }
                            return Err(format!("Agent error: {}", formatted_error));
                        }
                    };

                    // Find agent config
                    let agent = match find_agent(&state.config, &task.agent_id) {
                        Some(a) => a,
                        None => {
                            let (formatted_error, _) = format_agent_error(&error_str);
                            if let Some(window) = app.get_webview_window(&window_label) {
                                let _ = window
                                    .emit("ChatLogStatus", (&task_id, &formatted_error, "error"));
                            }
                            return Err(format!("Agent error: {}", formatted_error));
                        }
                    };

                    // Set up working directory
                    let cwd = resolve_task_cwd(&task)?;

                    // Build environment
                    let settings = state.settings.lock().await.clone();
                    let overrides = auth_env_for(&task.agent_id, &settings);
                    let allow_missing = (task.agent_id == "codex"
                        && settings.codex_auth_method.as_deref() == Some("chatgpt"))
                        || (task.agent_id == "claude-code"
                            && matches!(
                                settings.claude_auth_method.as_deref(),
                                Some("cli") | Some("oauth")
                            ));

                    let env = match build_env(&agent.required_env, &overrides, allow_missing) {
                        Ok(e) => e,
                        Err(e) => {
                            let error_msg = format!("Reconnection failed - auth error: {}", e);
                            if let Some(window) = app.get_webview_window(&window_label) {
                                let _ =
                                    window.emit("ChatLogStatus", (&task_id, &error_msg, "error"));
                            }
                            return Err(error_msg);
                        }
                    };

                    // Reconnect with context restoration
                    match reconnect_session_with_context(agent, &task, &cwd, &env, &state.db).await
                    {
                        Ok((new_client, new_session_id, _used_session_load)) => {
                            println!(
                                "[Harness] Chat session reconnected after termination: {}",
                                new_session_id
                            );

                            {
                                let mut handle = handle_ref.lock().await;
                                handle.client = new_client.clone();
                                handle.session_id = new_session_id.clone();

                                // Also update Claude watcher if applicable
                                if task.agent_id == "claude-code" {
                                    handle.claude_watcher =
                                        Some(claude_usage_watcher::start_watching(
                                            &new_session_id,
                                            &task_id,
                                            app.clone(),
                                            state.db.clone(),
                                        ));
                                }
                            }

                            client = new_client;
                            session_id = new_session_id.clone();

                            // Emit status and retry
                            if let Some(window) = app.get_webview_window(&window_label) {
                                let _ = window.emit(
                                    "ChatLogStatus",
                                    (&task_id, "Reconnected, retrying...", "running"),
                                );
                            }
                            continue; // Retry the prompt
                        }
                        Err(reconnect_err) => {
                            let (formatted_error, _) = format_agent_error(&error_str);
                            let error_msg = format!(
                                "Session terminated and reconnection failed: {} (reconnect error: {})",
                                formatted_error, reconnect_err
                            );
                            if let Some(window) = app.get_webview_window(&window_label) {
                                let _ =
                                    window.emit("ChatLogStatus", (&task_id, &error_msg, "error"));
                            }
                            return Err(error_msg);
                        }
                    }
                } else {
                    // Not recoverable or max attempts reached
                    let (formatted_error, _) = format_agent_error(&error_str);
                    let error_msg = format!("Agent error: {}", formatted_error);
                    println!("[Harness] send_chat_message error: {}", error_msg);
                    if let Some(window) = app.get_webview_window(&window_label) {
                        let _ = window.emit("ChatLogStatus", (&task_id, &error_msg, "error"));
                    }
                    return Err(error_msg);
                }
            }
        }
    };

    // Drop the sender to signal completion, then wait for emit task
    drop(stream_tx);
    let _ = stream_emit_handle.await;

    // Debug: log the response
    println!(
        "[Harness] session_prompt_streaming response: {} messages",
        response.messages.len()
    );
    for (i, msg) in response.messages.iter().enumerate() {
        println!(
            "[Harness]   msg[{}]: type={} content={:?}",
            i, msg.message_type, msg.content
        );
    }

    // Store and emit response messages to chat window
    let mut final_status = "Ready".to_string();
    if let Some(window) = app.get_webview_window(&window_label) {
        for msg in &response.messages {
            let msg_timestamp = chrono::Utc::now().to_rfc3339();
            // Map Agent message types to our frontend types
            let chat_msg = serde_json::json!({
                "message_type": msg.message_type,
                "content": msg.content,
                "reasoning": msg.reasoning,
                "tool_call": msg.name.as_ref().map(|name| serde_json::json!({
                    "name": name,
                    "arguments": msg.arguments
                })),
                "tool_return": msg.tool_return,
                "timestamp": msg_timestamp
            });

            // Store message in memory
            {
                let mut handle = handle_ref.lock().await;
                handle.messages.push(chat_msg.clone());
            }

            // Persist to DB
            {
                let conn = state.db.lock().map_err(|e| e.to_string())?;
                let _ = db::save_message(
                    &conn,
                    &task_id,
                    &msg.message_type,
                    msg.content.as_deref(),
                    msg.reasoning.as_deref(),
                    msg.name.as_deref(),
                    msg.arguments.as_deref(),
                    msg.tool_return.as_deref(),
                    &msg_timestamp,
                );
            }

            let _ = window.emit("ChatLogUpdate", (&task_id, chat_msg));
        }

        // Emit completion status
        final_status = response
            .messages
            .iter()
            .filter(|m| m.message_type == "assistant_message")
            .filter_map(|m| m.content.as_ref())
            .last()
            .map(|s| truncate_str(s, 40))
            .unwrap_or_else(|| "Ready".to_string());
    }

    // Check if the generation was cancelled (soft stop)
    let was_cancelled = cancel_token.is_cancelled();

    if was_cancelled {
        // Generation was stopped by user - emit GenerationStopped and set status to Ready
        println!(
            "[Harness] Generation was cancelled for task_id={}, emitting GenerationStopped",
            task_id
        );

        if let Some(window) = app.get_webview_window(&window_label) {
            let _ = window.emit("GenerationStopped", &task_id);
            let _ = window.emit("ChatLogStatus", (&task_id, "Ready", "idle"));
        }
        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.emit("StatusUpdate", (&task_id, "Ready", "#04d885", "idle"));
        }
        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db::update_task_status(&conn, &task_id, "Ready", "idle");
        }
    } else {
        // Normal completion
        let preview_source = response
            .messages
            .iter()
            .filter(|m| m.message_type == "assistant_message")
            .filter_map(|m| m.content.as_ref())
            .last()
            .cloned()
            .unwrap_or_else(|| final_status.clone());
        let summary_status =
            summarize_status_for_notifications(state, &agent_id, &preview_source, &final_status)
                .await;
        if let Some(window) = app.get_webview_window(&window_label) {
            let _ = window.emit("ChatLogStatus", (&task_id, &summary_status, "completed"));
        }
        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.emit(
                "StatusUpdate",
                (&task_id, &summary_status, "#04d885", "completed"),
            );
        }
        {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db::update_task_status(&conn, &task_id, &summary_status, "completed");
        }

        let _ =
            maybe_show_agent_notification(&app, state, &task_id, &agent_id, &summary_status).await;
    }

    // Process token usage and update cost (always do this, even if cancelled)
    if let Some(usage) = &response.token_usage {
        let cost = calculate_cost_from_usage(&model, usage);
        if cost > 0.0 {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            let current_cost = db::get_task_cost(&conn, &task_id).unwrap_or(0.0);
            let new_total = current_cost + cost;
            let _ = db::update_task_cost(&conn, &task_id, new_total);
            // Emit to main window and chat window
            app.emit("CostUpdate", (&task_id, new_total))
                .map_err(|e| e.to_string())?;
        }
        // Emit token usage for context indicator and save to database
        app.emit("TokenUsageUpdate", (&task_id, usage))
            .map_err(|e| e.to_string())?;
        // Persist token usage to database for restart recovery
        let total_tokens = usage.total_token_usage.total_tokens;
        let context_window = usage.model_context_window;
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let _ = db::update_task_token_usage(&conn, &task_id, total_tokens, context_window);
    }

    Ok(())
}

#[tauri::command]
async fn send_chat_message(
    task_id: String,
    message: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    send_chat_message_internal(task_id, message, state.inner(), app, MessageOrigin::Ui).await
}

#[tauri::command]
async fn respond_to_permission(
    task_id: String,
    request_id: String,
    response_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    println!(
        "[Harness] respond_to_permission: task={} request={} response={}",
        task_id, request_id, response_id
    );

    // Get session
    let handle_ref = {
        let sessions = state.sessions.lock().await;
        sessions.get(&task_id).cloned()
    }
    .ok_or_else(|| format!("Session not found: {}", task_id))?;

    let (client, session_id) = {
        let handle = handle_ref.lock().await;
        (handle.client.clone(), handle.session_id.clone())
    };

    // Send permission response to the agent
    client
        .send_permission_response(&session_id, &request_id, &response_id)
        .await
        .map_err(|e| format!("Failed to send permission response: {}", e))?;

    // Emit status update to chat window
    let window_label = format!(
        "chat-{}",
        task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_")
    );
    if let Some(window) = app.get_webview_window(&window_label) {
        let status_text = if response_id == "deny" {
            "Permission denied"
        } else {
            "Permission granted"
        };
        let _ = window.emit("ChatLogStatus", (&task_id, status_text, "running"));
    }

    // Also emit to main window
    if let Some(main_window) = app.get_webview_window("main") {
        let status_text = if response_id == "deny" {
            "Permission denied"
        } else {
            "Continuing..."
        };
        let _ = main_window.emit("StatusUpdate", (&task_id, status_text, "yellow", "running"));
    }

    Ok(())
}

pub(crate) async fn respond_to_user_input_internal(
    task_id: String,
    request_id: String,
    answers: serde_json::Value,
    state: &AppState,
    app: tauri::AppHandle,
) -> Result<(), String> {
    println!(
        "[Harness] respond_to_user_input: task={} request={}",
        task_id, request_id
    );
    tracing::info!("[Harness] respond_to_user_input payload: {}", answers);

    let handle_ref = {
        let sessions = state.sessions.lock().await;
        sessions.get(&task_id).cloned()
    }
    .ok_or_else(|| format!("Session not found: {}", task_id))?;

    let client = {
        let handle = handle_ref.lock().await;
        handle.client.clone()
    };

    client
        .send_user_input_response(&request_id, answers)
        .await
        .map_err(|e| {
            tracing::error!(
                "[Harness] respond_to_user_input send failed: request_id={} error={}",
                request_id,
                e
            );
            format!("Failed to send user input response: {}", e)
        })?;

    tracing::info!(
        "[Harness] respond_to_user_input sent for request_id={}",
        request_id
    );

    {
        let mut pending = state.pending_user_inputs.lock().await;
        pending.remove(&task_id);
    }

    // Emit status update to chat window
    let window_label = format!(
        "chat-{}",
        task_id.replace(|c: char| !c.is_alphanumeric() && c != '-', "_")
    );
    if let Some(window) = app.get_webview_window(&window_label) {
        let _ = window.emit("ChatLogStatus", (&task_id, "Answered", "running"));
    }

    if let Some(main_window) = app.get_webview_window("main") {
        let _ = main_window.emit(
            "StatusUpdate",
            (&task_id, "Continuing...", "yellow", "running"),
        );
    }

    Ok(())
}

#[tauri::command]
async fn respond_to_user_input(
    task_id: String,
    request_id: String,
    answers: serde_json::Value,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    respond_to_user_input_internal(task_id, request_id, answers, state.inner(), app).await
}

// ============================================================================
// Attachment handling commands
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SaveAttachmentPayload {
    #[serde(rename = "taskId")]
    task_id: String,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "mimeType")]
    mime_type: String,
    /// Base64-encoded file data
    data: String,
}

#[tauri::command]
async fn save_attachment(
    payload: SaveAttachmentPayload,
    state: State<'_, AppState>,
) -> Result<db::AttachmentRecord, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    // Decode base64 data
    let data = STANDARD
        .decode(&payload.data)
        .map_err(|e| format!("Invalid base64 data: {}", e))?;

    let byte_size = data.len() as i64;

    // Validate image size (max 5MB for Claude API)
    if byte_size > 5 * 1024 * 1024 {
        return Err("Image exceeds 5MB size limit".to_string());
    }

    // Generate unique ID and file path
    let id = uuid::Uuid::new_v4().to_string();
    let extension = payload.file_name.rsplit('.').next().unwrap_or("png");
    let relative_path = format!("{}.{}", id, extension);

    // Save file to disk
    let attachments_path = attachments_dir()?;
    let file_path = attachments_path.join(&relative_path);
    std::fs::write(&file_path, &data).map_err(|e| format!("Failed to save attachment: {}", e))?;

    let attachment = db::AttachmentRecord {
        id: id.clone(),
        file_name: Some(payload.file_name),
        mime_type: Some(payload.mime_type),
        relative_path,
        byte_size,
    };

    // Save to database
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::save_pending_attachments(&conn, &payload.task_id, &[attachment.clone()])
        .map_err(|e| format!("Failed to save attachment record: {}", e))?;

    println!(
        "[Harness] Saved attachment: id={} size={} bytes",
        id, byte_size
    );

    Ok(attachment)
}

#[tauri::command]
async fn get_pending_attachments(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<db::AttachmentRecord>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::get_pending_attachments(&conn, &task_id)
        .map_err(|e| format!("Failed to get attachments: {}", e))
}

#[tauri::command]
async fn delete_attachment(
    attachment_id: String,
    task_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    // Get attachment to find file path
    let attachments = db::get_pending_attachments(&conn, &task_id)
        .map_err(|e| format!("Failed to get attachments: {}", e))?;

    if let Some(attachment) = attachments.iter().find(|a| a.id == attachment_id) {
        // Delete file from disk
        let attachments_path = attachments_dir()?;
        let file_path = attachments_path.join(&attachment.relative_path);
        if file_path.exists() {
            std::fs::remove_file(&file_path)
                .map_err(|e| format!("Failed to delete file: {}", e))?;
        }

        // Delete from database
        conn.execute(
            "DELETE FROM pending_attachments WHERE id = ?1",
            rusqlite::params![attachment_id],
        )
        .map_err(|e| format!("Failed to delete attachment record: {}", e))?;

        println!("[Harness] Deleted attachment: {}", attachment_id);
    }

    Ok(())
}

#[tauri::command]
async fn get_attachment_base64(relative_path: String) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let attachments_path = attachments_dir()?;
    let file_path = attachments_path.join(&relative_path);

    let data =
        std::fs::read(&file_path).map_err(|e| format!("Failed to read attachment: {}", e))?;

    Ok(STANDARD.encode(&data))
}

// =============================================================================
// AUTO-UPDATE COMMANDS
// =============================================================================

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn check_for_updates(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;

    match updater.check().await {
        Ok(Some(update)) => {
            let info = serde_json::json!({
                "version": update.version,
                "notes": update.body,
                "date": update.date.map(|d| d.to_string())
            });
            // Emit event for frontend
            let _ = app.emit("update-available", &info);
            Ok(Some(info))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(format!("Update check failed: {}", e)),
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;

    if let Some(update) = updater.check().await.map_err(|e| e.to_string())? {
        let app_clone = app.clone();

        update
            .download_and_install(
                move |downloaded, total| {
                    if let Some(total) = total {
                        let progress = (downloaded as f64 / total as f64) * 100.0;
                        let _ = app_clone.emit("update-progress", progress);
                    }
                },
                || {
                    // Called before the installation - we could emit an event here
                },
            )
            .await
            .map_err(|e| format!("Install failed: {}", e))?;
    }

    Ok(())
}

fn main() {
    // Initialize logging first - keep the guard alive for the app lifetime
    let _log_guard = match logger::init_logging() {
        Ok(guard) => Some(guard),
        Err(e) => {
            eprintln!("[Harness] Failed to initialize logging: {}", e);
            None
        }
    };

    let config_path = config_path();
    let config = match load_agents_config(&config_path) {
        Ok(cfg) => cfg,
        Err(err) => {
            tracing::error!(
                path = %config_path.display(),
                error = %err,
                "Failed to load agents config"
            );
            AgentsConfig {
                version: Some(1),
                max_parallel: Some(5),
                agents: Vec::new(),
            }
        }
    };

    // Initialize database
    let db_path = db_path().expect("failed to get db path");
    let db_conn = db::init_db(&db_path).expect("failed to initialize database");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state() == ShortcutState::Pressed {
                        let shortcut_str = shortcut.to_string();
                        if shortcut_str.contains("R") {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.eval("location.reload()");
                            }
                        } else if shortcut_str.contains("Alt+I")
                            || shortcut_str.contains("Option+I")
                        {
                            let mut opened = false;
                            for (_, window) in app.webview_windows() {
                                if window.is_focused().unwrap_or(false) {
                                    window.open_devtools();
                                    opened = true;
                                    break;
                                }
                            }
                            if !opened {
                                if let Some(window) = app.get_webview_window("main") {
                                    window.open_devtools();
                                }
                            }
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
            let shortcut = "CmdOrCtrl+R"
                .parse::<Shortcut>()
                .map_err(|e| e.to_string())?;
            app.global_shortcut()
                .register(shortcut)
                .map_err(|e| e.to_string())?;

            let devtools_shortcut = "CmdOrCtrl+Alt+I"
                .parse::<Shortcut>()
                .map_err(|e| e.to_string())?;
            app.global_shortcut()
                .register(devtools_shortcut)
                .map_err(|e| e.to_string())?;

            // Optional: lightweight debug HTTP server for automation/testing.
            // Enable with: PHANTOM_DEBUG_HTTP=1 (default port 43777)
            if std::env::var("PHANTOM_DEBUG_HTTP").ok().as_deref() == Some("1") {
                let state: State<AppState> = app.state();
                let db = state.db.clone();
                std::thread::spawn(move || {
                    if let Err(e) = start_debug_http(db) {
                        eprintln!("[Harness] debug http server failed: {e}");
                    }
                });
            }

            {
                let app_handle = app.handle().clone();
                let state = app.state::<AppState>().inner().clone();
                let settings = state.settings.blocking_lock().clone();
                tauri::async_runtime::spawn(async move {
                    ensure_discord_bot(&app_handle, &state, &settings).await;
                });
            }

            {
                let app_handle = app.handle().clone();
                let state = app.state::<AppState>().inner().clone();
                let settings = state.settings.blocking_lock().clone();
                if mcp_enabled(&settings) {
                    if let Some(token) = settings.mcp_token.clone() {
                        let port = settings.mcp_port.unwrap_or(DEFAULT_MCP_PORT);
                        let token = token.trim().to_string();
                        let config = McpConfig { port, token };
                        tauri::async_runtime::spawn(async move {
                            if let Err(err) = start_mcp_server(app_handle, state, config).await {
                                eprintln!("[Harness] MCP server failed: {err}");
                            }
                        });
                    }
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            let should_close = matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            );
            if !should_close {
                return;
            }
            let label = window.label();
            if label.starts_with("chat-") {
                let task_id = label.trim_start_matches("chat-").to_string();
                let state = window.app_handle().state::<AppState>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    cleanup_terminal_session_by_task(&state, &task_id).await;
                });
                return;
            }
            if label != "main" {
                return;
            }
            let app = window.app_handle();
            for (label, win) in app.webview_windows() {
                if label.starts_with("chat-") {
                    let _ = win.close();
                }
            }
        })
        .manage({
            let mut settings = load_settings_from_disk();
            let changed = ensure_mcp_settings(&mut settings);
            if changed {
                if let Err(err) = persist_settings(&settings) {
                    eprintln!("[Harness] Failed to persist MCP settings: {err}");
                }
            }
            AppState {
                config,
                sessions: Arc::new(Mutex::new(HashMap::new())),
                settings: Arc::new(Mutex::new(settings)),
                db: Arc::new(StdMutex::new(db_conn)),
                notification_windows: Arc::new(StdMutex::new(Vec::new())),
                agent_availability: Arc::new(StdMutex::new(HashMap::new())),
                running_tasks: Arc::new(Mutex::new(HashSet::new())),
                discord_bot: Arc::new(StdMutex::new(None)),
                pending_user_inputs: Arc::new(Mutex::new(HashMap::new())),
                pending_discord_tasks: Arc::new(Mutex::new(HashMap::new())),
                codex_command_cache: Arc::new(StdMutex::new(HashMap::new())),
                claude_command_cache: Arc::new(StdMutex::new(HashMap::new())),
                terminal_sessions: Arc::new(Mutex::new(HashMap::new())),
                task_terminal_sessions: Arc::new(Mutex::new(HashMap::new())),
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_agent_models,
            get_cached_models,
            get_all_cached_models,
            refresh_agent_models,
            get_enriched_models,
            get_codex_commands,
            get_claude_commands,
            // Mode commands
            get_agent_modes,
            get_cached_modes,
            get_all_cached_modes_cmd,
            refresh_agent_modes,
            pick_project_path,
            get_repo_branches,
            get_pr_ready_state,
            check_existing_pr,
            get_github_pr_url,
            open_external_url,
            create_agent_session,
            start_task,
            stop_task,
            soft_stop_task,
            start_pending_prompt,
            get_settings,
            save_settings,
            test_webhook,
            test_discord,
            get_agent_availability,
            refresh_agent_availability,
            get_agent_skills,
            toggle_skill,
            get_running_tasks,
            restart_all_agents,
            codex_login,
            codex_logout,
            check_codex_auth,
            codex_rate_limits,
            claude_login,
            claude_logout,
            check_claude_auth,
            claude_rate_limits,
            load_tasks,
            check_task_uncommitted_changes,
            get_task_diff_stats,
            get_task_diff_files,
            get_task_file_diff,
            delete_task,
            get_task_history,
            open_task_directory,
            start_terminal_session,
            terminal_write,
            terminal_resize,
            terminal_close,
            open_chat_window,
            send_chat_message,
            respond_to_permission,
            respond_to_user_input,
            dismiss_notifications_for_task,
            local_usage::local_usage_snapshot,
            claude_local_usage::claude_local_usage_snapshot,
            // Analytics cache commands
            get_cached_analytics,
            get_all_cached_analytics,
            save_analytics_cache,
            // Attachment commands
            save_attachment,
            get_pending_attachments,
            delete_attachment,
            get_attachment_base64,
            // Code review commands
            gather_code_review_context,
            // Auto-update commands
            check_for_updates,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
