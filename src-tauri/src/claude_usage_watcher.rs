//! Real-time cost tracking for Claude Code ACP sessions.
//!
//! Watches Claude's JSONL session files (~/.claude/projects/**/{session-id}.jsonl)
//! and emits CostUpdate events as usage data is written. This enables live cost
//! tracking in the UI during active sessions.
//!
//! Claude Code ACP doesn't emit `_tokenUsage` notifications like Codex does,
//! so we watch the JSONL files that Claude writes in real-time instead.

use crate::claude_local_usage::calculate_cost;
use glob::glob;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

/// Watcher state for a Claude Code session
pub struct ClaudeUsageWatcher {
    session_id: String,
    #[allow(dead_code)] // Kept for future logging enhancements
    task_id: String,
    jsonl_path: Option<PathBuf>,
    last_position: u64,
    cumulative_cost: f64,
    processed_hashes: HashSet<String>,
}

impl ClaudeUsageWatcher {
    /// Create a new watcher for a Claude Code session
    pub fn new(session_id: String, task_id: String) -> Self {
        Self {
            session_id,
            task_id,
            jsonl_path: None,
            last_position: 0,
            cumulative_cost: 0.0,
            processed_hashes: HashSet::new(),
        }
    }

    /// Find the JSONL file for this session
    fn find_jsonl_file(&mut self) -> Option<PathBuf> {
        // Check if we already have a valid path
        if let Some(ref path) = self.jsonl_path {
            if path.exists() {
                return Some(path.clone());
            }
        }

        // Search for the JSONL file using glob pattern
        // Claude stores session files in ~/.claude/projects/**/{session_id}.jsonl
        let claude_dirs = resolve_claude_paths();

        for claude_dir in claude_dirs {
            let pattern = format!(
                "{}/projects/**/{}.jsonl",
                claude_dir.to_string_lossy(),
                self.session_id
            );

            if let Ok(paths) = glob(&pattern) {
                for entry in paths.flatten() {
                    self.jsonl_path = Some(entry.clone());
                    return Some(entry);
                }
            }
        }

        None
    }

    /// Poll for new usage entries and return cumulative cost if changed
    fn poll_usage(&mut self) -> Option<f64> {
        let path = self.find_jsonl_file()?;

        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => return None,
        };

        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(_) => return None,
        };

        let file_size = metadata.len();

        // Skip if file hasn't grown
        if file_size <= self.last_position {
            return None;
        }

        let mut reader = BufReader::new(file);

        // Seek to last known position
        if self.last_position > 0 {
            if reader.seek(SeekFrom::Start(self.last_position)).is_err() {
                return None;
            }
        }

        let mut cost_changed = false;
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if let Some(cost) = self.process_line(&line) {
                        self.cumulative_cost += cost;
                        cost_changed = true;
                    }
                }
                Err(_) => break,
            }
        }

        // Update position for next poll
        self.last_position = reader.stream_position().unwrap_or(file_size);

        if cost_changed {
            Some(self.cumulative_cost)
        } else {
            None
        }
    }

    /// Process a single JSONL line and return cost if it contains usage data
    fn process_line(&mut self, line: &str) -> Option<f64> {
        // Skip very large lines to prevent memory issues
        if line.len() > 512_000 {
            return None;
        }

        let value: Value = serde_json::from_str(line).ok()?;

        // Only process assistant messages with usage data
        let msg_type = value.get("type").and_then(|v| v.as_str())?;
        if msg_type != "assistant" {
            return None;
        }

        let message = value.get("message")?.as_object()?;
        let usage = message.get("usage")?.as_object()?;

        // Create unique hash for deduplication using message.id + requestId
        let message_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let request_id = value
            .get("requestId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !message_id.is_empty() && !request_id.is_empty() {
            let hash = format!("{}:{}", message_id, request_id);
            if self.processed_hashes.contains(&hash) {
                return None;
            }
            self.processed_hashes.insert(hash);
        }

        // Extract token counts
        let input_tokens = usage
            .get("input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let output_tokens = usage
            .get("output_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_creation_tokens = usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_read_tokens = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        // Skip if no tokens
        if input_tokens == 0 && output_tokens == 0 {
            return None;
        }

        // Extract model
        let model = message
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("claude-sonnet");

        // Calculate cost using shared pricing logic
        let cost = calculate_cost(
            model,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        );

        Some(cost)
    }
}

/// Resolve Claude config directories (same logic as claude_local_usage.rs)
fn resolve_claude_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Check environment variable first
    if let Ok(env_paths) = std::env::var("CLAUDE_CONFIG_DIR") {
        for env_path in env_paths
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
        {
            let path = PathBuf::from(env_path);
            if path.join("projects").exists() {
                paths.push(path);
            }
        }
        if !paths.is_empty() {
            return paths;
        }
    }

    // Check default paths
    if let Some(home) = dirs::home_dir() {
        // Check ~/.claude (primary on macOS)
        let claude_path = home.join(".claude");
        if claude_path.join("projects").exists() {
            paths.push(claude_path);
        }

        // Check ~/.config/claude (XDG standard)
        let config_claude_path = home.join(".config").join("claude");
        if config_claude_path.join("projects").exists() {
            paths.push(config_claude_path);
        }
    }

    paths
}

