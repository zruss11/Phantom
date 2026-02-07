use crate::automations;
use crate::utils::safe_prefix;
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub agent_id: String,
    #[serde(rename = "codexAccountId")]
    pub codex_account_id: Option<String>,
    pub model: String,
    pub prompt: Option<String>,
    pub project_path: Option<String>,
    #[serde(rename = "worktreePath")]
    pub worktree_path: Option<String>,
    /// Git branch name (may differ from folder name after async rename)
    pub branch: Option<String>,
    pub status: String,
    pub status_state: String,
    pub cost: f64,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(rename = "titleSummary")]
    pub title_summary: Option<String>,
    /// Agent session ID for session/load support (context restoration)
    #[serde(rename = "agentSessionId")]
    pub agent_session_id: Option<String>,
    /// Total tokens used (for context indicator)
    #[serde(rename = "totalTokens")]
    pub total_tokens: Option<i64>,
    /// Model context window size (for context indicator)
    #[serde(rename = "contextWindow")]
    pub context_window: Option<i64>,
    /// Claude Code runtime ("native" or "docker")
    #[serde(rename = "claudeRuntime")]
    pub claude_runtime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRecord {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub agent_id: String,
    pub exec_model: String,
    pub prompt: String,
    pub project_path: Option<String>,
    pub base_branch: Option<String>,
    pub plan_mode: bool,
    pub thinking: bool,
    pub use_worktree: bool,
    pub permission_mode: String,
    pub reasoning_effort: Option<String>,
    pub agent_mode: Option<String>,
    pub codex_mode: Option<String>,
    pub claude_runtime: Option<String>,
    pub cron: String,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunRecord {
    pub id: String,
    pub automation_id: String,
    pub task_id: Option<String>,
    pub scheduled_for: i64,
    pub created_at: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedModel {
    pub value: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMode {
    pub value: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexAccountRecord {
    pub id: String,
    pub label: Option<String>,
    #[serde(rename = "codexHome")]
    pub codex_home: String,
    pub email: Option<String>,
    #[serde(rename = "planType")]
    pub plan_type: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentRecord {
    pub id: String,
    #[serde(rename = "fileName")]
    pub file_name: Option<String>,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(rename = "relativePath")]
    pub relative_path: String,
    #[serde(rename = "byteSize")]
    pub byte_size: i64,
}

/// Structured message record for history formatting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    pub id: i64,
    pub message_type: String,
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub tool_name: Option<String>,
    pub tool_arguments: Option<String>,
    pub tool_return: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSessionRecord {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub capture_mic: bool,
    pub capture_system: bool,
    pub started_at: Option<i64>,
    pub stopped_at: Option<i64>,
    pub duration_ms: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSegmentRecord {
    pub id: i64,
    pub session_id: String,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: Option<String>,
    pub created_at: i64,
}

pub fn init_db(path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    // PRAGMAs that return results - use query_row to consume them
    let _ = conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()));
    let _ = conn.query_row("PRAGMA synchronous = NORMAL", [], |_| Ok(()));
    let _ = conn.query_row("PRAGMA temp_store = MEMORY", [], |_| Ok(()));
    let _ = conn.query_row("PRAGMA cache_size = -16000", [], |_| Ok(()));
    let _ = conn.query_row("PRAGMA mmap_size = 268435456", [], |_| Ok(())); // 256MB memory-mapped I/O
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            codex_account_id TEXT,
            model TEXT NOT NULL,
            prompt TEXT,
            project_path TEXT,
            worktree_path TEXT,
            status TEXT DEFAULT 'Ready',
            status_state TEXT DEFAULT 'idle',
            cost REAL DEFAULT 0.0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            claude_runtime TEXT
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS automations (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            agent_id TEXT NOT NULL,
            exec_model TEXT NOT NULL,
            prompt TEXT NOT NULL,
            project_path TEXT,
            base_branch TEXT,
            plan_mode INTEGER NOT NULL DEFAULT 0,
            thinking INTEGER NOT NULL DEFAULT 1,
            use_worktree INTEGER NOT NULL DEFAULT 1,
            permission_mode TEXT NOT NULL DEFAULT 'default',
            reasoning_effort TEXT,
            agent_mode TEXT,
            codex_mode TEXT,
            claude_runtime TEXT,
            cron TEXT NOT NULL,
            next_run_at INTEGER,
            last_run_at INTEGER,
            last_error TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;

    // Add missing columns to automations table (migration).
    // Some users may have an older schema from an early build/branch that created
    // the table without all the fields we now depend on (like next_run_at).
    conn.execute(
        "ALTER TABLE automations ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN agent_id TEXT NOT NULL DEFAULT 'codex'",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN exec_model TEXT NOT NULL DEFAULT 'default'",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN prompt TEXT NOT NULL DEFAULT ''",
        [],
    )
    .ok();
    conn.execute("ALTER TABLE automations ADD COLUMN project_path TEXT", [])
        .ok();
    conn.execute("ALTER TABLE automations ADD COLUMN base_branch TEXT", [])
        .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN plan_mode INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN thinking INTEGER NOT NULL DEFAULT 1",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN use_worktree INTEGER NOT NULL DEFAULT 1",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN permission_mode TEXT NOT NULL DEFAULT 'default'",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN reasoning_effort TEXT",
        [],
    )
    .ok();
    conn.execute("ALTER TABLE automations ADD COLUMN agent_mode TEXT", [])
        .ok();
    conn.execute("ALTER TABLE automations ADD COLUMN codex_mode TEXT", [])
        .ok();
    conn.execute("ALTER TABLE automations ADD COLUMN claude_runtime TEXT", [])
        .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN cron TEXT NOT NULL DEFAULT ''",
        [],
    )
    .ok();
    conn.execute("ALTER TABLE automations ADD COLUMN next_run_at INTEGER", [])
        .ok();
    conn.execute("ALTER TABLE automations ADD COLUMN last_run_at INTEGER", [])
        .ok();
    conn.execute("ALTER TABLE automations ADD COLUMN last_error TEXT", [])
        .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automations ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .ok();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS automation_runs (
            id TEXT PRIMARY KEY,
            automation_id TEXT NOT NULL,
            task_id TEXT,
            scheduled_for INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            error TEXT,
            FOREIGN KEY (automation_id) REFERENCES automations(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Add missing columns to automation_runs table (migration).
    // Older databases may have automation_runs without created_at (and potentially other fields).
    conn.execute(
        "ALTER TABLE automation_runs ADD COLUMN automation_id TEXT NOT NULL DEFAULT ''",
        [],
    )
    .ok();
    conn.execute("ALTER TABLE automation_runs ADD COLUMN task_id TEXT", [])
        .ok();
    conn.execute(
        "ALTER TABLE automation_runs ADD COLUMN scheduled_for INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .ok();
    conn.execute(
        "ALTER TABLE automation_runs ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .ok();
    conn.execute("ALTER TABLE automation_runs ADD COLUMN error TEXT", [])
        .ok();

    // Model cache table for instant model selector UX
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cached_models (
            agent_id TEXT NOT NULL,
            value TEXT NOT NULL,
            name TEXT,
            description TEXT,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (agent_id, value)
        )",
        [],
    )?;

    // Mode cache table for instant mode selector UX
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cached_modes (
            agent_id TEXT NOT NULL,
            value TEXT NOT NULL,
            name TEXT,
            description TEXT,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (agent_id, value)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS codex_accounts (
            id TEXT PRIMARY KEY,
            label TEXT,
            codex_home TEXT NOT NULL,
            email TEXT,
            plan_type TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;

    // Chat messages table for session persistence
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            message_type TEXT NOT NULL,
            content TEXT,
            reasoning TEXT,
            tool_name TEXT,
            tool_arguments TEXT,
            tool_return TEXT,
            timestamp TEXT NOT NULL,
            FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // pending_attachments stores images uploaded before task creation
    // No FK constraint since task_id may be a temporary ID that doesn't exist yet
    // Drop old table if it exists (had FK constraint that prevented pre-task uploads)
    conn.execute("DROP TABLE IF EXISTS pending_attachments", [])?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS pending_attachments (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            file_name TEXT,
            mime_type TEXT,
            relative_path TEXT NOT NULL,
            byte_size INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS message_attachments (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            message_id INTEGER NOT NULL,
            file_name TEXT,
            mime_type TEXT,
            relative_path TEXT NOT NULL,
            byte_size INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
            FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Analytics cache table for instant dashboard loading
    conn.execute(
        "CREATE TABLE IF NOT EXISTS analytics_cache (
            agent_type TEXT PRIMARY KEY,
            snapshot_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS discord_threads (
            task_id TEXT PRIMARY KEY,
            thread_id INTEGER NOT NULL,
            channel_id INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_automations_enabled_next_run_at
         ON automations(enabled, next_run_at)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_automation_runs_automation_id_created_at
         ON automation_runs(automation_id, created_at DESC)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_task_id_id ON messages(task_id, id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_message_attachments_task_id_message_id
         ON message_attachments(task_id, message_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_pending_attachments_task_id_created_at
         ON pending_attachments(task_id, created_at)",
        [],
    )?;

    // Meeting sessions table for meeting notes feature
    conn.execute(
        "CREATE TABLE IF NOT EXISTS meeting_sessions (
            id TEXT PRIMARY KEY,
            title TEXT,
            status TEXT NOT NULL DEFAULT 'idle',
            capture_mic INTEGER NOT NULL DEFAULT 1,
            capture_system INTEGER NOT NULL DEFAULT 0,
            started_at INTEGER,
            stopped_at INTEGER,
            duration_ms INTEGER DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;

    // Meeting segments table for transcription segments
    conn.execute(
        "CREATE TABLE IF NOT EXISTS meeting_segments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            text TEXT NOT NULL,
            start_ms INTEGER NOT NULL,
            end_ms INTEGER NOT NULL,
            speaker TEXT,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (session_id) REFERENCES meeting_sessions(id) ON DELETE CASCADE
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_meeting_segments_session
         ON meeting_segments(session_id, start_ms)",
        [],
    )?;

    // Enable foreign keys for cascade delete
    conn.execute("PRAGMA foreign_keys = ON", [])?;

    // Add title_summary column if it doesn't exist (migration)
    conn.execute("ALTER TABLE tasks ADD COLUMN title_summary TEXT", [])
        .ok(); // Ignore error if column already exists

    // Add agent_session_id column for session/load support (migration)
    conn.execute("ALTER TABLE tasks ADD COLUMN agent_session_id TEXT", [])
        .ok(); // Ignore error if column already exists
               // Backfill from legacy acp_session_id if present
    conn.execute(
        "UPDATE tasks SET agent_session_id = acp_session_id WHERE agent_session_id IS NULL",
        [],
    )
    .ok();

    // Add token usage columns for context indicator (migration)
    conn.execute("ALTER TABLE tasks ADD COLUMN total_tokens INTEGER", [])
        .ok();
    conn.execute("ALTER TABLE tasks ADD COLUMN context_window INTEGER", [])
        .ok();
    conn.execute("ALTER TABLE tasks ADD COLUMN worktree_path TEXT", [])
        .ok();
    // Add branch column for git branch name (may differ from folder after async rename)
    conn.execute("ALTER TABLE tasks ADD COLUMN branch TEXT", [])
        .ok();
    // Add codex_account_id column for Codex account routing (migration)
    conn.execute("ALTER TABLE tasks ADD COLUMN codex_account_id TEXT", [])
        .ok();
    // Add claude_runtime column for Docker runtime support (migration)
    conn.execute("ALTER TABLE tasks ADD COLUMN claude_runtime TEXT", [])
        .ok();

    // Backfill next_run_at for enabled automations that predate the column (migration).
    // Older schemas added next_run_at without populating it, which would cause enabled
    // schedules to never run unless a user edits/toggles them.
    if let Ok(rows) = list_enabled_automations_missing_next_run_at(&conn) {
        let now = chrono::Utc::now().timestamp();
        for (automation_id, cron) in rows {
            if let Ok(next_run_at) = automations::compute_next_run_at(&cron, chrono::Local::now()) {
                let _ = backfill_automation_next_run_at(&conn, &automation_id, next_run_at, now);
            }
        }
    }

    Ok(conn)
}

pub fn list_enabled_automations_missing_next_run_at(
    conn: &Connection,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, cron
         FROM automations
         WHERE enabled = 1
           AND (next_run_at IS NULL OR next_run_at = 0)
           AND TRIM(cron) <> ''",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn backfill_automation_next_run_at(
    conn: &Connection,
    automation_id: &str,
    next_run_at: i64,
    updated_at: i64,
) -> Result<usize> {
    conn.execute(
        "UPDATE automations
         SET next_run_at = ?1, updated_at = ?2
         WHERE id = ?3
           AND enabled = 1
           AND (next_run_at IS NULL OR next_run_at = 0)",
        params![next_run_at, updated_at, automation_id],
    )
}

pub fn list_automations(conn: &Connection) -> Result<Vec<AutomationRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, enabled, agent_id, exec_model, prompt, project_path, base_branch, plan_mode, thinking, use_worktree, permission_mode, reasoning_effort, agent_mode, codex_mode, claude_runtime, cron, next_run_at, last_run_at, last_error, created_at, updated_at
         FROM automations
         ORDER BY enabled DESC, COALESCE(next_run_at, 0) ASC, created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AutomationRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            agent_id: row.get(3)?,
            exec_model: row.get(4)?,
            prompt: row.get(5)?,
            project_path: row.get(6)?,
            base_branch: row.get(7)?,
            plan_mode: row.get::<_, i64>(8)? != 0,
            thinking: row.get::<_, i64>(9)? != 0,
            use_worktree: row.get::<_, i64>(10)? != 0,
            permission_mode: row.get(11)?,
            reasoning_effort: row.get(12)?,
            agent_mode: row.get(13)?,
            codex_mode: row.get(14)?,
            claude_runtime: row.get(15)?,
            cron: row.get(16)?,
            next_run_at: row.get(17)?,
            last_run_at: row.get(18)?,
            last_error: row.get(19)?,
            created_at: row.get(20)?,
            updated_at: row.get(21)?,
        })
    })?;
    rows.collect()
}

pub fn get_automation(conn: &Connection, automation_id: &str) -> Result<Option<AutomationRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, enabled, agent_id, exec_model, prompt, project_path, base_branch, plan_mode, thinking, use_worktree, permission_mode, reasoning_effort, agent_mode, codex_mode, claude_runtime, cron, next_run_at, last_run_at, last_error, created_at, updated_at
         FROM automations
         WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![automation_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(AutomationRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            agent_id: row.get(3)?,
            exec_model: row.get(4)?,
            prompt: row.get(5)?,
            project_path: row.get(6)?,
            base_branch: row.get(7)?,
            plan_mode: row.get::<_, i64>(8)? != 0,
            thinking: row.get::<_, i64>(9)? != 0,
            use_worktree: row.get::<_, i64>(10)? != 0,
            permission_mode: row.get(11)?,
            reasoning_effort: row.get(12)?,
            agent_mode: row.get(13)?,
            codex_mode: row.get(14)?,
            claude_runtime: row.get(15)?,
            cron: row.get(16)?,
            next_run_at: row.get(17)?,
            last_run_at: row.get(18)?,
            last_error: row.get(19)?,
            created_at: row.get(20)?,
            updated_at: row.get(21)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn insert_automation(conn: &Connection, automation: &AutomationRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO automations (id, name, enabled, agent_id, exec_model, prompt, project_path, base_branch, plan_mode, thinking, use_worktree, permission_mode, reasoning_effort, agent_mode, codex_mode, claude_runtime, cron, next_run_at, last_run_at, last_error, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        params![
            automation.id,
            automation.name,
            if automation.enabled { 1 } else { 0 },
            automation.agent_id,
            automation.exec_model,
            automation.prompt,
            automation.project_path,
            automation.base_branch,
            if automation.plan_mode { 1 } else { 0 },
            if automation.thinking { 1 } else { 0 },
            if automation.use_worktree { 1 } else { 0 },
            automation.permission_mode,
            automation.reasoning_effort,
            automation.agent_mode,
            automation.codex_mode,
            automation.claude_runtime,
            automation.cron,
            automation.next_run_at,
            automation.last_run_at,
            automation.last_error,
            automation.created_at,
            automation.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update_automation(conn: &Connection, automation: &AutomationRecord) -> Result<()> {
    conn.execute(
        "UPDATE automations SET
            name = ?2,
            enabled = ?3,
            agent_id = ?4,
            exec_model = ?5,
            prompt = ?6,
            project_path = ?7,
            base_branch = ?8,
            plan_mode = ?9,
            thinking = ?10,
            use_worktree = ?11,
            permission_mode = ?12,
            reasoning_effort = ?13,
            agent_mode = ?14,
            codex_mode = ?15,
            claude_runtime = ?16,
            cron = ?17,
            next_run_at = ?18,
            last_run_at = ?19,
            last_error = ?20,
            updated_at = ?21
         WHERE id = ?1",
        params![
            automation.id,
            automation.name,
            if automation.enabled { 1 } else { 0 },
            automation.agent_id,
            automation.exec_model,
            automation.prompt,
            automation.project_path,
            automation.base_branch,
            if automation.plan_mode { 1 } else { 0 },
            if automation.thinking { 1 } else { 0 },
            if automation.use_worktree { 1 } else { 0 },
            automation.permission_mode,
            automation.reasoning_effort,
            automation.agent_mode,
            automation.codex_mode,
            automation.claude_runtime,
            automation.cron,
            automation.next_run_at,
            automation.last_run_at,
            automation.last_error,
            automation.updated_at,
        ],
    )?;
    Ok(())
}

pub fn delete_automation(conn: &Connection, automation_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM automations WHERE id = ?1",
        params![automation_id],
    )?;
    Ok(())
}

pub fn set_automation_last_error(
    conn: &Connection,
    automation_id: &str,
    last_error: Option<String>,
    updated_at: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE automations
         SET last_error = ?1, updated_at = ?2
         WHERE id = ?3",
        params![last_error, updated_at, automation_id],
    )?;
    Ok(())
}

pub fn list_due_automations(conn: &Connection, now: i64) -> Result<Vec<AutomationRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, enabled, agent_id, exec_model, prompt, project_path, base_branch, plan_mode, thinking, use_worktree, permission_mode, reasoning_effort, agent_mode, codex_mode, claude_runtime, cron, next_run_at, last_run_at, last_error, created_at, updated_at
         FROM automations
         WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?1
         ORDER BY next_run_at ASC",
    )?;
    let rows = stmt.query_map(params![now], |row| {
        Ok(AutomationRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            agent_id: row.get(3)?,
            exec_model: row.get(4)?,
            prompt: row.get(5)?,
            project_path: row.get(6)?,
            base_branch: row.get(7)?,
            plan_mode: row.get::<_, i64>(8)? != 0,
            thinking: row.get::<_, i64>(9)? != 0,
            use_worktree: row.get::<_, i64>(10)? != 0,
            permission_mode: row.get(11)?,
            reasoning_effort: row.get(12)?,
            agent_mode: row.get(13)?,
            codex_mode: row.get(14)?,
            claude_runtime: row.get(15)?,
            cron: row.get(16)?,
            next_run_at: row.get(17)?,
            last_run_at: row.get(18)?,
            last_error: row.get(19)?,
            created_at: row.get(20)?,
            updated_at: row.get(21)?,
        })
    })?;
    rows.collect()
}

pub fn insert_automation_run(conn: &Connection, run: &AutomationRunRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO automation_runs (id, automation_id, task_id, scheduled_for, created_at, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            run.id,
            run.automation_id,
            run.task_id,
            run.scheduled_for,
            run.created_at,
            run.error,
        ],
    )?;
    Ok(())
}

pub fn list_automation_runs(conn: &Connection, limit: usize) -> Result<Vec<AutomationRunRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, automation_id, task_id, scheduled_for, created_at, error
         FROM automation_runs
         ORDER BY created_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(AutomationRunRecord {
            id: row.get(0)?,
            automation_id: row.get(1)?,
            task_id: row.get(2)?,
            scheduled_for: row.get(3)?,
            created_at: row.get(4)?,
            error: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn save_discord_thread(
    conn: &Connection,
    task_id: &str,
    thread_id: u64,
    channel_id: u64,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT OR REPLACE INTO discord_threads (task_id, thread_id, channel_id, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![task_id, thread_id as i64, channel_id as i64, now],
    )?;
    Ok(())
}

pub fn get_discord_thread_id(conn: &Connection, task_id: &str) -> Result<Option<u64>> {
    let mut stmt = conn.prepare("SELECT thread_id FROM discord_threads WHERE task_id = ?1")?;
    let mut rows = stmt.query(params![task_id])?;
    if let Some(row) = rows.next()? {
        let thread_id: i64 = row.get(0)?;
        return Ok(Some(thread_id as u64));
    }
    Ok(None)
}

pub fn get_task_id_for_discord_thread(conn: &Connection, thread_id: u64) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT task_id FROM discord_threads WHERE thread_id = ?1")?;
    let mut rows = stmt.query(params![thread_id as i64])?;
    if let Some(row) = rows.next()? {
        let task_id: String = row.get(0)?;
        return Ok(Some(task_id));
    }
    Ok(None)
}

/// Save analytics snapshot to cache
pub fn save_analytics_cache(
    conn: &Connection,
    agent_type: &str,
    snapshot_json: &str,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT OR REPLACE INTO analytics_cache (agent_type, snapshot_json, updated_at)
         VALUES (?1, ?2, ?3)",
        params![agent_type, snapshot_json, now],
    )?;
    Ok(())
}

/// Load analytics snapshot from cache
pub fn get_analytics_cache(conn: &Connection, agent_type: &str) -> Result<Option<(String, i64)>> {
    let result = conn.query_row(
        "SELECT snapshot_json, updated_at FROM analytics_cache WHERE agent_type = ?1",
        params![agent_type],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    );
    match result {
        Ok((json, updated_at)) => Ok(Some((json, updated_at))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Get all cached analytics (for startup preload)
pub fn get_all_analytics_cache(conn: &Connection) -> Result<Vec<(String, String, i64)>> {
    let mut stmt =
        conn.prepare("SELECT agent_type, snapshot_json, updated_at FROM analytics_cache")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    rows.collect()
}

/// Get cached models for an agent (returns empty vec if none cached)
pub fn get_cached_models(conn: &Connection, agent_id: &str) -> Result<Vec<CachedModel>> {
    let mut stmt = conn.prepare_cached(
        "SELECT value, name, description FROM cached_models WHERE agent_id = ?1 ORDER BY rowid",
    )?;
    let models = stmt.query_map(params![agent_id], |row| {
        Ok(CachedModel {
            value: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
        })
    })?;
    models.collect()
}

/// Save models to cache (replaces all models for agent)
pub fn save_cached_models(
    conn: &mut Connection,
    agent_id: &str,
    models: &[CachedModel],
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let tx = conn.transaction()?;

    // Delete existing models for this agent
    tx.execute(
        "DELETE FROM cached_models WHERE agent_id = ?1",
        params![agent_id],
    )?;

    // Insert new models
    for model in models {
        tx.execute(
            "INSERT INTO cached_models (agent_id, value, name, description, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![agent_id, model.value, model.name, model.description, now],
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// Get all cached models for all agents (for startup preload)
pub fn get_all_cached_models(conn: &Connection) -> Result<Vec<(String, Vec<CachedModel>)>> {
    let mut stmt = conn.prepare("SELECT DISTINCT agent_id FROM cached_models")?;
    let agent_ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut result = Vec::new();
    for agent_id in agent_ids {
        let models = get_cached_models(conn, &agent_id)?;
        if !models.is_empty() {
            result.push((agent_id, models));
        }
    }
    Ok(result)
}

/// Get cached modes for an agent (returns empty vec if none cached)
pub fn get_cached_modes(conn: &Connection, agent_id: &str) -> Result<Vec<CachedMode>> {
    let mut stmt = conn.prepare_cached(
        "SELECT value, name, description FROM cached_modes WHERE agent_id = ?1 ORDER BY rowid",
    )?;
    let modes = stmt.query_map(params![agent_id], |row| {
        Ok(CachedMode {
            value: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
        })
    })?;
    modes.collect()
}

/// Save modes to cache (replaces all modes for agent)
pub fn save_cached_modes(
    conn: &mut Connection,
    agent_id: &str,
    modes: &[CachedMode],
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let tx = conn.transaction()?;

    // Delete existing modes for this agent
    tx.execute(
        "DELETE FROM cached_modes WHERE agent_id = ?1",
        params![agent_id],
    )?;

    // Insert new modes
    for mode in modes {
        tx.execute(
            "INSERT INTO cached_modes (agent_id, value, name, description, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![agent_id, mode.value, mode.name, mode.description, now],
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// Get all cached modes for all agents (for startup preload)
pub fn get_all_cached_modes(conn: &Connection) -> Result<Vec<(String, Vec<CachedMode>)>> {
    let mut stmt = conn.prepare("SELECT DISTINCT agent_id FROM cached_modes")?;
    let agent_ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut result = Vec::new();
    for agent_id in agent_ids {
        let modes = get_cached_modes(conn, &agent_id)?;
        if !modes.is_empty() {
            result.push((agent_id, modes));
        }
    }
    Ok(result)
}

pub fn insert_task(conn: &Connection, task: &TaskRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO tasks (id, agent_id, codex_account_id, model, prompt, project_path, worktree_path, branch, status, status_state, cost, created_at, updated_at, title_summary, agent_session_id, total_tokens, context_window, claude_runtime)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            task.id,
            task.agent_id,
            task.codex_account_id,
            task.model,
            task.prompt,
            task.project_path,
            task.worktree_path,
            task.branch,
            task.status,
            task.status_state,
            task.cost,
            task.created_at,
            task.updated_at,
            task.title_summary,
            task.agent_session_id,
            task.total_tokens,
            task.context_window,
            task.claude_runtime,
        ],
    )?;
    Ok(())
}

pub fn update_task_status(conn: &Connection, id: &str, status: &str, state: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE tasks SET status = ?1, status_state = ?2, updated_at = ?3 WHERE id = ?4",
        params![status, state, now, id],
    )?;
    Ok(())
}

pub fn update_task_branch(conn: &Connection, id: &str, branch: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE tasks SET branch = ?1, updated_at = ?2 WHERE id = ?3",
        params![branch, now, id],
    )?;
    Ok(())
}

pub fn update_task_cost(conn: &Connection, id: &str, cost: f64) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE tasks SET cost = ?1, updated_at = ?2 WHERE id = ?3",
        params![cost, now, id],
    )?;
    Ok(())
}

pub fn update_task_token_usage(
    conn: &Connection,
    id: &str,
    total_tokens: i64,
    context_window: Option<i64>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE tasks SET total_tokens = ?1, context_window = ?2, updated_at = ?3 WHERE id = ?4",
        params![total_tokens, context_window, now, id],
    )?;
    Ok(())
}

pub fn update_task_title_summary(conn: &Connection, id: &str, title: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE tasks SET title_summary = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now, id],
    )?;
    Ok(())
}

/// Save the Agent session ID for session/load context restoration
pub fn update_task_agent_session_id(
    conn: &Connection,
    id: &str,
    agent_session_id: &str,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE tasks SET agent_session_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![agent_session_id, now, id],
    )?;
    Ok(())
}

pub fn update_task_codex_account_id(
    conn: &Connection,
    id: &str,
    codex_account_id: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE tasks SET codex_account_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![codex_account_id, now, id],
    )?;
    Ok(())
}

/// Get the Agent session ID for a task (for session/load restoration)
#[allow(dead_code)]
pub fn get_task_agent_session_id(conn: &Connection, id: &str) -> Result<Option<String>> {
    let result = conn.query_row(
        "SELECT agent_session_id FROM tasks WHERE id = ?1",
        params![id],
        |row| row.get::<_, Option<String>>(0),
    );
    match result {
        Ok(session_id) => Ok(session_id),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn get_task_cost(conn: &Connection, id: &str) -> Result<f64> {
    let cost: f64 = conn.query_row("SELECT cost FROM tasks WHERE id = ?1", params![id], |row| {
        row.get(0)
    })?;
    Ok(cost)
}

pub fn delete_task(conn: &Connection, id: &str) -> Result<()> {
    // Messages are auto-deleted via CASCADE
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    Ok(())
}

/// Save a chat message to the database
pub fn save_message(
    conn: &Connection,
    task_id: &str,
    message_type: &str,
    content: Option<&str>,
    reasoning: Option<&str>,
    tool_name: Option<&str>,
    tool_arguments: Option<&str>,
    tool_return: Option<&str>,
    timestamp: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO messages (task_id, message_type, content, reasoning, tool_name, tool_arguments, tool_return, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![task_id, message_type, content, reasoning, tool_name, tool_arguments, tool_return, timestamp],
    )?;
    Ok(conn.last_insert_rowid())
}

#[allow(dead_code)]
pub fn save_message_attachments(
    conn: &Connection,
    task_id: &str,
    message_id: i64,
    attachments: &[AttachmentRecord],
) -> Result<()> {
    if attachments.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().timestamp();
    for attachment in attachments {
        conn.execute(
            "INSERT OR REPLACE INTO message_attachments
             (id, task_id, message_id, file_name, mime_type, relative_path, byte_size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                attachment.id,
                task_id,
                message_id,
                attachment.file_name,
                attachment.mime_type,
                attachment.relative_path,
                attachment.byte_size,
                now
            ],
        )?;
    }
    Ok(())
}

pub fn save_pending_attachments(
    conn: &mut Connection,
    task_id: &str,
    attachments: &[AttachmentRecord],
) -> Result<()> {
    if attachments.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().timestamp();
    let tx = conn.transaction()?;
    for attachment in attachments {
        tx.execute(
            "INSERT OR REPLACE INTO pending_attachments
             (id, task_id, file_name, mime_type, relative_path, byte_size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                attachment.id,
                task_id,
                attachment.file_name,
                attachment.mime_type,
                attachment.relative_path,
                attachment.byte_size,
                now
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn get_pending_attachments(conn: &Connection, task_id: &str) -> Result<Vec<AttachmentRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_name, mime_type, relative_path, byte_size
         FROM pending_attachments WHERE task_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        Ok(AttachmentRecord {
            id: row.get(0)?,
            file_name: row.get(1)?,
            mime_type: row.get(2)?,
            relative_path: row.get(3)?,
            byte_size: row.get(4)?,
        })
    })?;
    rows.collect()
}

#[allow(dead_code)]
pub fn clear_pending_attachments(conn: &Connection, task_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM pending_attachments WHERE task_id = ?1",
        params![task_id],
    )?;
    Ok(())
}

pub fn get_message_attachments(
    conn: &Connection,
    task_id: &str,
) -> Result<HashMap<i64, Vec<AttachmentRecord>>> {
    let mut stmt = conn.prepare_cached(
        "SELECT message_id, id, file_name, mime_type, relative_path, byte_size
         FROM message_attachments WHERE task_id = ?1 ORDER BY message_id ASC, rowid ASC",
    )?;
    let mut map: HashMap<i64, Vec<AttachmentRecord>> = HashMap::new();
    let rows = stmt.query_map(params![task_id], |row| {
        let message_id: i64 = row.get(0)?;
        let attachment = AttachmentRecord {
            id: row.get(1)?,
            file_name: row.get(2)?,
            mime_type: row.get(3)?,
            relative_path: row.get(4)?,
            byte_size: row.get(5)?,
        };
        Ok((message_id, attachment))
    })?;
    for row in rows {
        let (message_id, attachment) = row?;
        map.entry(message_id).or_default().push(attachment);
    }
    Ok(map)
}

#[allow(dead_code)]
pub fn list_attachment_relative_paths(conn: &Connection, task_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT relative_path FROM pending_attachments WHERE task_id = ?1
         UNION ALL
         SELECT relative_path FROM message_attachments WHERE task_id = ?1",
    )?;
    let rows = stmt.query_map(params![task_id], |row| row.get(0))?;
    rows.collect()
}

#[allow(dead_code)]
pub fn attachment_ref_count(
    conn: &Connection,
    relative_path: &str,
    exclude_task_id: Option<&str>,
) -> Result<i64> {
    if let Some(exclude) = exclude_task_id {
        conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT task_id FROM pending_attachments WHERE relative_path = ?1
                UNION ALL
                SELECT task_id FROM message_attachments WHERE relative_path = ?1
            ) WHERE task_id != ?2",
            params![relative_path, exclude],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT task_id FROM pending_attachments WHERE relative_path = ?1
                UNION ALL
                SELECT task_id FROM message_attachments WHERE relative_path = ?1
            )",
            params![relative_path],
            |row| row.get(0),
        )
    }
}

/// Load all messages for a task
pub fn get_messages(conn: &Connection, task_id: &str) -> Result<Vec<serde_json::Value>> {
    let attachments_by_message = get_message_attachments(conn, task_id).unwrap_or_default();
    let mut stmt = conn.prepare_cached(
        "SELECT id, message_type, content, reasoning, tool_name, tool_arguments, tool_return, timestamp
         FROM messages WHERE task_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        let message_id: i64 = row.get(0)?;
        let message_type: String = row.get(1)?;
        let content: Option<String> = row.get(2)?;
        let reasoning: Option<String> = row.get(3)?;
        let tool_name: Option<String> = row.get(4)?;
        let tool_arguments: Option<String> = row.get(5)?;
        let tool_return: Option<String> = row.get(6)?;
        let timestamp: String = row.get(7)?;

        // Build tool_call object if tool_name exists
        let tool_call = tool_name.as_ref().map(|name| {
            serde_json::json!({
                "name": name,
                "arguments": tool_arguments
            })
        });

        Ok((
            message_id,
            serde_json::json!({
                "message_type": message_type,
                "content": content,
                "reasoning": reasoning,
                "tool_call": tool_call,
                "tool_return": tool_return,
                "timestamp": timestamp
            }),
        ))
    })?;

    let mut messages = Vec::new();
    for row in rows {
        let (message_id, mut message) = row?;
        if let Some(attachments) = attachments_by_message.get(&message_id) {
            message["attachments"] = serde_json::json!(attachments);
        }
        messages.push(message);
    }
    Ok(messages)
}

pub fn list_tasks(conn: &Connection) -> Result<Vec<TaskRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, agent_id, codex_account_id, model, prompt, project_path, worktree_path, branch, status, status_state, cost, created_at, updated_at, title_summary, agent_session_id, total_tokens, context_window, claude_runtime
         FROM tasks ORDER BY created_at DESC"
    )?;
    let tasks = stmt.query_map([], |row| {
        Ok(TaskRecord {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            codex_account_id: row.get(2)?,
            model: row.get(3)?,
            prompt: row.get(4)?,
            project_path: row.get(5)?,
            worktree_path: row.get(6)?,
            branch: row.get(7)?,
            status: row.get(8)?,
            status_state: row.get(9)?,
            cost: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            title_summary: row.get(13)?,
            agent_session_id: row.get(14)?,
            total_tokens: row.get(15)?,
            context_window: row.get(16)?,
            claude_runtime: row.get(17)?,
        })
    })?;
    tasks.collect()
}

