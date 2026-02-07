use rusqlite::{Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::State;

use crate::utils::truncate_str;
use crate::{embedding_inference, embedding_model};

pub const ENTITY_TYPE_TASK: &str = "task";
pub const ENTITY_TYPE_NOTE: &str = "note";

pub fn pack_f32_embedding(embedding: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(embedding.len().saturating_mul(4));
    for &v in embedding {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub fn unpack_f32_embedding(blob: &[u8]) -> Result<Vec<f32>, String> {
    if blob.len() % 4 != 0 {
        return Err("Invalid embedding blob length (must be multiple of 4)".to_string());
    }
    let mut out = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticIndexStatus {
    pub fts_available: bool,
    pub chunks_total: i64,
    pub chunks_by_type: HashMap<String, i64>,
    pub last_updated_at: Option<i64>,
    /// Best-effort: number of debounced indexing jobs currently queued/running.
    /// `None` means "couldn't sample" (mutex contended).
    pub pending_jobs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSearchRequest {
    pub query: String,
    pub types: Option<Vec<String>>,
    pub limit: Option<u32>,
    pub exact: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSearchResult {
    pub entity_type: String,
    pub entity_id: String,
    pub title: Option<String>,
    pub snippet: Option<String>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticDeleteForEntityRequest {
    pub entity_type: String,
    pub entity_id: String,
}

#[tauri::command]
pub fn semantic_index_status(
    state: State<'_, crate::AppState>,
) -> Result<SemanticIndexStatus, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    let fts_available = semantic_fts_available(&conn);
    let chunks_total = semantic_chunks_count(&conn).map_err(|e| e.to_string())?;
    let chunks_by_type = semantic_chunks_count_by_type(&conn).map_err(|e| e.to_string())?;
    let last_updated_at = semantic_chunks_last_updated_at(&conn).map_err(|e| e.to_string())?;
    let pending_jobs = match state.semantic_index_jobs.try_lock() {
        Ok(jobs) => Some(jobs.len() as u32),
        Err(_) => None,
    };

    Ok(SemanticIndexStatus {
        fts_available,
        chunks_total,
        chunks_by_type,
        last_updated_at,
        pending_jobs,
    })
}

#[tauri::command]
pub async fn semantic_search(
    state: State<'_, crate::AppState>,
    req: SemanticSearchRequest,
) -> Result<Vec<SemanticSearchResult>, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || semantic_search_sync(&db, req))
        .await
        .map_err(|e| format!("Search worker failed: {e}"))?
}

fn semantic_search_sync(
    db: &std::sync::Arc<std::sync::Mutex<Connection>>,
    req: SemanticSearchRequest,
) -> Result<Vec<SemanticSearchResult>, String> {
    let query = req.query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let limit = req.limit.unwrap_or(20).clamp(1, 50) as i64;
    let types = req.types.unwrap_or_default();
    let include_tasks = types.is_empty() || types.iter().any(|t| t == ENTITY_TYPE_TASK);
    let include_notes = types.is_empty() || types.iter().any(|t| t == ENTITY_TYPE_NOTE);
    let exact = req.exact.unwrap_or(false);

    let mut candidates: Vec<SemanticSearchResult> = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        if semantic_fts_available(&conn) {
            if semantic_fts_count(&conn).unwrap_or(0) == 0
                || semantic_fts_is_stale(&conn).unwrap_or(false)
            {
                let _ = rebuild_semantic_fts(&conn);
            }
            semantic_search_via_fts(&conn, &query, include_tasks, include_notes, limit)
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    };

    if candidates.is_empty() {
        let like = like_pattern(&query);
        let conn = db.lock().map_err(|e| e.to_string())?;
        if include_tasks {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT
                        t.id,
                        t.title_summary,
                        t.prompt,
                        (
                            SELECT m.content
                            FROM messages m
                            WHERE m.task_id = t.id
                              AND m.content IS NOT NULL
                              AND m.content LIKE ?1 ESCAPE '\\'
                            ORDER BY m.id DESC
                            LIMIT 1
                        ) AS msg_snippet,
                        t.updated_at
                     FROM tasks t
                     WHERE (
                        t.title_summary LIKE ?1 ESCAPE '\\'
                        OR t.prompt LIKE ?1 ESCAPE '\\'
                        OR EXISTS(
                            SELECT 1
                            FROM messages m2
                            WHERE m2.task_id = t.id
                              AND m2.content IS NOT NULL
                              AND m2.content LIKE ?1 ESCAPE '\\'
                        )
                     )
                     ORDER BY t.updated_at DESC
                     LIMIT ?2",
                )
                .map_err(|e| e.to_string())?;
            let mut rows = stmt.query((&like, limit)).map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let id: String = row.get(0).map_err(|e| e.to_string())?;
                let title_summary: Option<String> = row.get(1).map_err(|e| e.to_string())?;
                let prompt: Option<String> = row.get(2).map_err(|e| e.to_string())?;
                let msg_snippet: Option<String> = row.get(3).map_err(|e| e.to_string())?;

                let title = title_summary.or_else(|| prompt.as_ref().map(|p| truncate_str(p, 80)));
                let snippet = msg_snippet.map(|s| truncate_str(&s, 200));

                candidates.push(SemanticSearchResult {
                    entity_type: ENTITY_TYPE_TASK.to_string(),
                    entity_id: id,
                    title,
                    snippet,
                    score: 0.0,
                });
            }
        }

        if include_notes {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT
                        s.id,
                        s.title,
                        (
                            SELECT seg.text
                            FROM meeting_segments seg
                            WHERE seg.session_id = s.id
                              AND seg.text LIKE ?1 ESCAPE '\\'
                            ORDER BY seg.start_ms DESC
                            LIMIT 1
                        ) AS seg_snippet,
                        s.updated_at
                     FROM meeting_sessions s
                     WHERE (
                        s.title LIKE ?1 ESCAPE '\\'
                        OR EXISTS(
                            SELECT 1
                            FROM meeting_segments seg2
                            WHERE seg2.session_id = s.id
                              AND seg2.text LIKE ?1 ESCAPE '\\'
                        )
                     )
                     ORDER BY s.updated_at DESC
                     LIMIT ?2",
                )
                .map_err(|e| e.to_string())?;
            let mut rows = stmt.query((&like, limit)).map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                let id: String = row.get(0).map_err(|e| e.to_string())?;
                let title: Option<String> = row.get(1).map_err(|e| e.to_string())?;
                let seg_snippet: Option<String> = row.get(2).map_err(|e| e.to_string())?;

                let snippet = seg_snippet.map(|s| truncate_str(&s, 200));

                candidates.push(SemanticSearchResult {
                    entity_type: ENTITY_TYPE_NOTE.to_string(),
                    entity_id: id,
                    title: title.or_else(|| Some("Untitled note".to_string())),
                    snippet,
                    score: 0.0,
                });
            }
        }
    }

    if candidates.is_empty() || exact {
        candidates.truncate(limit as usize);
        return Ok(candidates);
    }

    let model_id = embedding_model::DEFAULT_EMBEDDING_MODEL_ID;
    let onnx_path = embedding_model::model_dir(model_id)
        .join("onnx")
        .join("model.onnx");
    if !onnx_path.exists() {
        candidates.truncate(limit as usize);
        return Ok(candidates);
    }

    let query_emb = match embedding_inference::embed_text_sync(model_id, 128, &query) {
        Ok(v) => v,
        Err(_) => {
            candidates.truncate(limit as usize);
            return Ok(candidates);
        }
    };

    let conn = db.lock().map_err(|e| e.to_string())?;
    for r in &mut candidates {
        if let Ok(Some(best)) = best_similarity_for_entity(&conn, model_id, r, &query_emb) {
            r.score = best as f64;
        }
    }

    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    candidates.truncate(limit as usize);
    Ok(candidates)
}

