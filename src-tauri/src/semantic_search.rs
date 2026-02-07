use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

use crate::utils::truncate_str;

pub const ENTITY_TYPE_TASK: &str = "task";
pub const ENTITY_TYPE_NOTE: &str = "note";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticIndexStatus {
    pub fts_available: bool,
    pub chunks_total: i64,
    pub chunks_by_type: HashMap<String, i64>,
    pub last_updated_at: Option<i64>,
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

    Ok(SemanticIndexStatus {
        fts_available,
        chunks_total,
        chunks_by_type,
        last_updated_at,
    })
}

#[tauri::command]
pub fn semantic_search(
    state: State<'_, crate::AppState>,
    req: SemanticSearchRequest,
) -> Result<Vec<SemanticSearchResult>, String> {
    let query = req.query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let limit = req.limit.unwrap_or(20).clamp(1, 50) as i64;
    let types = req.types.unwrap_or_default();
    let include_tasks = types.is_empty() || types.iter().any(|t| t == ENTITY_TYPE_TASK);
    let include_notes = types.is_empty() || types.iter().any(|t| t == ENTITY_TYPE_NOTE);

    let like = like_pattern(query);

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut out = Vec::new();

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
                          AND m.content LIKE ?1 ESCAPE '\\\\'
                        ORDER BY m.id DESC
                        LIMIT 1
                    ) AS msg_snippet,
                    t.updated_at
                 FROM tasks t
                 WHERE (
                    t.title_summary LIKE ?1 ESCAPE '\\\\'
                    OR t.prompt LIKE ?1 ESCAPE '\\\\'
                    OR EXISTS(
                        SELECT 1
                        FROM messages m2
                        WHERE m2.task_id = t.id
                          AND m2.content IS NOT NULL
                          AND m2.content LIKE ?1 ESCAPE '\\\\'
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

            out.push(SemanticSearchResult {
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
                          AND seg.text LIKE ?1 ESCAPE '\\\\'
                        ORDER BY seg.start_ms DESC
                        LIMIT 1
                    ) AS seg_snippet,
                    s.updated_at
                 FROM meeting_sessions s
                 WHERE (
                    s.title LIKE ?1 ESCAPE '\\\\'
                    OR EXISTS(
                        SELECT 1
                        FROM meeting_segments seg2
                        WHERE seg2.session_id = s.id
                          AND seg2.text LIKE ?1 ESCAPE '\\\\'
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

            out.push(SemanticSearchResult {
                entity_type: ENTITY_TYPE_NOTE.to_string(),
                entity_id: id,
                title: title.or_else(|| Some("Untitled note".to_string())),
                snippet,
                score: 0.0,
            });
        }
    }

    Ok(out)
}

#[tauri::command]
pub fn semantic_reindex_all(state: State<'_, crate::AppState>) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    if !semantic_fts_available(&conn) {
        return Ok(());
    }

    conn.execute("DELETE FROM semantic_fts", [])
        .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO semantic_fts(rowid, text, entity_type, entity_id, field, chunk_index)
         SELECT id, text, entity_type, entity_id, field, chunk_index
         FROM semantic_chunks",
        [],
    )
    .map_err(|e| e.to_string())?;

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

pub fn semantic_chunks_last_updated_at(conn: &Connection) -> Result<Option<i64>> {
    conn.query_row("SELECT MAX(updated_at) FROM semantic_chunks", [], |row| {
        row.get(0)
    })
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