pub fn list_codex_accounts(conn: &Connection) -> Result<Vec<CodexAccountRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, label, codex_home, email, plan_type, created_at, updated_at
         FROM codex_accounts ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CodexAccountRecord {
            id: row.get(0)?,
            label: row.get(1)?,
            codex_home: row.get(2)?,
            email: row.get(3)?,
            plan_type: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn get_codex_account(conn: &Connection, id: &str) -> Result<Option<CodexAccountRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, label, codex_home, email, plan_type, created_at, updated_at
         FROM codex_accounts WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(CodexAccountRecord {
            id: row.get(0)?,
            label: row.get(1)?,
            codex_home: row.get(2)?,
            email: row.get(3)?,
            plan_type: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn insert_codex_account(conn: &Connection, account: &CodexAccountRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO codex_accounts (id, label, codex_home, email, plan_type, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            account.id,
            account.label,
            account.codex_home,
            account.email,
            account.plan_type,
            account.created_at,
            account.updated_at
        ],
    )?;
    Ok(())
}

pub fn update_codex_account_meta(
    conn: &Connection,
    id: &str,
    email: Option<&str>,
    plan_type: Option<&str>,
    label: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE codex_accounts SET email = COALESCE(?1, email), plan_type = COALESCE(?2, plan_type), label = COALESCE(?3, label), updated_at = ?4 WHERE id = ?5",
        params![email, plan_type, label, now, id],
    )?;
    Ok(())
}

pub fn delete_codex_account(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM codex_accounts WHERE id = ?1", params![id])?;
    Ok(())
}

/// Get messages as structured MessageRecord for history formatting
pub fn get_message_records(conn: &Connection, task_id: &str) -> Result<Vec<MessageRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, message_type, content, reasoning, tool_name, tool_arguments, tool_return, timestamp
         FROM messages WHERE task_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        Ok(MessageRecord {
            id: row.get(0)?,
            message_type: row.get(1)?,
            content: row.get(2)?,
            reasoning: row.get(3)?,
            tool_name: row.get(4)?,
            tool_arguments: row.get(5)?,
            tool_return: row.get(6)?,
            timestamp: row.get(7)?,
        })
    })?;
    rows.collect()
}