fn best_similarity_for_entity(
    conn: &Connection,
    model_id: &str,
    r: &SemanticSearchResult,
    query_emb: &[f32],
) -> Result<Option<f32>, String> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT embedding
             FROM semantic_chunks
             WHERE entity_type = ?1 AND entity_id = ?2 AND model_name = ?3",
        )
        .map_err(|e| e.to_string())?;

    let mut rows = stmt
        .query((&r.entity_type, &r.entity_id, model_id))
        .map_err(|e| e.to_string())?;

    let mut best: Option<f32> = None;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let blob: Vec<u8> = row.get(0).map_err(|e| e.to_string())?;
        let sim = dot_product_f32_le(&blob, query_emb)?;
        best = Some(best.map(|b| b.max(sim)).unwrap_or(sim));
    }
    Ok(best)
}

fn dot_product_f32_le(blob: &[u8], v: &[f32]) -> Result<f32, String> {
    if blob.len() != v.len() * 4 {
        return Err("Embedding dims mismatch".to_string());
    }
    let mut sum = 0f32;
    for (i, chunk) in blob.chunks_exact(4).enumerate() {
        let a = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        sum += a * v[i];
    }
    Ok(sum)
}

#[tauri::command]
pub fn semantic_reindex_all(state: State<'_, crate::AppState>) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    if !semantic_fts_available(&conn) {
        return Ok(());
    }
    rebuild_semantic_fts(&conn).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn semantic_delete_for_entity(
    state: State<'_, crate::AppState>,
    req: SemanticDeleteForEntityRequest,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    conn.execute(
        "DELETE FROM semantic_chunks WHERE entity_type = ?1 AND entity_id = ?2",
        (&req.entity_type, &req.entity_id),
    )
    .map_err(|e| e.to_string())?;

    if semantic_fts_available(&conn) {
        let _ = conn.execute(
            "DELETE FROM semantic_fts WHERE entity_type = ?1 AND entity_id = ?2",
            (&req.entity_type, &req.entity_id),
        );
    }

    Ok(())
}

