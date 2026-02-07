//! Local embedding model catalog + download manager.
//!
//! Embedding inference is implemented separately (tokenizer + ORT session), but
//! we keep download logic and on-disk layout here so the rest of the app can
//! reliably "ensure assets exist" without bundling them into the DMG.

use crate::{local_asr_model, AppState};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

pub const DEFAULT_EMBEDDING_MODEL_ID: &str = "all-minilm-l6-v2";
pub const EMBEDDING_STATUS_EVENT: &str = "EmbeddingModelStatus";
pub const EMBEDDING_PROGRESS_EVENT: &str = "EmbeddingModelProgress";

// Xenova hosts a Transformers.js-ready ONNX + tokenizer bundle.
// We use raw-file URLs for direct download.
const HF_BASE_URL: &str = "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModelSpec {
    pub id: String,
    pub label: String,
    pub dims: u32,
    pub approx_size_mb: u32,
}

pub fn embeddings_root_dir() -> PathBuf {
    local_asr_model::models_root_dir().join("embeddings")
}

pub fn model_dir(model_id: &str) -> PathBuf {
    embeddings_root_dir().join(model_id)
}

#[derive(Debug, Clone)]
struct EmbeddingModelFile {
    rel_path: &'static str,
    url: String,
}

pub fn default_model_spec() -> EmbeddingModelSpec {
    // Widely used small SentenceTransformers model (384 dims).
    EmbeddingModelSpec {
        id: DEFAULT_EMBEDDING_MODEL_ID.to_string(),
        label: "all-MiniLM-L6-v2".to_string(),
        dims: 384,
        approx_size_mb: 90,
    }
}

fn model_catalog() -> Vec<EmbeddingModelSpec> {
    vec![default_model_spec()]
}

fn resolve_model_id(model_id: Option<String>) -> String {
    let candidate = model_id.unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL_ID.to_string());
    if model_catalog().iter().any(|m| m.id == candidate) {
        candidate
    } else {
        DEFAULT_EMBEDDING_MODEL_ID.to_string()
    }
}

fn required_files_for_model_id(model_id: &str) -> Option<Vec<EmbeddingModelFile>> {
    if model_id != DEFAULT_EMBEDDING_MODEL_ID {
        return None;
    }
    let mk = |rel_path: &'static str| EmbeddingModelFile {
        rel_path,
        url: format!("{}/{}", HF_BASE_URL, rel_path),
    };

    Some(vec![
        mk("config.json"),
        mk("special_tokens_map.json"),
        mk("tokenizer.json"),
        mk("tokenizer_config.json"),
        mk("vocab.txt"),
        mk("onnx/model.onnx"),
    ])
}

fn is_model_downloaded(model_id: &str) -> bool {
    let Some(files) = required_files_for_model_id(model_id) else {
        return false;
    };
    let dir = model_dir(model_id);
    files.iter().all(|f| dir.join(f.rel_path).exists())
}