/// Format conversation history as a context string for agent injection
/// Includes full tool call details with arguments and returns
pub fn format_conversation_history(
    messages: &[MessageRecord],
    original_prompt: Option<&str>,
) -> String {
    let mut output = String::new();

    // Include original task prompt if available
    if let Some(prompt) = original_prompt {
        output.push_str("[Original Task]\n");
        output.push_str(prompt);
        output.push_str("\n\n");
    }

    output.push_str("[Previous Conversation]\n\n");

    for msg in messages {
        match msg.message_type.as_str() {
            "user_message" => {
                if let Some(ref content) = msg.content {
                    output.push_str("User: ");
                    output.push_str(content);
                    output.push_str("\n\n");
                }
            }
            "assistant_message" => {
                if let Some(ref content) = msg.content {
                    output.push_str("Assistant: ");
                    output.push_str(content);
                    output.push_str("\n\n");
                }
            }
            "reasoning_message" => {
                if let Some(ref reasoning) = msg.reasoning {
                    output.push_str("[Reasoning] ");
                    // Truncate long reasoning to save context (safe for UTF-8)
                    let prefix = safe_prefix(reasoning, 500);
                    output.push_str(prefix);
                    if reasoning.chars().count() > 500 {
                        output.push_str("...");
                    }
                    output.push_str("\n\n");
                }
            }
            "tool_call_message" => {
                if let Some(ref name) = msg.tool_name {
                    output.push_str(&format!("[Tool Call: {}]\n", name));
                    if let Some(ref args) = msg.tool_arguments {
                        // Truncate very long arguments (safe for UTF-8)
                        output.push_str("Arguments: ");
                        output.push_str(safe_prefix(args, 1000));
                        if args.chars().count() > 1000 {
                            output.push_str("...");
                        }
                        output.push('\n');
                    }
                    output.push('\n');
                }
            }
            "tool_return_message" => {
                if let Some(ref result) = msg.tool_return {
                    output.push_str("[Tool Result]\n");
                    // Truncate very long tool returns (safe for UTF-8)
                    output.push_str(safe_prefix(result, 2000));
                    if result.chars().count() > 2000 {
                        output.push_str("...");
                    }
                    output.push('\n');
                    output.push('\n');
                }
            }
            _ => {
                // Skip unknown message types
            }
        }
    }

    output.push_str("---\n\n");
    output
}