pub fn sqlite_table_exists(conn: &Connection, table_name: &str) -> Result<bool> {
    let mut stmt = conn.prepare_cached(
        "SELECT 1
         FROM sqlite_master
         WHERE type = 'table' AND name = ?1
         LIMIT 1",
    )?;
    let mut rows = stmt.query([table_name])?;
    Ok(rows.next()?.is_some())
}

pub fn semantic_fts_available(conn: &Connection) -> bool {
    sqlite_table_exists(conn, "semantic_fts").unwrap_or(false)
}

fn semantic_fts_count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(1) FROM semantic_fts", [], |row| row.get(0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SemanticFtsFingerprint {
    tasks_count: i64,
    tasks_max_updated_at: i64,
    messages_count: i64,
    messages_max_id: i64,
    meeting_sessions_count: i64,
    meeting_sessions_max_updated_at: i64,
    meeting_segments_count: i64,
    meeting_segments_max_id: i64,
}

fn semantic_fts_fingerprint(conn: &Connection) -> Result<SemanticFtsFingerprint> {
    conn.query_row(
        "SELECT
            (SELECT COUNT(1) FROM tasks),
            COALESCE((SELECT MAX(updated_at) FROM tasks), 0),
            (SELECT COUNT(1) FROM messages),
            COALESCE((SELECT MAX(id) FROM messages), 0),
            (SELECT COUNT(1) FROM meeting_sessions),
            COALESCE((SELECT MAX(updated_at) FROM meeting_sessions), 0),
            (SELECT COUNT(1) FROM meeting_segments),
            COALESCE((SELECT MAX(id) FROM meeting_segments), 0)",
        [],
        |row| {
            Ok(SemanticFtsFingerprint {
                tasks_count: row.get(0)?,
                tasks_max_updated_at: row.get(1)?,
                messages_count: row.get(2)?,
                messages_max_id: row.get(3)?,
                meeting_sessions_count: row.get(4)?,
                meeting_sessions_max_updated_at: row.get(5)?,
                meeting_segments_count: row.get(6)?,
                meeting_segments_max_id: row.get(7)?,
            })
        },
    )
}

fn semantic_fts_is_stale(conn: &Connection) -> Result<bool> {
    if !sqlite_table_exists(conn, "semantic_fts_meta")? {
        return Ok(true);
    }

    let stored: Option<SemanticFtsFingerprint> = conn
        .query_row(
            "SELECT
                tasks_count,
                tasks_max_updated_at,
                messages_count,
                messages_max_id,
                meeting_sessions_count,
                meeting_sessions_max_updated_at,
                meeting_segments_count,
                meeting_segments_max_id
             FROM semantic_fts_meta
             WHERE id = 1",
            [],
            |row| {
                Ok(SemanticFtsFingerprint {
                    tasks_count: row.get(0)?,
                    tasks_max_updated_at: row.get(1)?,
                    messages_count: row.get(2)?,
                    messages_max_id: row.get(3)?,
                    meeting_sessions_count: row.get(4)?,
                    meeting_sessions_max_updated_at: row.get(5)?,
                    meeting_segments_count: row.get(6)?,
                    meeting_segments_max_id: row.get(7)?,
                })
            },
        )
        .optional()?;

    let Some(stored) = stored else {
        return Ok(true);
    };

    Ok(stored != semantic_fts_fingerprint(conn)?)
}

