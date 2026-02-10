use crate::cli::{ImageContent, PermissionOption};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerState {
    pub name: String,
    pub status: String,
}

/// Browser-facing session snapshot (Companion-style).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: String,
    pub model: String,
    pub cwd: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(rename = "permissionMode")]
    pub permission_mode: String,
    pub claude_code_version: String,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerState>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub slash_commands: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    pub total_cost_usd: f64,
    pub num_turns: i64,
    pub context_used_percent: i64,
    pub is_compacting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub input: Value,
    #[serde(default)]
    pub permission_suggestions: Option<Value>,
    /// UI-friendly options for Phantom (allow/deny).
    #[serde(default)]
    pub options: Vec<PermissionOption>,
    #[serde(default)]
    pub description: Option<String>,
    pub tool_use_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    pub timestamp: i64,
}

/// Messages the browser sends to the bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserOutgoingMessage {
    UserMessage {
        content: String,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        images: Option<Vec<ImageContent>>,
    },
    PermissionResponse {
        request_id: String,
        behavior: String,
        #[serde(default)]
        updated_input: Option<Value>,
        #[serde(default)]
        updated_permissions: Option<Value>,
        #[serde(default)]
        message: Option<String>,
    },
    Interrupt,
    SetModel {
        model: String,
    },
    SetPermissionMode {
        mode: String,
    },
}

/// Messages the bridge sends to the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserIncomingMessage {
    SessionInit {
        session: SessionState,
    },
    SessionUpdate {
        session: Value,
    },
    Assistant {
        message: Value,
        parent_tool_use_id: Option<String>,
    },
    StreamEvent {
        event: Value,
        parent_tool_use_id: Option<String>,
    },
    Result {
        data: Value,
    },
    PermissionRequest {
        request: PermissionRequest,
    },
    PermissionCancelled {
        request_id: String,
    },
    ToolProgress {
        tool_use_id: String,
        tool_name: String,
        elapsed_time_seconds: f64,
    },
    ToolUseSummary {
        summary: String,
        tool_use_ids: Vec<String>,
    },
    StatusChange {
        status: Option<String>,
    },
    AuthStatus {
        #[serde(rename = "isAuthenticating")]
        is_authenticating: bool,
        output: Vec<String>,
        #[serde(default)]
        error: Option<String>,
    },
    Error {
        message: String,
    },
    CliDisconnected,
    CliConnected,
    UserMessage {
        content: String,
        timestamp: i64,
    },
    MessageHistory {
        messages: Vec<BrowserIncomingMessage>,
    },
}