/// Compact history to fit within a token limit
/// Keeps recent messages intact and progressively summarizes older messages
/// Returns (compacted_history, was_truncated)
pub fn compact_history(
    messages: &[MessageRecord],
    original_prompt: Option<&str>,
    max_chars: usize,
) -> (String, bool) {
    // First, try full history
    let full_history = format_conversation_history(messages, original_prompt);
    if full_history.len() <= max_chars {
        return (full_history, false);
    }

    // Need to compact - keep most recent messages, summarize older ones
    let mut output = String::new();

    // Always include original prompt
    if let Some(prompt) = original_prompt {
        output.push_str("[Original Task]\n");
        output.push_str(prompt);
        output.push_str("\n\n");
    }

    // Calculate how many chars we have for messages
    let available_chars = max_chars.saturating_sub(output.len() + 100); // Reserve 100 for summary header

    // Split messages: older half gets summarized, newer half kept intact
    let split_point = messages.len() / 2;
    let (older, newer) = messages.split_at(split_point);

    // Summarize older messages
    if !older.is_empty() {
        let mut tool_calls = 0;
        let mut assistant_msgs = 0;
        let mut user_msgs = 0;

        for msg in older {
            match msg.message_type.as_str() {
                "tool_call_message" => tool_calls += 1,
                "assistant_message" => assistant_msgs += 1,
                "user_message" => user_msgs += 1,
                _ => {}
            }
        }

        output.push_str(&format!(
            "[Earlier conversation summary: {} user messages, {} assistant responses, {} tool calls]\n\n",
            user_msgs, assistant_msgs, tool_calls
        ));
    }

    // Format newer messages
    output.push_str("[Recent Conversation]\n\n");
    let newer_history = format_conversation_history(newer, None);

    // If still too long, truncate from the beginning of newer messages
    if output.len() + newer_history.len() > available_chars {
        let remaining = available_chars.saturating_sub(output.len());
        if remaining > 0 && newer_history.len() > remaining {
            // Take the last 'remaining' chars
            let start = newer_history.len() - remaining;
            output.push_str("...[truncated]...\n\n");
            output.push_str(&newer_history[start..]);
        } else if remaining > 0 {
            output.push_str(&newer_history);
        }
    } else {
        output.push_str(&newer_history);
    }

    output.push_str("---\n\n");
    (output, true)
}

