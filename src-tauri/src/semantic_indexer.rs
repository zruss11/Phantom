//! Incremental semantic indexing (chunking + embeddings) into `semantic_chunks`.
//!
//! This is best-effort: if the embedding model isn't available yet, we skip
//! indexing rather than breaking user flows. Callers should schedule indexing
//! via `schedule_index_entity` (debounced).

use crate::{embedding_inference, embedding_model, semantic_search, AppState};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const DEFAULT_MODEL_ID: &str = embedding_model::DEFAULT_EMBEDDING_MODEL_ID;
const DEFAULT_MAX_SEQ_LEN: usize = 128;
const CHUNK_MAX_CHARS: usize = 2000;

fn fnv1a_64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in s.as_bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn content_hash(text: &str) -> String {
    format!("{:016x}", fnv1a_64(text))
}

fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let t = text.trim();
    if t.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();

    for word in t.split_whitespace() {
        if cur.is_empty() {
            cur.push_str(word);
            continue;
        }

        if cur.len() + 1 + word.len() > max_chars {
            out.push(cur);
            cur = word.to_string();
        } else {
            cur.push(' ');
            cur.push_str(word);
        }
    }

    if !cur.is_empty() {
        out.push(cur);
    }

    out
}

fn embedding_model_onnx_path(model_id: &str) -> PathBuf {
    embedding_model::model_dir(model_id)
        .join("onnx")
        .join("model.onnx")
}

fn model_available(model_id: &str) -> bool {
    embedding_model_onnx_path(model_id).exists()
}

#[derive(Debug, Clone)]
struct DesiredChunk {
    field: String,
    chunk_index: i64,
    text: String,
    content_hash: String,
}

fn desired_chunks_for_task(conn: &Connection, task_id: &str) -> Result<Vec<DesiredChunk>, String> {
    let title: Option<String> = conn
        .query_row(
            "SELECT COALESCE(NULLIF(title_summary, ''), NULLIF(prompt, '')) FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("DB error: {e}"))?
        .flatten();

    let prompt: Option<String> = conn
        .query_row(
            "SELECT prompt FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("DB error: {e}"))?;

    let mut transcript = String::new();
    if let Some(p) = prompt.as_ref().and_then(|s| {
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }) {
        transcript.push_str("Prompt: ");
        transcript.push_str(p);
        transcript.push('\n');
    }

    let mut stmt = conn
        .prepare_cached(
            "SELECT message_type, content
             FROM messages
             WHERE task_id = ?1 AND content IS NOT NULL AND content <> ''
             ORDER BY id ASC",
        )
        .map_err(|e| format!("DB error: {e}"))?;
    let mut rows = stmt
        .query(params![task_id])
        .map_err(|e| format!("DB error: {e}"))?;
    while let Some(row) = rows.next().map_err(|e| format!("DB error: {e}"))? {
        let message_type: String = row.get(0).map_err(|e| format!("DB error: {e}"))?;
        let content: String = row.get(1).map_err(|e| format!("DB error: {e}"))?;
        let label = match message_type.as_str() {
            "user_message" => "User",
            "assistant_message" => "Assistant",
            _ => "Message",
        };
        transcript.push_str(label);
        transcript.push_str(": ");
        transcript.push_str(content.trim());
        transcript.push('\n');
    }

    let mut out: Vec<DesiredChunk> = Vec::new();

    if let Some(t) = title.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        out.push(DesiredChunk {
            field: "title".to_string(),
            chunk_index: 0,
            text: t.to_string(),
            content_hash: content_hash(t),
        });
    }

    for (i, c) in chunk_text(&transcript, CHUNK_MAX_CHARS)
        .into_iter()
        .enumerate()
    {
        let h = content_hash(&c);
        out.push(DesiredChunk {
            field: "body".to_string(),
            chunk_index: i as i64,
            text: c,
            content_hash: h,
        });
    }

    Ok(out)
}

fn desired_chunks_for_note(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<DesiredChunk>, String> {
    let title: Option<String> = conn
        .query_row(
            "SELECT title FROM meeting_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("DB error: {e}"))?;

    let mut body = String::new();
    let mut stmt = conn
        .prepare_cached(
            "SELECT text
             FROM meeting_segments
             WHERE session_id = ?1 AND text <> ''
             ORDER BY start_ms ASC, id ASC",
        )
        .map_err(|e| format!("DB error: {e}"))?;
    let mut rows = stmt
        .query(params![session_id])
        .map_err(|e| format!("DB error: {e}"))?;
    while let Some(row) = rows.next().map_err(|e| format!("DB error: {e}"))? {
        let text: String = row.get(0).map_err(|e| format!("DB error: {e}"))?;
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        body.push_str(t);
        body.push('\n');
    }

    let mut out: Vec<DesiredChunk> = Vec::new();

    if let Some(t) = title.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        out.push(DesiredChunk {
            field: "title".to_string(),
            chunk_index: 0,
            text: t.to_string(),
            content_hash: content_hash(t),
        });
    }

    for (i, c) in chunk_text(&body, CHUNK_MAX_CHARS).into_iter().enumerate() {
        let h = content_hash(&c);
        out.push(DesiredChunk {
            field: "body".to_string(),
            chunk_index: i as i64,
            text: c,
            content_hash: h,
        });
    }

    Ok(out)
}

fn upsert_chunks(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    desired: Vec<DesiredChunk>,
) -> Result<(), String> {
    let model_name = DEFAULT_MODEL_ID.to_string();
    let dims = embedding_model::default_model_spec().dims as i64;
    let now = Utc::now().timestamp();

    // Build existing map for content_hash checks.
    let mut stmt = conn
        .prepare_cached(
            "SELECT field, chunk_index, content_hash
             FROM semantic_chunks
             WHERE entity_type = ?1 AND entity_id = ?2 AND model_name = ?3",
        )
        .map_err(|e| format!("DB error: {e}"))?;
    let mut rows = stmt
        .query(params![entity_type, entity_id, model_name])
        .map_err(|e| format!("DB error: {e}"))?;

    let mut existing: HashMap<(String, i64), String> = HashMap::new();
    while let Some(row) = rows.next().map_err(|e| format!("DB error: {e}"))? {
        let field: String = row.get(0).map_err(|e| format!("DB error: {e}"))?;
        let chunk_index: i64 = row.get(1).map_err(|e| format!("DB error: {e}"))?;
        let h: String = row.get(2).map_err(|e| format!("DB error: {e}"))?;
        existing.insert((field, chunk_index), h);
    }

    let mut desired_keys: HashSet<(String, i64)> = HashSet::new();

    for chunk in desired {
        let key = (chunk.field.clone(), chunk.chunk_index);
        desired_keys.insert(key.clone());

        if existing
            .get(&key)
            .map(|h| h == &chunk.content_hash)
            .unwrap_or(false)
        {
            continue;
        }

        // Generate embedding (best-effort). If embedding fails, skip persisting this chunk.
        let embedding = match embedding_inference::embed_text_sync(
            DEFAULT_MODEL_ID,
            DEFAULT_MAX_SEQ_LEN,
            &chunk.text,
        ) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    entity_type = %entity_type,
                    entity_id = %entity_id,
                    field = %chunk.field,
                    chunk_index = chunk.chunk_index,
                    error = %e,
                    "Embedding generation failed; skipping chunk"
                );
                continue;
            }
        };
        let blob = semantic_search::pack_f32_embedding(&embedding);

        conn.execute(
            "DELETE FROM semantic_chunks
             WHERE entity_type = ?1 AND entity_id = ?2 AND model_name = ?3 AND field = ?4 AND chunk_index = ?5",
            params![entity_type, entity_id, DEFAULT_MODEL_ID, chunk.field, chunk.chunk_index],
        )
        .map_err(|e| format!("DB error: {e}"))?;

        conn.execute(
            "INSERT INTO semantic_chunks(
                entity_type, entity_id, field, chunk_index, text,
                content_hash, model_name, dims, embedding, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entity_type,
                entity_id,
                chunk.field,
                chunk.chunk_index,
                chunk.text,
                chunk.content_hash,
                DEFAULT_MODEL_ID,
                dims,
                blob,
                now,
                now
            ],
        )
        .map_err(|e| format!("DB error: {e}"))?;
    }

    // Delete stale chunks (when content shrank).
    for (field, chunk_index) in existing.keys() {
        let key = (field.clone(), *chunk_index);
        if desired_keys.contains(&key) {
            continue;
        }
        conn.execute(
            "DELETE FROM semantic_chunks
             WHERE entity_type = ?1 AND entity_id = ?2 AND model_name = ?3 AND field = ?4 AND chunk_index = ?5",
            params![entity_type, entity_id, DEFAULT_MODEL_ID, field, chunk_index],
        )
        .ok();
    }

    Ok(())
}