pub fn semantic_chunks_count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(1) FROM semantic_chunks", [], |row| row.get(0))
}

pub fn semantic_chunks_count_by_type(conn: &Connection) -> Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT entity_type, COUNT(1)
         FROM semantic_chunks
         GROUP BY entity_type",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = HashMap::new();
    while let Some(row) = rows.next()? {
        let entity_type: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        out.insert(entity_type, count);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack_f32_embedding_roundtrip() {
        let v = vec![0.0_f32, 1.25, -2.5, 12345.0];
        let blob = pack_f32_embedding(&v);
        let back = unpack_f32_embedding(&blob).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn test_unpack_f32_embedding_rejects_bad_length() {
        let err = unpack_f32_embedding(&[1, 2, 3]).unwrap_err();
        assert!(err.contains("multiple of 4"));
    }

    #[test]
    fn test_embedding_blob_in_sqlite_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE semantic_chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                embedding BLOB NOT NULL
             )",
            [],
        )
        .unwrap();

        let embedding = vec![0.1_f32, 0.2, 0.3, 0.4];
        let blob = pack_f32_embedding(&embedding);
        conn.execute(
            "INSERT INTO semantic_chunks (embedding) VALUES (?1)",
            [&blob],
        )
        .unwrap();

        let read: Vec<u8> = conn
            .query_row("SELECT embedding FROM semantic_chunks LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let back = unpack_f32_embedding(&read).unwrap();
        assert_eq!(back, embedding);
    }
}

pub fn semantic_chunks_last_updated_at(conn: &Connection) -> Result<Option<i64>> {
    conn.query_row("SELECT MAX(updated_at) FROM semantic_chunks", [], |row| {
        row.get(0)
    })
}

fn rebuild_semantic_fts(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM semantic_fts;

         INSERT INTO semantic_fts(text, entity_type, entity_id, field, chunk_index)
         SELECT
            COALESCE(NULLIF(title_summary, ''), NULLIF(prompt, '')),
            'task',
            id,
            'title',
            0
         FROM tasks
         WHERE COALESCE(NULLIF(title_summary, ''), NULLIF(prompt, '')) IS NOT NULL;

         INSERT INTO semantic_fts(text, entity_type, entity_id, field, chunk_index)
         SELECT
            content,
            'task',
            task_id,
            'body',
            id
         FROM messages
         WHERE content IS NOT NULL AND content <> '';

         INSERT INTO semantic_fts(text, entity_type, entity_id, field, chunk_index)
         SELECT
            title,
            'note',
            id,
            'title',
            0
         FROM meeting_sessions
         WHERE title IS NOT NULL AND title <> '';

         INSERT INTO semantic_fts(text, entity_type, entity_id, field, chunk_index)
         SELECT
            text,
            'note',
            session_id,
            'body',
            id
         FROM meeting_segments
         WHERE text <> '';

         INSERT INTO semantic_fts(semantic_fts) VALUES('optimize');",
    )?;

    // Track a fingerprint of the source tables so we can cheaply detect staleness.
    // Best-effort: if this fails, we'll just rebuild more often.
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS semantic_fts_meta (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            tasks_count INTEGER NOT NULL,
            tasks_max_updated_at INTEGER NOT NULL,
            messages_count INTEGER NOT NULL,
            messages_max_id INTEGER NOT NULL,
            meeting_sessions_count INTEGER NOT NULL,
            meeting_sessions_max_updated_at INTEGER NOT NULL,
            meeting_segments_count INTEGER NOT NULL,
            meeting_segments_max_id INTEGER NOT NULL,
            rebuilt_at INTEGER NOT NULL
         );

         DELETE FROM semantic_fts_meta;

         INSERT INTO semantic_fts_meta(
            id,
            tasks_count,
            tasks_max_updated_at,
            messages_count,
            messages_max_id,
            meeting_sessions_count,
            meeting_sessions_max_updated_at,
            meeting_segments_count,
            meeting_segments_max_id,
            rebuilt_at
         ) VALUES (
            1,
            (SELECT COUNT(1) FROM tasks),
            COALESCE((SELECT MAX(updated_at) FROM tasks), 0),
            (SELECT COUNT(1) FROM messages),
            COALESCE((SELECT MAX(id) FROM messages), 0),
            (SELECT COUNT(1) FROM meeting_sessions),
            COALESCE((SELECT MAX(updated_at) FROM meeting_sessions), 0),
            (SELECT COUNT(1) FROM meeting_segments),
            COALESCE((SELECT MAX(id) FROM meeting_segments), 0),
            CAST(strftime('%s','now') AS INTEGER)
         );",
    );

    Ok(())
}

