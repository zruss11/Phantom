use serde::{Deserialize, Serialize};

// ============== GitHub Types ==============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubLabel {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubIssue {
    pub id: u64,
    pub number: u32,
    pub title: String,
    pub state: String,
    pub html_url: String,
    pub labels: Vec<GithubLabel>,
    pub assignee: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubWorkflow {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub branch: String,
    pub run_number: u32,
    pub html_url: String,
    pub created_at: String,
    pub duration_seconds: Option<u64>,
    pub repo: String,
}

// ============== Linear Types ==============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearLabel {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearState {
    pub name: String,
    pub color: String,
    #[serde(rename = "type")]
    pub state_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearIssue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub priority: u8,
    pub state: LinearState,
    pub labels: Vec<LinearLabel>,
    pub assignee: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub url: String,
    pub project: Option<String>,
    pub cycle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearProject {
    pub id: String,
    pub name: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearCycle {
    pub id: String,
    pub name: Option<String>,
    pub number: u32,
    pub starts_at: String,
    pub ends_at: String,
}

// ============== Sentry Types ==============

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryMetadata {
    pub filename: Option<String>,
    pub function: Option<String>,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryError {
    pub id: String,
    pub title: String,
    pub culprit: String,
    pub short_id: String,
    pub count: u64,
    pub user_count: u64,
    pub first_seen: String,
    pub last_seen: String,
    pub level: String,
    pub status: String,
    pub permalink: String,
    pub project: String,
    pub metadata: SentryMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryOrganization {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryProject {
    pub slug: String,
    pub name: String,
    pub id: String,
}

// ============== Combined Response ==============

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommandCenterData {
    pub github_issues: Vec<GithubIssue>,
    pub github_workflows: Vec<GithubWorkflow>,
    pub linear_issues: Vec<LinearIssue>,
    pub sentry_errors: Vec<SentryError>,
    pub last_updated: String,
    pub errors: Vec<String>,
}

// ============== Auth Status Types ==============

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GhCliAuthStatus {
    pub available: bool,
    pub username: Option<String>,
    pub error: Option<String>,
}
