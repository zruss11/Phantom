use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

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
