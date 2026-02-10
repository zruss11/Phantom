use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use fs2::FileExt;

use super::paths;
use super::types::{InboxMessage, ParsedMessage, PollEvent, StructuredMessage};

const LOCK_RETRIES: usize = 20;

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
                delay_ms = (delay_ms * 2).min(1000);
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

fn lock_path_for_inbox(inbox_path: &Path) -> PathBuf {
    let mut lock = inbox_path.to_path_buf();
    lock.set_extension("json.lock");
    lock
}

fn lock_exclusive(lock_path: &Path) -> Result<std::fs::File, String> {
    ensure_file(lock_path).map_err(|e| format!("ensure lock file: {e}"))?;
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|e| format!("open lock file: {e}"))?;
    lock_with_backoff(&f).map_err(|e| format!("lock: {e}"))?;
    Ok(f)
}

pub fn ensure_inbox_file(team_name: &str, agent_name: &str) -> Result<(), String> {
    let path = paths::inbox_path(team_name, agent_name)
        .ok_or_else(|| "home dir not found (or invalid name)".to_string())?;
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

fn parse_inbox_or_recover(path: &Path, raw: &str) -> Vec<InboxMessage> {
    match serde_json::from_str::<Vec<InboxMessage>>(raw) {
        Ok(v) => v,
        Err(err) => {
            // Don't silently drop messages. Preserve the corrupt content for inspection, then
            // reset so the system can make progress.
            let ts = chrono::Utc::now().timestamp_millis();
            let mut backup = PathBuf::from(path);
            backup.set_extension(format!("json.corrupt-{ts}"));
            let _ = std::fs::write(&backup, raw.as_bytes());
            eprintln!(
                "[Harness] inbox JSON parse failed for {:?} (backed up to {:?}): {err}",
                path, backup
            );
            Vec::new()
        }
    }
}

fn read_to_string_bounded(path: &Path) -> Result<String, String> {
    let f = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|e| format!("open inbox: {e}"))?;
    let mut raw = String::new();
    (&f).take(5_000_000)
        .read_to_string(&mut raw)
        .map_err(|e| format!("read inbox: {e}"))?;
    Ok(raw)
}

fn write_inbox_at_paths(
    inbox_path: &Path,
    lock_path: &Path,
    message: InboxMessage,
) -> Result<(), String> {
    ensure_file(inbox_path).map_err(|e| format!("ensure inbox file: {e}"))?;

    // Serialize all writers/readers via a dedicated lock file so atomic rename does not break
    // lock semantics (locks are tied to inodes).
    let lock = lock_exclusive(lock_path)?;

    let raw = read_to_string_bounded(inbox_path)?;
    let mut messages = parse_inbox_or_recover(inbox_path, &raw);
    messages.push(message);
    let bytes = serde_json::to_vec_pretty(&messages).map_err(|e| format!("serialize: {e}"))?;
    atomic_write_json(inbox_path, &bytes).map_err(|e| format!("write inbox: {e}"))?;

    lock.unlock().ok();
    Ok(())
}

pub fn write_inbox(team_name: &str, agent_name: &str, message: InboxMessage) -> Result<(), String> {
    let path = paths::inbox_path(team_name, agent_name)
        .ok_or_else(|| "home dir not found (or invalid name)".to_string())?;
    let lock_path =
        paths::inbox_lock_path(team_name, agent_name).unwrap_or_else(|| lock_path_for_inbox(&path));
    write_inbox_at_paths(&path, &lock_path, message)
}

fn read_unread_and_mark_read_at_paths(
    inbox_path: &Path,
    lock_path: &Path,
) -> Result<Vec<PollEvent>, String> {
    if !inbox_path.exists() {
        return Ok(Vec::new());
    }

    let lock = lock_exclusive(lock_path)?;

    let raw = read_to_string_bounded(inbox_path)?;
    let mut messages = parse_inbox_or_recover(inbox_path, &raw);

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
        atomic_write_json(inbox_path, &bytes).map_err(|e| format!("write inbox: {e}"))?;
    }

    lock.unlock().ok();
    Ok(unread)
}

pub fn read_unread_and_mark_read(
    team_name: &str,
    agent_name: &str,
) -> Result<Vec<PollEvent>, String> {
    let path = paths::inbox_path(team_name, agent_name)
        .ok_or_else(|| "home dir not found (or invalid name)".to_string())?;
    let lock_path =
        paths::inbox_lock_path(team_name, agent_name).unwrap_or_else(|| lock_path_for_inbox(&path));
    read_unread_and_mark_read_at_paths(&path, &lock_path)
}

#[cfg(test)]
mod tests {
    use super::{read_unread_and_mark_read_at_paths, write_inbox_at_paths};
    use crate::claude_controller::types::InboxMessage;
    use std::fs;
    use std::path::PathBuf;

    fn tmp_dir(name: &str) -> PathBuf {
        let base = std::env::temp_dir();
        let dir = base.join(format!(
            "phantom-harness-inbox-test-{}-{}",
            name,
            uuid::Uuid::new_v4().to_string()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_concurrent_writes_do_not_lose_messages() {
        let dir = tmp_dir("concurrent");
        let inbox = dir.join("agent.json");
        let lock = dir.join("agent.json.lock");

        // Seed empty inbox.
        fs::write(&inbox, "[]").unwrap();

        let mut handles = Vec::new();
        for i in 0..25 {
            let inbox = inbox.clone();
            let lock = lock.clone();
            handles.push(std::thread::spawn(move || {
                let msg = InboxMessage {
                    from: "controller".to_string(),
                    text: format!("hello-{i}"),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    color: None,
                    read: false,
                    summary: None,
                };
                write_inbox_at_paths(&inbox, &lock, msg).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let raw = fs::read_to_string(&inbox).unwrap();
        let msgs: Vec<InboxMessage> = serde_json::from_str(&raw).unwrap();
        assert_eq!(msgs.len(), 25);
    }

    #[test]
    fn test_mark_read_is_atomic_under_lock() {
        let dir = tmp_dir("mark-read");
        let inbox = dir.join("agent.json");
        let lock = dir.join("agent.json.lock");

        let mut msgs: Vec<InboxMessage> = Vec::new();
        for i in 0..3 {
            msgs.push(InboxMessage {
                from: "agent".to_string(),
                text: format!("m{i}"),
                timestamp: chrono::Utc::now().to_rfc3339(),
                color: None,
                read: false,
                summary: None,
            });
        }
        fs::write(&inbox, serde_json::to_string_pretty(&msgs).unwrap()).unwrap();

        let unread1 = read_unread_and_mark_read_at_paths(&inbox, &lock).unwrap();
        assert_eq!(unread1.len(), 3);
        let unread2 = read_unread_and_mark_read_at_paths(&inbox, &lock).unwrap();
        assert_eq!(unread2.len(), 0);
    }
}