/// Handle type for stopping the watcher
pub struct WatcherHandle {
    #[allow(dead_code)] // Used for graceful shutdown in future
    stop_flag: Arc<Mutex<bool>>,
}

impl WatcherHandle {
    /// Signal the watcher to stop
    #[allow(dead_code)] // Reserved for session cleanup
    pub async fn stop(&self) {
        let mut flag = self.stop_flag.lock().await;
        *flag = true;
    }
}

/// Start watching a Claude Code session for usage updates
///
/// Returns a handle that can be used to stop the watcher.
/// The watcher will emit CostUpdate events to the frontend as it detects
/// new usage entries in the JSONL file.
pub fn start_watching(
    session_id: &str,
    task_id: &str,
    app: AppHandle,
    db: Arc<std::sync::Mutex<rusqlite::Connection>>,
) -> WatcherHandle {
    let session_id = session_id.to_string();
    let task_id = task_id.to_string();
    let stop_flag = Arc::new(Mutex::new(false));
    let stop_flag_clone = stop_flag.clone();

    tauri::async_runtime::spawn(async move {
        let mut watcher = ClaudeUsageWatcher::new(session_id.clone(), task_id.clone());
        let mut poll_interval = interval(Duration::from_millis(500));

        // Initial delay to let Claude start writing the JSONL file
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!(
            "[ClaudeWatcher] Started watching session {} for task {}",
            session_id, task_id
        );

        loop {
            poll_interval.tick().await;

            // Check if we should stop
            {
                let flag = stop_flag_clone.lock().await;
                if *flag {
                    println!(
                        "[ClaudeWatcher] Stopping watcher for session {}",
                        session_id
                    );
                    break;
                }
            }

            // Poll for new usage data
            if let Some(new_cost) = watcher.poll_usage() {
                // Update database
                if let Ok(conn) = db.lock() {
                    let current_cost = crate::db::get_task_cost(&conn, &task_id).unwrap_or(0.0);
                    // Only update if our cumulative is higher (avoid double-counting)
                    if new_cost > current_cost {
                        let _ = crate::db::update_task_cost(&conn, &task_id, new_cost);
                    }
                }

                // Emit event to frontend
                if let Err(e) = app.emit("CostUpdate", (&task_id, new_cost)) {
                    eprintln!("[ClaudeWatcher] Failed to emit CostUpdate: {}", e);
                }

                println!(
                    "[ClaudeWatcher] Cost update for {}: ${:.4}",
                    task_id, new_cost
                );
            }
        }

        println!("[ClaudeWatcher] Watcher stopped for session {}", session_id);
    });

    WatcherHandle { stop_flag }
}
