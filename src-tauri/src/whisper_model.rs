//! Whisper model management for local transcription.
//!
//! This code downloads Whisper.cpp GGML model files from HuggingFace and keeps
//! track of a user-selected active model.

use crate::local_asr_model;
use crate::AppState;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::task::JoinHandle;

const HF_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const ACTIVE_MODEL_FILE: &str = "active_model.json";
const DEFAULT_MODEL_ID: &str = "small";
const WHISPER_STATUS_EVENT: &str = "WhisperModelStatus";

#[derive(Clone, Serialize)]
pub struct WhisperModelSpec {
    pub id: String,
    pub label: String,
    pub filename: String,
    pub approx_size_mb: u32,
    pub language: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WhisperModelState {
    Missing,
    Downloading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhisperDownloadProgress {
    #[serde(rename = "downloadedBytes")]
    pub downloaded_bytes: u64,
    #[serde(rename = "totalBytes")]
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhisperModelStatus {
    pub state: WhisperModelState,
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub progress: Option<WhisperDownloadProgress>,
    pub error: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Serialize)]
struct WhisperModelProgress {
    model_id: String,
    downloaded: u64,
    total: u64,
    progress: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveModelState {
    model_id: String,
}

#[derive(Debug)]
pub struct WhisperModelManagerState {
    pub status: WhisperModelStatus,
    pub download_cancel: Option<Arc<AtomicBool>>,
    pub download_task: Option<JoinHandle<()>>,
}

impl Default for WhisperModelManagerState {
    fn default() -> Self {
        Self {
            status: missing_status(DEFAULT_MODEL_ID),
            download_cancel: None,
            download_task: None,
        }
    }
}

pub(crate) fn model_catalog() -> Vec<WhisperModelSpec> {
    // Matches the canonical whisper.cpp model set. Sizes are approximate and
    // meant for UI only.
    let items: Vec<(&str, &str, &str, u32, &str)> = vec![
        (
            "tiny.en",
            "Tiny (English)",
            "ggml-tiny.en.bin",
            75,
            "English-only",
        ),
        ("tiny", "Tiny", "ggml-tiny.bin", 75, "Multilingual"),
        (
            "base.en",
            "Base (English)",
            "ggml-base.en.bin",
            142,
            "English-only",
        ),
        ("base", "Base", "ggml-base.bin", 142, "Multilingual"),
        (
            "small.en",
            "Small (English)",
            "ggml-small.en.bin",
            466,
            "English-only",
        ),
        ("small", "Small", "ggml-small.bin", 466, "Multilingual"),
        (
            "medium.en",
            "Medium (English)",
            "ggml-medium.en.bin",
            1460,
            "English-only",
        ),
        ("medium", "Medium", "ggml-medium.bin", 1460, "Multilingual"),
        (
            "large-v1",
            "Large v1",
            "ggml-large-v1.bin",
            2950,
            "Multilingual",
        ),
        (
            "large-v2",
            "Large v2",
            "ggml-large-v2.bin",
            2950,
            "Multilingual",
        ),
        (
            "large-v3",
            "Large v3",
            "ggml-large-v3.bin",
            2950,
            "Multilingual",
        ),
        (
            "large-v3-turbo",
            "Large v3 Turbo",
            "ggml-large-v3-turbo.bin",
            1550,
            "Multilingual",
        ),
    ];

    items
        .into_iter()
        .map(
            |(id, label, filename, approx_size_mb, language)| WhisperModelSpec {
                id: id.to_string(),
                label: label.to_string(),
                filename: filename.to_string(),
                approx_size_mb,
                language: language.to_string(),
                url: format!("{}/{}", HF_BASE_URL, filename),
            },
        )
        .collect()
}

pub fn to_local_asr_status(st: &WhisperModelStatus) -> local_asr_model::LocalAsrModelStatus {
    let state = match st.state {
        WhisperModelState::Missing => local_asr_model::LocalAsrModelState::Missing,
        WhisperModelState::Downloading => local_asr_model::LocalAsrModelState::Downloading,
        WhisperModelState::Ready => local_asr_model::LocalAsrModelState::Ready,
        WhisperModelState::Error => local_asr_model::LocalAsrModelState::Error,
    };

    local_asr_model::LocalAsrModelStatus {
        state,
        engine: local_asr_model::LocalAsrEngine::Whisper
            .as_str()
            .to_string(),
        model_id: local_asr_model::local_model_key(
            local_asr_model::LocalAsrEngine::Whisper,
            &st.model_id,
        ),
        progress: st
            .progress
            .as_ref()
            .map(|p| local_asr_model::LocalAsrDownloadProgress {
                downloaded_bytes: p.downloaded_bytes,
                total_bytes: p.total_bytes,
            }),
        error: st.error.clone(),
        path: st.path.clone(),
    }
}

/// Returns the directory where whisper models are stored.
pub fn models_dir() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config
        .join("phantom-harness")
        .join("models")
        .join("whisper")
}

fn active_model_state_path() -> PathBuf {
    models_dir().join(ACTIVE_MODEL_FILE)
}

fn read_active_model_id() -> String {
    let path = active_model_state_path();
    let Ok(data) = std::fs::read_to_string(&path) else {
        return DEFAULT_MODEL_ID.to_string();
    };
    let Ok(state) = serde_json::from_str::<ActiveModelState>(&data) else {
        return DEFAULT_MODEL_ID.to_string();
    };
    if state.model_id.trim().is_empty() {
        DEFAULT_MODEL_ID.to_string()
    } else {
        state.model_id
    }
}

fn write_active_model_id(model_id: &str) -> Result<(), String> {
    let dir = models_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create models directory: {}", e))?;
    let state = ActiveModelState {
        model_id: model_id.to_string(),
    };
    let json = serde_json::to_string_pretty(&state)
        .map_err(|e| format!("Failed to serialize active model state: {}", e))?;
    std::fs::write(active_model_state_path(), json)
        .map_err(|e| format!("Failed to write active model state: {}", e))?;
    Ok(())
}

fn spec_by_id(model_id: &str) -> Option<WhisperModelSpec> {
    model_catalog().into_iter().find(|m| m.id == model_id)
}

pub fn model_path_for_id(model_id: &str) -> Option<PathBuf> {
    let spec = spec_by_id(model_id)?;
    Some(models_dir().join(spec.filename))
}

pub fn active_model_id() -> String {
    read_active_model_id()
}

pub fn active_model_path() -> PathBuf {
    model_path_for_id(&active_model_id()).unwrap_or_else(|| models_dir().join("ggml-small.bin"))
}

pub fn is_model_downloaded(model_id: &str) -> bool {
    model_path_for_id(model_id)
        .map(|p| p.exists())
        .unwrap_or(false)
}

fn file_size_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn missing_status(model_id: &str) -> WhisperModelStatus {
    WhisperModelStatus {
        state: WhisperModelState::Missing,
        model_id: model_id.to_string(),
        progress: None,
        error: None,
        path: None,
    }
}

fn ready_status(model_id: &str, path: &PathBuf) -> WhisperModelStatus {
    WhisperModelStatus {
        state: WhisperModelState::Ready,
        model_id: model_id.to_string(),
        progress: None,
        error: None,
        path: Some(path.to_string_lossy().to_string()),
    }
}

fn error_status(model_id: &str, message: String) -> WhisperModelStatus {
    WhisperModelStatus {
        state: WhisperModelState::Error,
        model_id: model_id.to_string(),
        progress: None,
        error: Some(message),
        path: None,
    }
}

fn downloading_status(
    model_id: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) -> WhisperModelStatus {
    WhisperModelStatus {
        state: WhisperModelState::Downloading,
        model_id: model_id.to_string(),
        progress: Some(WhisperDownloadProgress {
            downloaded_bytes,
            total_bytes,
        }),
        error: None,
        path: None,
    }
}

fn emit_status(app: &AppHandle, status: &WhisperModelStatus) {
    let _ = app.emit(WHISPER_STATUS_EVENT, status);
    let local = to_local_asr_status(status);
    local_asr_model::emit_local_status(app, &local);
}

async fn update_status(app: &AppHandle, state: &AppState, status: WhisperModelStatus) {
    {
        let mut mgr = state.whisper_models.lock().await;
        mgr.status = status.clone();
    }
    emit_status(app, &status);
}

async fn is_current_download(state: &AppState, cancel_flag: &Arc<AtomicBool>) -> bool {
    let mgr = state.whisper_models.lock().await;
    mgr.download_cancel
        .as_ref()
        .map(|f| Arc::ptr_eq(f, cancel_flag))
        .unwrap_or(false)
}

async fn clear_download_state_if_current(state: &AppState, cancel_flag: &Arc<AtomicBool>) {
    let mut mgr = state.whisper_models.lock().await;
    if mgr
        .download_cancel
        .as_ref()
        .map(|f| Arc::ptr_eq(f, cancel_flag))
        .unwrap_or(false)
    {
        mgr.download_cancel = None;
        mgr.download_task = None;
    }
}

fn resolve_model_id(model_id: Option<String>) -> String {
    let candidate = model_id.unwrap_or_else(|| DEFAULT_MODEL_ID.to_string());
    if spec_by_id(&candidate).is_some() {
        candidate
    } else {
        DEFAULT_MODEL_ID.to_string()
    }
}

async fn refresh_status(app: &AppHandle, state: &AppState, model_id: &str) -> WhisperModelStatus {
    {
        let mgr = state.whisper_models.lock().await;
        if mgr.status.state == WhisperModelState::Downloading && mgr.status.model_id == model_id {
            return mgr.status.clone();
        }
    }

    let Some(path) = model_path_for_id(model_id) else {
        let status = error_status(model_id, "Unknown model".to_string());
        update_status(app, state, status.clone()).await;
        return status;
    };

    let status = if path.exists() {
        ready_status(model_id, &path)
    } else {
        missing_status(model_id)
    };

    {
        let mut mgr = state.whisper_models.lock().await;
        mgr.status = status.clone();
    }

    status
}

/// Downloads a whisper model with streaming progress events.
///
/// Emits `WhisperModelProgress` events via the Tauri app handle during download.
/// The file is first written to a `.partial` file, then atomically renamed on completion.
async fn download_model(
    app: &AppHandle,
    state: &AppState,
    model_id: &str,
    cancel_flag: Arc<AtomicBool>,
) -> Result<PathBuf, String> {
    let spec = spec_by_id(model_id).ok_or_else(|| "Unknown model".to_string())?;

    let dir = models_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create models directory: {}", e))?;

    let model_path = dir.join(&spec.filename);
    let tmp_path = dir.join(format!("{}.partial", spec.filename));

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| format!("Failed to configure download client: {}", e))?;

    let response = client
        .get(&spec.url)
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let total_opt = response.content_length();
    let total = total_opt.unwrap_or(0);
    let mut downloaded: u64 = 0;

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut stream = response.bytes_stream();
    let mut last_progress_emit = Instant::now();

    while let Some(chunk) = stream.next().await {
        if cancel_flag.load(Ordering::Relaxed) || !is_current_download(state, &cancel_flag).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err("Download canceled".to_string());
        }

        let chunk = chunk.map_err(|e| format!("Error during download: {}", e))?;

        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| format!("Failed to write chunk: {}", e))?;

