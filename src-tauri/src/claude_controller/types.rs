use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    pub from: String,
    pub text: String,
    pub timestamp: String, // ISO string
    #[serde(default)]
    pub color: Option<String>,
    pub read: bool,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StructuredMessage {
    #[serde(rename = "shutdown_request")]
    ShutdownRequest {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        #[serde(default)]
        reason: Option<String>,
        timestamp: String,
    },
    #[serde(rename = "shutdown_approved")]
    ShutdownApproved {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        timestamp: String,
        #[serde(default)]
        #[serde(rename = "paneId")]
        pane_id: Option<String>,
        #[serde(default)]
        #[serde(rename = "backendType")]
        backend_type: Option<String>,
    },
    #[serde(rename = "idle_notification")]
    IdleNotification {
        from: String,
        timestamp: String,
        #[serde(rename = "idleReason")]
        idle_reason: String,
    },
    #[serde(rename = "plan_approval_request")]
    PlanApprovalRequest {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        #[serde(default)]
        #[serde(rename = "planContent")]
        plan_content: Option<String>,
        timestamp: String,
    },
    #[serde(rename = "plan_approval_response")]
    PlanApprovalResponse {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        approved: bool,
        #[serde(default)]
        feedback: Option<String>,
        timestamp: String,
    },
    #[serde(rename = "permission_request")]
    PermissionRequest {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(default)]
        #[serde(rename = "toolUseId")]
        tool_use_id: Option<String>,
        description: String,
        #[serde(default)]
        input: Option<serde_json::Value>,
        #[serde(default)]
        #[serde(rename = "permissionSuggestions")]
        permission_suggestions: Option<Vec<String>>,
        timestamp: String,
    },
    #[serde(rename = "permission_response")]
    PermissionResponse {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        approved: bool,
        timestamp: String,
    },
    #[serde(rename = "plain_text")]
    PlainText { text: String },
}

#[derive(Debug, Clone)]
pub struct PollEvent {
    pub raw: InboxMessage,
    pub parsed: ParsedMessage,
}

#[derive(Debug, Clone)]
pub enum ParsedMessage {
    Structured(StructuredMessage),
    PlainText(String),
}