/// Insert a new meeting session
pub fn insert_meeting_session(conn: &Connection, session: &MeetingSessionRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO meeting_sessions (id, title, status, capture_mic, capture_system, started_at, stopped_at, duration_ms, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            session.id,
            session.title,
            session.status,
            session.capture_mic as i64,
            session.capture_system as i64,
            session.started_at,
            session.stopped_at,
            session.duration_ms,
            session.created_at,
            session.updated_at,
        ],
    )?;
    Ok(())
}

/// Update meeting session status, stopped_at, duration_ms, and updated_at
pub fn update_meeting_session_status(
    conn: &Connection,
    id: &str,
    status: &str,
    stopped_at: Option<i64>,
    duration_ms: Option<i64>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE meeting_sessions SET status = ?1, stopped_at = COALESCE(?2, stopped_at), duration_ms = COALESCE(?3, duration_ms), updated_at = ?4 WHERE id = ?5",
        params![status, stopped_at, duration_ms, now, id],
    )?;
    Ok(())
}

/// Update meeting session title and updated_at.
pub fn update_meeting_session_title(
    conn: &Connection,
    id: &str,
    title: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE meeting_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now, id],
    )?;
    Ok(())
}

/// List all meeting sessions ordered by created_at DESC
pub fn list_meeting_sessions(conn: &Connection) -> Result<Vec<MeetingSessionRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, title, status, capture_mic, capture_system, started_at, stopped_at, duration_ms, created_at, updated_at
         FROM meeting_sessions ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(MeetingSessionRecord {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            capture_mic: row.get::<_, i64>(3)? != 0,
            capture_system: row.get::<_, i64>(4)? != 0,
            started_at: row.get(5)?,
            stopped_at: row.get(6)?,
            duration_ms: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    })?;
    rows.collect()
}