        downloaded += chunk.len() as u64;

        if last_progress_emit.elapsed() >= Duration::from_millis(150) {
            last_progress_emit = Instant::now();

            let pct: u8 = if total > 0 {
                ((downloaded as f64 / total as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8
            } else {
                0
            };

            // Back-compat progress event (existing UI listens to this).
            let _ = app.emit(
                "WhisperModelProgress",
                WhisperModelProgress {
                    model_id: model_id.to_string(),
                    downloaded,
                    total,
                    progress: pct,
                },
            );
            // Unified progress event (new UI).
            local_asr_model::emit_local_progress(
                app,
                &local_asr_model::LocalAsrModelProgress {
                    model_id: local_asr_model::local_model_key(
                        local_asr_model::LocalAsrEngine::Whisper,
                        model_id,
                    ),
                    downloaded,
                    total,
                    progress: pct,
                },
            );

            // New richer status event (used for polished UX + cancel behavior).
            let status = downloading_status(model_id, downloaded, total_opt);
            update_status(app, state, status).await;
        }
    }

    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| format!("Failed to flush temp file: {}", e))?;
    drop(file);

    tokio::fs::rename(&tmp_path, &model_path)
        .await
        .map_err(|e| format!("Failed to rename temp file to final path: {}", e))?;

    Ok(model_path)
}

