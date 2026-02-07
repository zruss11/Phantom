use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use fs2::FileExt;

use super::paths;
use super::types::{InboxMessage, ParsedMessage, PollEvent, StructuredMessage};

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

fn ensure_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn ensure_file(path: &Path) -> std::io::Result<()> {
    ensure_dir(path)?;
    if !path.exists() {
        std::fs::write(path, b"[]")?;
    }
    Ok(())
}

pub fn ensure_inbox_file(team_name: &str, agent_name: &str) -> Result<(), String> {
    let path = paths::inbox_path(team_name, agent_name)
        .ok_or_else(|| "home dir not found".to_string())?;
    ensure_file(&path).map_err(|e| format!("ensure inbox file: {e}"))
}

fn parse_structured(text: &str) -> ParsedMessage {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        if value.get("type").and_then(|v| v.as_str()).is_some() {
            if let Ok(s) = serde_json::from_value::<StructuredMessage>(value) {
                return ParsedMessage::Structured(s);
            }
        }
    }
    ParsedMessage::PlainText(text.to_string())
}

fn atomic_write_json(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = PathBuf::from(path);
    tmp.set_extension("json.tmp");
    {
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub fn write_inbox(
    team_name: &str,
    agent_name: &str,
    message: InboxMessage,
) -> Result<(), String> {
    let path = paths::inbox_path(team_name, agent_name)
        .ok_or_else(|| "home dir not found".to_string())?;
    ensure_file(&path).map_err(|e| format!("ensure inbox file: {e}"))?;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open inbox: {e}"))?;
    lock_with_backoff(&file).map_err(|e| format!("lock inbox: {e}"))?;

    let mut raw = String::new();
    (&file)
        .take(5_000_000)
        .read_to_string(&mut raw)
        .map_err(|e| format!("read inbox: {e}"))?;
    let mut messages: Vec<InboxMessage> = serde_json::from_str(&raw).unwrap_or_default();
    messages.push(message);
    let bytes = serde_json::to_vec_pretty(&messages).map_err(|e| format!("serialize: {e}"))?;

    atomic_write_json(&path, &bytes).map_err(|e| format!("write inbox: {e}"))?;
    file.unlock().ok();
    Ok(())
}

pub fn read_unread_and_mark_read(
    team_name: &str,
    agent_name: &str,
) -> Result<Vec<PollEvent>, String> {
    let path = paths::inbox_path(team_name, agent_name)
        .ok_or_else(|| "home dir not found".to_string())?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open inbox: {e}"))?;
    lock_with_backoff(&file).map_err(|e| format!("lock inbox: {e}"))?;

    let mut raw = String::new();
    (&file)
        .take(5_000_000)
        .read_to_string(&mut raw)
        .map_err(|e| format!("read inbox: {e}"))?;
    let mut messages: Vec<InboxMessage> = serde_json::from_str(&raw).unwrap_or_default();

    let mut unread: Vec<PollEvent> = Vec::new();
    let mut changed = false;
    for msg in &mut messages {
        if !msg.read {
            unread.push(PollEvent {
                raw: msg.clone(),
                parsed: parse_structured(&msg.text),
            });
            msg.read = true;
            changed = true;
        }
    }

    if changed {
        let bytes = serde_json::to_vec_pretty(&messages).map_err(|e| format!("serialize: {e}"))?;
        atomic_write_json(&path, &bytes).map_err(|e| format!("write inbox: {e}"))?;
    }

    file.unlock().ok();
    Ok(unread)
}
