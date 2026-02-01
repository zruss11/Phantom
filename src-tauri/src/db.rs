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

pub fn init_db(path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    // PRAGMAs that return results - use query_row to consume them
    let _ = conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()));
    conn.execute("PRAGMA synchronous = NORMAL", [])?;
    conn.execute("PRAGMA temp_store = MEMORY", [])?;
    conn.execute("PRAGMA cache_size = -16000", [])?;
    conn.execute("PRAGMA mmap_size = 268435456", [])?; // 256MB memory-mapped I/O
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
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;

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

    Ok(conn)
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
pub fn save_cached_models(conn: &mut Connection, agent_id: &str, models: &[CachedModel]) -> Result<()> {
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
pub fn save_cached_modes(conn: &mut Connection, agent_id: &str, modes: &[CachedMode]) -> Result<()> {
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
        "INSERT INTO tasks (id, agent_id, codex_account_id, model, prompt, project_path, worktree_path, branch, status, status_state, cost, created_at, updated_at, title_summary, agent_session_id, total_tokens, context_window)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
    conn: &mut Connection,
    task_id: &str,
    message_id: i64,
    attachments: &[AttachmentRecord],
) -> Result<()> {
    if attachments.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().timestamp();
    let tx = conn.transaction()?;
    for attachment in attachments {
        tx.execute(
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
    tx.commit()?;
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
        "SELECT id, agent_id, codex_account_id, model, prompt, project_path, worktree_path, branch, status, status_state, cost, created_at, updated_at, title_summary, agent_session_id, total_tokens, context_window
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

/// Optimize database and checkpoint WAL on shutdown
pub fn optimize_and_shutdown(conn: &Connection) -> Result<()> {
    // Let SQLite analyze and optimize based on usage patterns
    conn.execute("PRAGMA optimize", [])?;
    // Merge WAL back into main database file
    conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", [])?;
    Ok(())
}