pub fn delete_model(model_id: &str) -> Result<(), String> {
    let Some(path) = model_path_for_id(model_id) else {
        return Err("Unknown model".to_string());
    };
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Failed to delete model file: {}", e))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn whisper_model_status(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: Option<String>,
) -> Result<WhisperModelStatus, String> {
    let model_id = resolve_model_id(model_id);
    Ok(refresh_status(&app, state.inner(), &model_id).await)
}

#[tauri::command]
pub fn check_whisper_model() -> serde_json::Value {
    let active_id = active_model_id();
    let models = model_catalog();

    let mut downloaded_map: HashMap<String, bool> = HashMap::new();
    let mut size_map: HashMap<String, u64> = HashMap::new();
    for m in &models {
        let p = models_dir().join(&m.filename);
        let downloaded = p.exists();
        downloaded_map.insert(m.id.clone(), downloaded);
        size_map.insert(
            m.id.clone(),
            if downloaded { file_size_bytes(&p) } else { 0 },
        );
    }

    let active_path = active_model_path();
    serde_json::json!({
        "active_model_id": active_id,
        "active_model_path": active_path.to_string_lossy(),
        "models": models,
        "downloaded": downloaded_map,
        "size_bytes": size_map,
    })
}

#[tauri::command]
pub async fn download_whisper_model(
    app: AppHandle,
    model_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<WhisperModelStatus, String> {
    let model_id = resolve_model_id(model_id);
    let current = refresh_status(&app, state.inner(), &model_id).await;
    if current.state == WhisperModelState::Ready {
        return Ok(current);
    }
    if current.state == WhisperModelState::Downloading && current.model_id == model_id {
        return Ok(current);
    }

    // Prepare new download state; cancel any existing download first.
    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut mgr = state.whisper_models.lock().await;
        if mgr.status.state == WhisperModelState::Downloading && mgr.status.model_id != model_id {
            let prev_model_id = mgr.status.model_id.clone();
            if let Some(flag) = mgr.download_cancel.take() {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(task) = mgr.download_task.take() {
                task.abort();
            }
            // If the task was aborted mid-write, the `.partial` cleanup in
            // download_model may never run.
            if let Some(spec) = spec_by_id(&prev_model_id) {
                let tmp = models_dir().join(format!("{}.partial", spec.filename));
                let _ = std::fs::remove_file(tmp);
            }
        }

        mgr.download_cancel = Some(cancel_flag.clone());
        mgr.status = downloading_status(&model_id, 0, None);
    }

    emit_status(&app, &refresh_status(&app, state.inner(), &model_id).await);

    let app_handle = app.clone();
    let model_id_clone = model_id.clone();
    let cancel_flag_clone = cancel_flag.clone();
    let task = tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        let state_ref = state.inner();

        let result = download_model(
            &app_handle,
            state_ref,
            &model_id_clone,
            cancel_flag_clone.clone(),
        )
        .await;
        match result {
            Ok(path) => {
                // Auto-select the downloaded model as active for a smoother UX.
                if let Err(err) = write_active_model_id(&model_id_clone) {
                    let status = error_status(&model_id_clone, err);
                    update_status(&app_handle, state_ref, status).await;
                    clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
                    return;
                }
                if let Err(err) = local_asr_model::write_active_local_model(
                    local_asr_model::LocalAsrEngine::Whisper,
                    &model_id_clone,
                ) {
                    let status = error_status(&model_id_clone, err);
                    update_status(&app_handle, state_ref, status).await;
                    clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
                    return;
                }

                // If the meeting manager already loaded a different model, reset so next
                // session loads the new one.
                if let Ok(mut mgr) = state_ref.meeting_manager.lock() {
                    let _ = mgr.reset_local_asr_model();
                }
                if let Ok(mut svc) = state_ref.dictation.lock() {
                    let _ = svc.reset_local_asr_model();
                }

                let status = ready_status(&model_id_clone, &path);
                update_status(&app_handle, state_ref, status).await;
                clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
            }
            Err(err) => {
                if err == "Download canceled" {
                    let status = missing_status(&model_id_clone);
                    update_status(&app_handle, state_ref, status).await;
                    clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
                    return;
                }
                let status = error_status(&model_id_clone, err);
                update_status(&app_handle, state_ref, status).await;
                clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
            }
        }
    });

    {
        let mut mgr = state.whisper_models.lock().await;
        mgr.download_task = Some(task);
    }

    Ok(refresh_status(&app, state.inner(), &model_id).await)
}