// ---------------------------------------------------------------------------
// Status + manager state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingModelState {
    Missing,
    Downloading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingDownloadProgress {
    #[serde(rename = "downloadedBytes")]
    pub downloaded_bytes: u64,
    #[serde(rename = "totalBytes")]
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingModelStatus {
    pub state: EmbeddingModelState,
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub progress: Option<EmbeddingDownloadProgress>,
    pub error: Option<String>,
    pub path: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct EmbeddingModelProgress {
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub progress: u8,
}

#[derive(Default)]
pub struct EmbeddingModelManagerState {
    pub(crate) status: EmbeddingModelStatus,
    download_cancel: Option<Arc<AtomicBool>>,
    download_task: Option<tokio::task::JoinHandle<()>>,
}

impl Default for EmbeddingModelStatus {
    fn default() -> Self {
        Self {
            state: EmbeddingModelState::Missing,
            model_id: DEFAULT_EMBEDDING_MODEL_ID.to_string(),
            progress: None,
            error: None,
            path: None,
        }
    }
}

fn missing_status(model_id: &str) -> EmbeddingModelStatus {
    EmbeddingModelStatus {
        state: EmbeddingModelState::Missing,
        model_id: model_id.to_string(),
        progress: None,
        error: None,
        path: None,
    }
}

fn ready_status(model_id: &str, dir: &PathBuf) -> EmbeddingModelStatus {
    EmbeddingModelStatus {
        state: EmbeddingModelState::Ready,
        model_id: model_id.to_string(),
        progress: None,
        error: None,
        path: Some(dir.to_string_lossy().to_string()),
    }
}

fn error_status(model_id: &str, message: String) -> EmbeddingModelStatus {
    EmbeddingModelStatus {
        state: EmbeddingModelState::Error,
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
) -> EmbeddingModelStatus {
    EmbeddingModelStatus {
        state: EmbeddingModelState::Downloading,
        model_id: model_id.to_string(),
        progress: Some(EmbeddingDownloadProgress {
            downloaded_bytes,
            total_bytes,
        }),
        error: None,
        path: None,
    }
}

fn emit_status(app: &AppHandle, status: &EmbeddingModelStatus) {
    let _ = app.emit(EMBEDDING_STATUS_EVENT, status);
}

fn emit_progress(app: &AppHandle, progress: &EmbeddingModelProgress) {
    let _ = app.emit(EMBEDDING_PROGRESS_EVENT, progress);
}

async fn update_status(app: &AppHandle, state: &AppState, status: EmbeddingModelStatus) {
    {
        let mut mgr = state.embedding_models.lock().await;
        mgr.status = status.clone();
    }
    emit_status(app, &status);
}

async fn is_current_download(state: &AppState, cancel_flag: &Arc<AtomicBool>) -> bool {
    let mgr = state.embedding_models.lock().await;
    mgr.download_cancel
        .as_ref()
        .map(|f| Arc::ptr_eq(f, cancel_flag))
        .unwrap_or(false)
}

async fn clear_download_state_if_current(state: &AppState, cancel_flag: &Arc<AtomicBool>) {
    let mut mgr = state.embedding_models.lock().await;
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

async fn refresh_status(app: &AppHandle, state: &AppState, model_id: &str) -> EmbeddingModelStatus {
    {
        let mgr = state.embedding_models.lock().await;
        if mgr.status.state == EmbeddingModelState::Downloading && mgr.status.model_id == model_id {
            return mgr.status.clone();
        }
    }

    if required_files_for_model_id(model_id).is_none() {
        let st = error_status(model_id, "Unknown model".to_string());
        update_status(app, state, st.clone()).await;
        return st;
    }

    let dir = model_dir(model_id);
    let st = if is_model_downloaded(model_id) {
        ready_status(model_id, &dir)
    } else {
        missing_status(model_id)
    };

    {
        let mut mgr = state.embedding_models.lock().await;
        mgr.status = st.clone();
    }

    st
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

fn file_size_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

async fn download_model_files(
    app: &AppHandle,
    state: &AppState,
    model_id: &str,
    cancel_flag: Arc<AtomicBool>,
) -> Result<PathBuf, String> {
    let Some(files) = required_files_for_model_id(model_id) else {
        return Err("Unknown model".to_string());
    };

    let dir = model_dir(model_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create embeddings model directory: {e}"))?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| format!("Failed to configure download client: {e}"))?;

    // Best-effort total size for progress UX (HEAD may fail).
    let mut total_opt: Option<u64> = Some(0);
    for f in &files {
        if total_opt.is_none() {
            break;
        }
        let resp = match client.head(&f.url).send().await {
            Ok(r) => r,
            Err(_) => {
                total_opt = None;
                continue;
            }
        };
        if !resp.status().is_success() {
            total_opt = None;
            continue;
        }
        match resp.content_length() {
            Some(n) => {
                if let Some(t) = total_opt {
                    total_opt = Some(t.saturating_add(n));
                }
            }
            None => total_opt = None,
        }
    }

    let total_for_pct = total_opt.unwrap_or(0);
    let mut downloaded_total: u64 = 0;
    let mut last_emit = Instant::now();

    // If some files already exist (partial prior run), count their sizes so we
    // don't jump backwards in progress.
    for f in &files {
        let p = dir.join(f.rel_path);
        if p.exists() {
            downloaded_total = downloaded_total.saturating_add(file_size_bytes(&p));
        }
    }

    for f in &files {
        if cancel_flag.load(Ordering::Relaxed) || !is_current_download(state, &cancel_flag).await {
            return Err("Download canceled".to_string());
        }

        let final_path = dir.join(f.rel_path);
        if final_path.exists() {
            continue;
        }

        if let Some(parent) = final_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }

        let tmp_path = dir.join(format!("{}.partial", f.rel_path.replace('/', "_")));

        let resp = client
            .get(&f.url)
            .send()
            .await
            .map_err(|e| format!("Failed to start download: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Download failed with status: {}", resp.status()));
        }

        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| format!("Failed to create temp file: {e}"))?;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if cancel_flag.load(Ordering::Relaxed)
                || !is_current_download(state, &cancel_flag).await
            {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err("Download canceled".to_string());
            }

            let chunk = chunk.map_err(|e| format!("Error during download: {e}"))?;
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| format!("Failed to write chunk: {e}"))?;

            downloaded_total = downloaded_total.saturating_add(chunk.len() as u64);

            if last_emit.elapsed() >= Duration::from_millis(150) {
                last_emit = Instant::now();
                let pct: u8 = if total_for_pct > 0 {
                    ((downloaded_total as f64 / total_for_pct as f64) * 100.0)
                        .round()
                        .clamp(0.0, 100.0) as u8
                } else {
                    0
                };

                emit_progress(
                    app,
                    &EmbeddingModelProgress {
                        model_id: model_id.to_string(),
                        downloaded: downloaded_total,
                        total: total_for_pct,
                        progress: pct,
                    },
                );

                let st = downloading_status(model_id, downloaded_total, total_opt);
                update_status(app, state, st).await;
            }
        }

        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|e| format!("Failed to flush temp file: {e}"))?;
        drop(file);

        tokio::fs::rename(&tmp_path, &final_path)
            .await
            .map_err(|e| format!("Failed to finalize downloaded file: {e}"))?;
    }

    Ok(dir)
}