/// Fetch a single meeting session record by id.
pub fn get_meeting_session(conn: &Connection, id: &str) -> Result<Option<MeetingSessionRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, title, status, capture_mic, capture_system, started_at, stopped_at, duration_ms, created_at, updated_at
         FROM meeting_sessions WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(MeetingSessionRecord {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            capture_mic: row.get::<_, i64>(3)? != 0,
            capture_system: row.get::<_, i64>(4)? != 0,
            started_at: row.get(5)?,
            stopped_at: row.get(6)?,
            duration_ms: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        }))
    } else {
        Ok(None)
    }
}

/// Get all segments for a meeting session ordered by start_ms ASC
pub fn get_meeting_segments(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<MeetingSegmentRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, session_id, text, start_ms, end_ms, speaker, created_at
         FROM meeting_segments WHERE session_id = ?1 ORDER BY start_ms ASC",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(MeetingSegmentRecord {
            id: row.get(0)?,
            session_id: row.get(1)?,
            text: row.get(2)?,
            start_ms: row.get(3)?,
            end_ms: row.get(4)?,
            speaker: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

/// Delete all segments for a meeting session.
pub fn delete_meeting_segments_for_session(conn: &Connection, session_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM meeting_segments WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(())
}

/// Save a transcription segment and return the inserted row ID
pub fn save_meeting_segment(
    conn: &Connection,
    session_id: &str,
    text: &str,
    start_ms: i64,
    end_ms: i64,
    speaker: Option<&str>,
) -> Result<i64> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO meeting_segments (session_id, text, start_ms, end_ms, speaker, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![session_id, text, start_ms, end_ms, speaker, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Delete a meeting session (segments auto-deleted via CASCADE)
pub fn delete_meeting_session(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM meeting_sessions WHERE id = ?1", params![id])?;
    Ok(())
}

/// Update a session's updated_at field to "now".
pub fn touch_meeting_session_updated_at(conn: &Connection, id: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE meeting_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

/// Optimize database and checkpoint WAL on shutdown
pub fn optimize_and_shutdown(conn: &Connection) -> Result<()> {
    // Let SQLite analyze and optimize based on usage patterns
    conn.execute("PRAGMA optimize", [])?;
    // Merge WAL back into main database file
    conn.execute("PRAGMA wal_checkpoint(PASSIVE)", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_init_db_migrates_automations_schema() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!(
            "phantom-harness-automations-migration-{suffix}.sqlite"
        ));

        // Create an older automations schema (missing next_run_at and other newer fields).
        {
            let conn = Connection::open(&path).expect("open temp db");
            conn.execute(
                "CREATE TABLE automations (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    agent_id TEXT NOT NULL,
                    exec_model TEXT NOT NULL,
                    prompt TEXT NOT NULL,
                    project_path TEXT,
                    cron TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                )",
                [],
            )
            .expect("create old automations table");

            let now = chrono::Utc::now().timestamp();
            conn.execute(
                "INSERT INTO automations
                 (id, name, enabled, agent_id, exec_model, prompt, project_path, cron, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    "auto-1",
                    "Test automation",
                    1,
                    "codex",
                    "default",
                    "hello",
                    Option::<String>::None,
                    "0 9 * * *",
                    now,
                    now
                ],
            )
            .expect("insert old automation row");
        }

        // init_db should migrate the schema so list_automations can select all expected columns.
        let conn = init_db(&path).expect("init_db should migrate");
        let automations = list_automations(&conn).expect("list_automations should work");
        assert_eq!(automations.len(), 1);
        assert_eq!(automations[0].id, "auto-1");
        assert_eq!(automations[0].cron, "0 9 * * *");
        assert_eq!(automations[0].permission_mode, "default");
        assert!(
            automations[0].next_run_at.is_some(),
            "init_db should backfill next_run_at for enabled automations"
        );

        // Best-effort cleanup.
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn test_init_db_migrates_automation_runs_schema() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!(
            "phantom-harness-automation-runs-migration-{suffix}.sqlite"
        ));

        // Create an older automation_runs schema (missing created_at).
        {
            let conn = Connection::open(&path).expect("open temp db");
            conn.execute(
                "CREATE TABLE automation_runs (
                    id TEXT PRIMARY KEY,
                    automation_id TEXT NOT NULL,
                    task_id TEXT,
                    scheduled_for INTEGER NOT NULL,
                    error TEXT
                )",
                [],
            )
            .expect("create old automation_runs table");

            let now = chrono::Utc::now().timestamp();
            conn.execute(
                "INSERT INTO automation_runs
                 (id, automation_id, task_id, scheduled_for, error)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    "run-1",
                    "auto-1",
                    Option::<String>::None,
                    now,
                    Option::<String>::None
                ],
            )
            .expect("insert old automation_runs row");
        }

        // init_db should migrate the schema so list_automation_runs can select created_at.
        let conn = init_db(&path).expect("init_db should migrate");
        let runs = list_automation_runs(&conn, 10).expect("list_automation_runs should work");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "run-1");
        assert_eq!(runs[0].automation_id, "auto-1");

        // Best-effort cleanup.
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn test_set_automation_last_error_does_not_clobber_next_run_at() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!(
            "phantom-harness-automation-last-error-{suffix}.sqlite"
        ));

        let conn = init_db(&path).expect("init_db should create schema");

        let now = chrono::Utc::now().timestamp();
        let automation = AutomationRecord {
            id: "auto-err-1".to_string(),
            name: "Test".to_string(),
            enabled: true,
            agent_id: "codex".to_string(),
            exec_model: "default".to_string(),
            prompt: "hello".to_string(),
            project_path: None,
            base_branch: None,
            plan_mode: false,
            thinking: true,
            use_worktree: true,
            permission_mode: "default".to_string(),
            reasoning_effort: None,
            agent_mode: None,
            codex_mode: None,
            claude_runtime: None,
            cron: "0 9 * * *".to_string(),
            next_run_at: Some(now + 3600),
            last_run_at: Some(now),
            last_error: None,
            created_at: now,
            updated_at: now,
        };
        insert_automation(&conn, &automation).expect("insert automation");

        set_automation_last_error(&conn, "auto-err-1", Some("boom".to_string()), now + 1)
            .expect("set_automation_last_error should work");

        let updated = get_automation(&conn, "auto-err-1")
            .expect("get automation")
            .expect("automation exists");
        assert_eq!(updated.last_error.as_deref(), Some("boom"));
        assert_eq!(updated.next_run_at, automation.next_run_at);

        // Best-effort cleanup.
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn test_text_note_segments_roundtrip() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("phantom-harness-text-note-{suffix}.sqlite"));

        let conn = init_db(&path).expect("init db");
        let now = chrono::Utc::now().timestamp();

        let session = MeetingSessionRecord {
            id: "note-test-1".to_string(),
            title: Some("Scratch".to_string()),
            status: "text".to_string(),
            capture_mic: false,
            capture_system: false,
            started_at: None,
            stopped_at: None,
            duration_ms: 0,
            created_at: now,
            updated_at: now,
        };
        insert_meeting_session(&conn, &session).expect("insert meeting session");

        // Write text note content as a single segment.
        delete_meeting_segments_for_session(&conn, &session.id).expect("clear segments");
        save_meeting_segment(&conn, &session.id, "hello\nworld", 0, 0, None).expect("save segment");
        touch_meeting_session_updated_at(&conn, &session.id).expect("touch updated_at");

        let fetched = get_meeting_session(&conn, &session.id)
            .expect("get session")
            .expect("session exists");
        assert_eq!(fetched.status, "text");

        let segs = get_meeting_segments(&conn, &session.id).expect("get segments");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "hello\nworld");

        // Best-effort cleanup.
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }
}