#[tauri::command]
pub async fn whisper_cancel_download(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: Option<String>,
) -> Result<WhisperModelStatus, String> {
    let model_id = resolve_model_id(model_id);
    {
        let mut mgr = state.whisper_models.lock().await;
        if let Some(flag) = mgr.download_cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(task) = mgr.download_task.take() {
            task.abort();
        }
        mgr.status = missing_status(&model_id);
    }

    if let Some(spec) = spec_by_id(&model_id) {
        let tmp_path = models_dir().join(format!("{}.partial", spec.filename));
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }

    let status = refresh_status(&app, state.inner(), &model_id).await;
    emit_status(&app, &status);
    Ok(status)
}

#[tauri::command]
pub async fn delete_whisper_model(
    model_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let model_id = resolve_model_id(model_id);

    // If deleting the model currently downloading, cancel the background task
    // and remove any partial download file first.
    {
        let mut mgr = state.whisper_models.lock().await;
        if mgr.status.state == WhisperModelState::Downloading && mgr.status.model_id == model_id {
            if let Some(flag) = mgr.download_cancel.take() {
                flag.store(true, Ordering::Relaxed);
            }
            if let Some(task) = mgr.download_task.take() {
                task.abort();
            }
            mgr.status = missing_status(&model_id);
        }
    }
    if let Some(spec) = spec_by_id(&model_id) {
        let tmp_path = models_dir().join(format!("{}.partial", spec.filename));
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }

    delete_model(&model_id)?;

    // If the active model was deleted, fall back to default.
    let active_id = active_model_id();
    if active_id == model_id {
        write_active_model_id(DEFAULT_MODEL_ID)?;
        // Only update the unified local active model if it was pointing at this Whisper model.
        let active_local = local_asr_model::read_active_local_model();
        if active_local.engine == local_asr_model::LocalAsrEngine::Whisper
            && active_local.model_id == model_id
        {
            local_asr_model::write_active_local_model(
                local_asr_model::LocalAsrEngine::Whisper,
                DEFAULT_MODEL_ID,
            )?;
        }
        if let Ok(mut mgr) = state.meeting_manager.lock() {
            let _ = mgr.reset_local_asr_model();
        }
        if let Ok(mut svc) = state.dictation.lock() {
            let _ = svc.reset_local_asr_model();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn set_active_whisper_model(
    model_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let model_id = model_id.unwrap_or_else(|| DEFAULT_MODEL_ID.to_string());
    if !is_model_downloaded(&model_id) {
        return Err("Model is not downloaded".to_string());
    }
    write_active_model_id(&model_id)?;
    local_asr_model::write_active_local_model(local_asr_model::LocalAsrEngine::Whisper, &model_id)?;
    if let Ok(mut mgr) = state.meeting_manager.lock() {
        mgr.reset_local_asr_model()?;
    }
    if let Ok(mut svc) = state.dictation.lock() {
        svc.reset_local_asr_model()?;
    }
    Ok(())
}