async fn cleanup_partial_files(model_id: &str) {
    let Some(files) = required_files_for_model_id(model_id) else {
        return;
    };
    let dir = model_dir(model_id);
    for f in &files {
        let tmp_path = dir.join(format!("{}.partial", f.rel_path.replace('/', "_")));
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn embedding_model_status(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: Option<String>,
) -> Result<EmbeddingModelStatus, String> {
    let model_id = resolve_model_id(model_id);
    Ok(refresh_status(&app, state.inner(), &model_id).await)
}

#[tauri::command]
pub async fn download_embedding_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: Option<String>,
) -> Result<EmbeddingModelStatus, String> {
    let model_id = resolve_model_id(model_id);
    let current = refresh_status(&app, state.inner(), &model_id).await;
    if current.state == EmbeddingModelState::Ready {
        return Ok(current);
    }
    if current.state == EmbeddingModelState::Downloading && current.model_id == model_id {
        return Ok(current);
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut mgr = state.embedding_models.lock().await;
        if mgr.status.state == EmbeddingModelState::Downloading && mgr.status.model_id != model_id {
            if let Some(flag) = mgr.download_cancel.take() {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(task) = mgr.download_task.take() {
                task.abort();
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

        let result = download_model_files(
            &app_handle,
            state_ref,
            &model_id_clone,
            cancel_flag_clone.clone(),
        )
        .await;
        match result {
            Ok(dir) => {
                let st = ready_status(&model_id_clone, &dir);
                update_status(&app_handle, state_ref, st).await;
                clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
            }
            Err(err) => {
                if err == "Download canceled" {
                    let st = missing_status(&model_id_clone);
                    update_status(&app_handle, state_ref, st).await;
                    clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
                    return;
                }
                let st = error_status(&model_id_clone, err);
                update_status(&app_handle, state_ref, st).await;
                clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
            }
        }
    });

    {
        let mut mgr = state.embedding_models.lock().await;
        mgr.download_task = Some(task);
    }

    Ok(refresh_status(&app, state.inner(), &model_id).await)
}

#[tauri::command]
pub async fn embedding_cancel_download(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: Option<String>,
) -> Result<EmbeddingModelStatus, String> {
    let model_id = resolve_model_id(model_id);
    {
        let mut mgr = state.embedding_models.lock().await;
        if let Some(flag) = mgr.download_cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(task) = mgr.download_task.take() {
            task.abort();
        }
        mgr.status = missing_status(&model_id);
    }

    cleanup_partial_files(&model_id).await;

    let st = refresh_status(&app, state.inner(), &model_id).await;
    emit_status(&app, &st);
    Ok(st)
}