fn index_entity_sync(conn: &Connection, entity_type: &str, entity_id: &str) -> Result<(), String> {
    let desired = if entity_type == crate::semantic_search::ENTITY_TYPE_TASK {
        desired_chunks_for_task(conn, entity_id)?
    } else if entity_type == crate::semantic_search::ENTITY_TYPE_NOTE {
        desired_chunks_for_note(conn, entity_id)?
    } else {
        return Ok(());
    };

    upsert_chunks(conn, entity_type, entity_id, desired)?;
    Ok(())
}

async fn index_entity(app: &AppHandle, entity_type: String, entity_id: String) {
    if !model_available(DEFAULT_MODEL_ID) {
        return;
    }

    let state = app.state::<AppState>();
    let db = state.db.clone();

    let _ = tokio::task::spawn_blocking(move || {
        let conn = db.lock().map_err(|e| format!("DB lock error: {e}"))?;
        index_entity_sync(&conn, &entity_type, &entity_id)
    })
    .await;
}

pub struct SemanticIndexJob {
    pub id: u64,
    pub task: tokio::task::JoinHandle<()>,
}

pub async fn schedule_index_entity(app: &AppHandle, entity_type: &str, entity_id: &str) {
    schedule_index_entity_with_delay(app, entity_type, entity_id, Duration::from_millis(800)).await;
}

pub async fn schedule_index_entity_with_delay(
    app: &AppHandle,
    entity_type: &str,
    entity_id: &str,
    delay: Duration,
) {
    let key = format!("{}:{}", entity_type, entity_id);
    let state = app.state::<AppState>();

    {
        let mut jobs = state.semantic_index_jobs.lock().await;
        if let Some(prev) = jobs.remove(&key) {
            prev.task.abort();
        }

        let job_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        let app_handle = app.clone();
        let app_for_cleanup = app.clone();
        let key_for_cleanup = key.clone();
        let entity_type = entity_type.to_string();
        let entity_id = entity_id.to_string();
        let task = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            index_entity(&app_handle, entity_type, entity_id).await;

            let state = app_for_cleanup.state::<AppState>();
            let mut jobs = state.semantic_index_jobs.lock().await;
            if let Some(job) = jobs.get(&key_for_cleanup) {
                if job.id == job_id {
                    jobs.remove(&key_for_cleanup);
                }
            }
        });

        jobs.insert(key, SemanticIndexJob { id: job_id, task });
    }
}
