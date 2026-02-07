//! Unified local ASR (automatic speech recognition) model selection.
//!
//! Phantom Harness historically supported local transcription via Whisper only.
//! This module adds Parakeet as a second local engine and provides a single,
//! engine-agnostic model catalog and "active local model" state for the UI and
//! transcription subsystems.

use crate::{parakeet_model, whisper_model, AppState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, State};

pub const LOCAL_ASR_STATUS_EVENT: &str = "LocalAsrModelStatus";
pub const LOCAL_ASR_PROGRESS_EVENT: &str = "LocalAsrModelProgress";

const ACTIVE_LOCAL_ASR_FILE: &str = "active_local_asr.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalAsrEngine {
    Whisper,
    Parakeet,
}

impl LocalAsrEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            LocalAsrEngine::Whisper => "whisper",
            LocalAsrEngine::Parakeet => "parakeet",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "whisper" => Some(LocalAsrEngine::Whisper),
            "parakeet" => Some(LocalAsrEngine::Parakeet),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveLocalAsrModel {
    pub engine: LocalAsrEngine,
    pub model_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalAsrModelState {
    Missing,
    Downloading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalAsrDownloadProgress {
    #[serde(rename = "downloadedBytes")]
    pub downloaded_bytes: u64,
    #[serde(rename = "totalBytes")]
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalAsrModelStatus {
    pub state: LocalAsrModelState,
    pub engine: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub progress: Option<LocalAsrDownloadProgress>,
    pub error: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct LocalAsrModelProgress {
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub progress: u8,
}

#[derive(Clone, Serialize)]
pub struct LocalAsrModelSpec {
    // Composite key used by the UI: "<engine>:<model_id>"
    pub id: String,
    pub label: String,
    pub approx_size_mb: u32,
    pub language: String,
    pub engine: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
}

pub fn models_root_dir() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config.join("phantom-harness").join("models")
}

fn active_local_state_path() -> PathBuf {
    models_root_dir().join(ACTIVE_LOCAL_ASR_FILE)
}

pub fn local_model_key(engine: LocalAsrEngine, model_id: &str) -> String {
    format!("{}:{}", engine.as_str(), model_id)
}

pub fn parse_local_model_key(key: &str) -> Option<(LocalAsrEngine, String)> {
    let mut parts = key.splitn(2, ':');
    let engine = parts.next()?;
    let model_id = parts.next()?;
    let engine = LocalAsrEngine::from_str(engine)?;
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return None;
    }
    Some((engine, model_id.to_string()))
}

pub fn read_active_local_model() -> ActiveLocalAsrModel {
    let path = active_local_state_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(state) = serde_json::from_str::<ActiveLocalAsrModel>(&data) {
            if !state.model_id.trim().is_empty() {
                return state;
            }
        }
    }

    // Migration/default: prefer the legacy Whisper active model file.
    ActiveLocalAsrModel {
        engine: LocalAsrEngine::Whisper,
        model_id: whisper_model::active_model_id(),
    }
}

pub fn write_active_local_model(engine: LocalAsrEngine, model_id: &str) -> Result<(), String> {
    let dir = models_root_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create models directory: {e}"))?;
    let state = ActiveLocalAsrModel {
        engine,
        model_id: model_id.to_string(),
    };
    let json = serde_json::to_string_pretty(&state)
        .map_err(|e| format!("Failed to serialize active local model state: {e}"))?;
    std::fs::write(active_local_state_path(), json)
        .map_err(|e| format!("Failed to write active local model state: {e}"))?;
    Ok(())
}

pub fn active_local_model_key() -> String {
    let active = read_active_local_model();
    local_model_key(active.engine, &active.model_id)
}

fn file_size_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn whisper_specs() -> Vec<LocalAsrModelSpec> {
    whisper_model::model_catalog()
        .into_iter()
        .map(|m| LocalAsrModelSpec {
            id: local_model_key(LocalAsrEngine::Whisper, &m.id),
            label: format!("Whisper · {}", m.label),
            approx_size_mb: m.approx_size_mb,
            language: m.language,
            engine: LocalAsrEngine::Whisper.as_str().to_string(),
            model_id: m.id,
        })
        .collect()
}

fn parakeet_specs() -> Vec<LocalAsrModelSpec> {
    parakeet_model::model_catalog()
        .into_iter()
        .map(|m| LocalAsrModelSpec {
            id: local_model_key(LocalAsrEngine::Parakeet, &m.id),
            label: format!("Parakeet · {}", m.label),
            approx_size_mb: m.approx_size_mb,
            language: m.language,
            engine: LocalAsrEngine::Parakeet.as_str().to_string(),
            model_id: m.id,
        })
        .collect()
}

fn all_specs() -> Vec<LocalAsrModelSpec> {
    let mut out = Vec::new();
    out.extend(whisper_specs());
    out.extend(parakeet_specs());
    out
}

pub fn is_local_model_downloaded(key: &str) -> bool {
    let Some((engine, model_id)) = parse_local_model_key(key) else {
        return false;
    };
    match engine {
        LocalAsrEngine::Whisper => whisper_model::is_model_downloaded(&model_id),
        LocalAsrEngine::Parakeet => parakeet_model::is_model_downloaded(&model_id),
    }
}

pub fn local_model_size_bytes(key: &str) -> u64 {
    let Some((engine, model_id)) = parse_local_model_key(key) else {
        return 0;
    };
    match engine {
        LocalAsrEngine::Whisper => whisper_model::model_path_for_id(&model_id)
            .map(|p| file_size_bytes(&p))
            .unwrap_or(0),
        LocalAsrEngine::Parakeet => parakeet_model::model_size_bytes(&model_id),
    }
}

pub fn emit_local_status(app: &AppHandle, status: &LocalAsrModelStatus) {
    let _ = app.emit(LOCAL_ASR_STATUS_EVENT, status);
}

pub fn emit_local_progress(app: &AppHandle, progress: &LocalAsrModelProgress) {
    let _ = app.emit(LOCAL_ASR_PROGRESS_EVENT, progress);
}

// ---------------------------------------------------------------------------
// Tauri commands (UI-facing)
// ---------------------------------------------------------------------------

/// Unified local model catalog for the Notes Models UI.
///
/// Mirrors the older `check_whisper_model` shape for minimal JS churn.
#[tauri::command]
pub fn check_local_asr_model() -> serde_json::Value {
    let active_key = active_local_model_key();
    let models = all_specs();

    let mut downloaded_map: HashMap<String, bool> = HashMap::new();
    let mut size_map: HashMap<String, u64> = HashMap::new();
    for m in &models {
        let downloaded = is_local_model_downloaded(&m.id);
        downloaded_map.insert(m.id.clone(), downloaded);
        size_map.insert(
            m.id.clone(),
            if downloaded {
                local_model_size_bytes(&m.id)
            } else {
                0
            },
        );
    }

    serde_json::json!({
        "active_model_id": active_key,
        "models": models,
        "downloaded": downloaded_map,
        "size_bytes": size_map,
    })
}

#[tauri::command]
pub async fn local_asr_model_status(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<LocalAsrModelStatus, String> {
    let (engine, inner_id) =
        parse_local_model_key(&model_id).ok_or_else(|| "Invalid model id".to_string())?;

    match engine {
        LocalAsrEngine::Whisper => {
            let st = whisper_model::whisper_model_status(app, state, Some(inner_id)).await?;
            Ok(whisper_model::to_local_asr_status(&st))
        }
        LocalAsrEngine::Parakeet => {
            let st = parakeet_model::parakeet_model_status(state, &inner_id).await?;
            Ok(st)
        }
    }
}

#[tauri::command]
pub async fn download_local_asr_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<LocalAsrModelStatus, String> {
    let (engine, inner_id) =
        parse_local_model_key(&model_id).ok_or_else(|| "Invalid model id".to_string())?;

    match engine {
        LocalAsrEngine::Whisper => {
            let st = whisper_model::download_whisper_model(app, Some(inner_id), state).await?;
            Ok(whisper_model::to_local_asr_status(&st))
        }
        LocalAsrEngine::Parakeet => {
            parakeet_model::download_parakeet_model(app, state, &inner_id).await
        }
    }
}

#[tauri::command]
pub async fn cancel_local_asr_download(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Best-effort: cancel whichever engine is currently downloading.
    // We avoid calling engine-specific cancel commands here to keep this command
    // engine-agnostic and to avoid needing to clone `State`.

    // Cancel Whisper download (if any).
    let whisper_model_id = {
        let mut mgr = state.whisper_models.lock().await;
        if mgr.status.state == whisper_model::WhisperModelState::Downloading {
            let id = mgr.status.model_id.clone();
            if let Some(flag) = mgr.download_cancel.take() {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(task) = mgr.download_task.take() {
                task.abort();
            }
            mgr.status = whisper_model::WhisperModelStatus {
                state: whisper_model::WhisperModelState::Missing,
                model_id: id.clone(),
                progress: None,
                error: None,
                path: None,
            };
            Some(id)
        } else {
            None
        }
    };
    if whisper_model_id.is_some() {
        // Remove any partial downloads (best-effort).
        let dir = whisper_model::models_dir();
        if let Ok(mut entries) = std::fs::read_dir(&dir) {
            while let Some(Ok(ent)) = entries.next() {
                let p = ent.path();
                if p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e == "partial")
                    .unwrap_or(false)
                {
                    let _ = tokio::fs::remove_file(p).await;
                }
            }
        }

        // Emit unified status so the UI updates immediately (best-effort).
        if let Some(inner_id) = whisper_model_id {
            let st = local_status_for_whisper(&inner_id);
            emit_local_status(&app, &st);
        }
    }

    // Cancel Parakeet download (if any).
    let parakeet_inner_id = {
        let mut mgr = state.parakeet_models.lock().await;
        if mgr.status.state == LocalAsrModelState::Downloading {
            let inner = parse_local_model_key(&mgr.status.model_id)
                .map(|(_, id)| id)
                .unwrap_or_else(|| "v3-int8".to_string());
            if let Some(flag) = mgr.download_cancel.take() {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(task) = mgr.download_task.take() {
                task.abort();
            }
            mgr.status = local_status_for_parakeet_missing(&inner);
            Some(inner)
        } else {
            None
        }
    };
    if let Some(inner) = parakeet_inner_id {
        // Remove any partial downloads (best-effort).
        if let Some(dir) = parakeet_model::model_dir_for_id(&inner) {
            if let Ok(mut entries) = std::fs::read_dir(&dir) {
                while let Some(Ok(ent)) = entries.next() {
                    let p = ent.path();
                    if p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e == "partial")
                        .unwrap_or(false)
                    {
                        let _ = tokio::fs::remove_file(p).await;
                    }
                }
            }
        }

        let status = local_status_for_parakeet(&inner);
        emit_local_status(&app, &status);
    }

    Ok(())
}

fn local_status_for_whisper(inner_id: &str) -> LocalAsrModelStatus {
    if whisper_model::is_model_downloaded(inner_id) {
        let path =
            whisper_model::model_path_for_id(inner_id).map(|p| p.to_string_lossy().to_string());
        LocalAsrModelStatus {
            state: LocalAsrModelState::Ready,
            engine: LocalAsrEngine::Whisper.as_str().to_string(),
            model_id: local_model_key(LocalAsrEngine::Whisper, inner_id),
            progress: None,
            error: None,
            path,
        }
    } else {
        LocalAsrModelStatus {
            state: LocalAsrModelState::Missing,
            engine: LocalAsrEngine::Whisper.as_str().to_string(),
            model_id: local_model_key(LocalAsrEngine::Whisper, inner_id),
            progress: None,
            error: None,
            path: None,
        }
    }
}

fn local_status_for_parakeet(inner_id: &str) -> LocalAsrModelStatus {
    if parakeet_model::is_model_downloaded(inner_id) {
        let path =
            parakeet_model::model_dir_for_id(inner_id).map(|p| p.to_string_lossy().to_string());
        LocalAsrModelStatus {
            state: LocalAsrModelState::Ready,
            engine: LocalAsrEngine::Parakeet.as_str().to_string(),
            model_id: local_model_key(LocalAsrEngine::Parakeet, inner_id),
            progress: None,
            error: None,
            path,
        }
    } else {
        local_status_for_parakeet_missing(inner_id)
    }
}

fn local_status_for_parakeet_missing(inner_id: &str) -> LocalAsrModelStatus {
    LocalAsrModelStatus {
        state: LocalAsrModelState::Missing,
        engine: LocalAsrEngine::Parakeet.as_str().to_string(),
        model_id: local_model_key(LocalAsrEngine::Parakeet, inner_id),
        progress: None,
        error: None,
        path: None,
    }
}

#[tauri::command]
pub async fn delete_local_asr_model(
    state: State<'_, AppState>,
    model_id: String,
) -> Result<(), String> {
    let (engine, inner_id) =
        parse_local_model_key(&model_id).ok_or_else(|| "Invalid model id".to_string())?;

    match engine {
        LocalAsrEngine::Whisper => whisper_model::delete_whisper_model(Some(inner_id), state).await,
        LocalAsrEngine::Parakeet => parakeet_model::delete_parakeet_model(state, &inner_id).await,
    }
}

#[tauri::command]
pub fn set_active_local_asr_model(
    state: State<'_, AppState>,
    model_id: String,
) -> Result<(), String> {
    let (engine, inner_id) =
        parse_local_model_key(&model_id).ok_or_else(|| "Invalid model id".to_string())?;

    match engine {
        LocalAsrEngine::Whisper => {
            whisper_model::set_active_whisper_model(Some(inner_id), state)?;
            Ok(())
        }
        LocalAsrEngine::Parakeet => {
            if !parakeet_model::is_model_downloaded(&inner_id) {
                return Err("Model is not downloaded".to_string());
            }
            write_active_local_model(LocalAsrEngine::Parakeet, &inner_id)?;
            if let Ok(mut mgr) = state.meeting_manager.lock() {
                mgr.reset_local_asr_model()?;
            }
            if let Ok(mut svc) = state.dictation.lock() {
                let _ = svc.reset_local_asr_model();
            }
            Ok(())
        }
    }
}
