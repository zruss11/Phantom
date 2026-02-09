use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::path::Path;
use std::thread;
use std::time::Duration;

use super::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "leadAgentId")]
    pub lead_agent_id: String,
    #[serde(rename = "leadSessionId")]
    pub lead_session_id: String,
    pub members: Vec<TeamMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    #[serde(rename = "agentId")]
    pub agent_id: String, // name@team
    pub name: String,
    #[serde(rename = "agentType")]
    pub agent_type: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(rename = "joinedAt")]
    pub joined_at: i64,
    #[serde(default)]
    #[serde(rename = "tmuxPaneId")]
    pub tmux_pane_id: Option<String>,
    pub cwd: String,
    #[serde(default)]
    pub subscriptions: Option<Vec<String>>,
}

fn write_json_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename tmp: {e}"))?;
    Ok(())
}

const LOCK_RETRIES: usize = 5;

fn lock_with_backoff(file: &std::fs::File) -> std::io::Result<()> {
    let mut delay_ms = 50u64;
    for i in 0..LOCK_RETRIES {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(e) => {
                if i == LOCK_RETRIES - 1 {
                    return Err(e);
                }
                thread::sleep(Duration::from_millis(delay_ms));
                delay_ms = (delay_ms * 2).min(500);
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "lock retry loop exhausted",
    ))
}

fn lock_team_config(team_name: &str) -> Result<std::fs::File, String> {
    let lock_path = paths::team_config_lock_path(team_name)
        .ok_or_else(|| "home dir not found (or invalid name)".to_string())?;
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir lock dir: {e}"))?;
    }
    if !lock_path.exists() {
        fs::write(&lock_path, b"").map_err(|e| format!("init lock file: {e}"))?;
    }
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("open lock: {e}"))?;
    lock_with_backoff(&f).map_err(|e| format!("lock config: {e}"))?;
    Ok(f)
}

pub fn ensure_team(team_name: &str, cwd: &str, lead_session_id: &str) -> Result<(), String> {
    let team_dir = paths::team_dir(team_name).ok_or_else(|| "home dir not found".to_string())?;
    let inbox_dir =
        paths::inboxes_dir(team_name).ok_or_else(|| "home dir not found".to_string())?;
    fs::create_dir_all(&team_dir).map_err(|e| format!("mkdir team: {e}"))?;
    fs::create_dir_all(&inbox_dir).map_err(|e| format!("mkdir inbox: {e}"))?;

    // Ensure controller inbox exists.
    let ctrl_inbox = paths::inbox_path(team_name, "controller")
        .ok_or_else(|| "home dir not found".to_string())?;
    if !ctrl_inbox.exists() {
        fs::write(&ctrl_inbox, b"[]").map_err(|e| format!("init controller inbox: {e}"))?;
    }

    let config_path =
        paths::team_config_path(team_name).ok_or_else(|| "home dir not found".to_string())?;
    if config_path.exists() {
        return Ok(());
    }

    let lead_name = "controller";
    let lead_agent_id = format!("{lead_name}@{team_name}");
    let now = chrono::Utc::now().timestamp_millis();
    let config = TeamConfig {
        name: team_name.to_string(),
        description: None,
        created_at: now,
        lead_agent_id: lead_agent_id.clone(),
        lead_session_id: lead_session_id.to_string(),
        members: vec![TeamMember {
            agent_id: lead_agent_id,
            name: lead_name.to_string(),
            agent_type: "controller".to_string(),
            model: None,
            joined_at: now,
            tmux_pane_id: Some(String::new()),
            cwd: cwd.to_string(),
            subscriptions: Some(Vec::new()),
        }],
    };

    let bytes = serde_json::to_vec_pretty(&config).map_err(|e| format!("serialize: {e}"))?;
    write_json_atomic(&config_path, &bytes)?;
    Ok(())
}

pub fn read_config(team_name: &str) -> Result<TeamConfig, String> {
    let lock = lock_team_config(team_name)?;
    let path =
        paths::team_config_path(team_name).ok_or_else(|| "home dir not found".to_string())?;
    let raw = fs::read_to_string(&path).map_err(|e| format!("read config: {e}"))?;
    let out = serde_json::from_str(&raw).map_err(|e| format!("parse config: {e}"));
    lock.unlock().ok();
    out
}

pub fn write_config(team_name: &str, config: &TeamConfig) -> Result<(), String> {
    let lock = lock_team_config(team_name)?;
    let path =
        paths::team_config_path(team_name).ok_or_else(|| "home dir not found".to_string())?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| format!("serialize: {e}"))?;
    let out = write_json_atomic(&path, &bytes);
    lock.unlock().ok();
    out
}

pub fn add_member(team_name: &str, member: TeamMember) -> Result<(), String> {
    let mut config = read_config(team_name)?;
    config.members.retain(|m| m.name != member.name);
    config.members.push(member);
    write_config(team_name, &config)
}

pub fn remove_member(team_name: &str, name: &str) -> Result<(), String> {
    let mut config = read_config(team_name)?;
    config.members.retain(|m| m.name != name);
    write_config(team_name, &config)
}