fn semantic_search_via_fts(
    conn: &Connection,
    query: &str,
    include_tasks: bool,
    include_notes: bool,
    limit: i64,
) -> Result<Vec<SemanticSearchResult>> {
    let mut out = Vec::new();
    let fts_query = fts5_literal_query(query).unwrap_or_else(|| query.to_string());

    if include_tasks {
        let mut stmt = conn.prepare_cached(
            "SELECT entity_id, text, bm25(semantic_fts)
             FROM semantic_fts
             WHERE semantic_fts MATCH ?1 AND entity_type = 'task'
             ORDER BY bm25(semantic_fts)
             LIMIT ?2",
        )?;
        let mut rows = stmt.query((&fts_query, limit * 5))?;
        let mut seen = HashSet::new();
        while let Some(row) = rows.next()? {
            let entity_id: String = row.get(0)?;
            if !seen.insert(entity_id.clone()) {
                continue;
            }
            let text: String = row.get(1)?;
            let bm25: f64 = row.get(2)?;

            let (title_summary, prompt): (Option<String>, Option<String>) = conn
                .query_row(
                    "SELECT title_summary, prompt FROM tasks WHERE id = ?1",
                    [&entity_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .unwrap_or((None, None));
            let title = title_summary.or_else(|| prompt.as_ref().map(|p| truncate_str(p, 80)));

            out.push(SemanticSearchResult {
                entity_type: ENTITY_TYPE_TASK.to_string(),
                entity_id,
                title,
                snippet: Some(truncate_str(&text, 200)),
                score: -bm25,
            });

            if out.len() as i64 >= limit {
                break;
            }
        }
    }

    if out.len() as i64 >= limit {
        return Ok(out);
    }

    if include_notes {
        let mut stmt = conn.prepare_cached(
            "SELECT entity_id, text, bm25(semantic_fts)
             FROM semantic_fts
             WHERE semantic_fts MATCH ?1 AND entity_type = 'note'
             ORDER BY bm25(semantic_fts)
             LIMIT ?2",
        )?;
        let mut rows = stmt.query((&fts_query, limit * 5))?;
        let mut seen = HashSet::new();
        while let Some(row) = rows.next()? {
            let entity_id: String = row.get(0)?;
            if !seen.insert(entity_id.clone()) {
                continue;
            }
            let text: String = row.get(1)?;
            let bm25: f64 = row.get(2)?;

            let title: Option<String> = conn
                .query_row(
                    "SELECT title FROM meeting_sessions WHERE id = ?1",
                    [&entity_id],
                    |r| r.get(0),
                )
                .optional()?;

            out.push(SemanticSearchResult {
                entity_type: ENTITY_TYPE_NOTE.to_string(),
                entity_id,
                title: title.or_else(|| Some("Untitled note".to_string())),
                snippet: Some(truncate_str(&text, 200)),
                score: -bm25,
            });

            if out.len() as i64 >= limit {
                break;
            }
        }
    }

    Ok(out)
}

fn like_pattern(query: &str) -> String {
    let mut out = String::with_capacity(query.len() + 2);
    out.push('%');
    for ch in query.chars() {
        if ch == '%' || ch == '_' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}

fn fts5_literal_query(query: &str) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw in query.split_whitespace() {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        // Escape internal quotes by doubling them, then wrap so tokens are treated literally.
        terms.push(format!("\"{}\"", t.replace('\"', "\"\"")));
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}
