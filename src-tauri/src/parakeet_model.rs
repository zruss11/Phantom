//! Parakeet model download/management for local transcription.
//!
//! Parakeet inference is provided via `transcribe-rs` (ONNX Runtime). Models are
//! hosted on Hugging Face and downloaded on-demand.

use crate::{local_asr_model, whisper_model, AppState};
use futures_util::StreamExt;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use tokio::task::JoinHandle;

const HF_BASE_URL: &str = "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main";

const DEFAULT_MODEL_ID: &str = "v3-int8";
const MODEL_DIRNAME_V3_INT8: &str = "parakeet-tdt-0.6b-v3-int8";

#[derive(Clone, Serialize)]
pub struct ParakeetModelSpec {
    pub id: String,
    pub label: String,
    pub approx_size_mb: u32,
    pub language: String,
}

#[derive(Debug)]
pub struct ParakeetModelManagerState {
    pub status: local_asr_model::LocalAsrModelStatus,
    pub download_cancel: Option<Arc<AtomicBool>>,
    pub download_task: Option<JoinHandle<()>>,
}

impl Default for ParakeetModelManagerState {
    fn default() -> Self {
        Self {
            status: missing_status(DEFAULT_MODEL_ID),
            download_cancel: None,
            download_task: None,
        }
    }
}

#[derive(Clone)]
struct ParakeetFileSpec {
    name: &'static str,
    url: String,
}

pub fn model_catalog() -> Vec<ParakeetModelSpec> {
    vec![ParakeetModelSpec {
        id: DEFAULT_MODEL_ID.to_string(),
        label: "v3 (int8)".to_string(),
        // UI-only; actual size varies a bit.
        approx_size_mb: 680,
        language: "English".to_string(),
    }]
}

fn resolve_model_id(model_id: &str) -> Option<&'static str> {
    if model_id == DEFAULT_MODEL_ID {
        Some(DEFAULT_MODEL_ID)
    } else {
        None
    }
}

fn files_for_model_id(model_id: &str) -> Option<Vec<ParakeetFileSpec>> {
    resolve_model_id(model_id)?;
    let files = vec![
        "vocab.txt",
        "nemo128.onnx",
        "encoder-model.int8.onnx",
        "decoder_joint-model.int8.onnx",
    ];
    Some(
        files
            .into_iter()
            .map(|name| ParakeetFileSpec {
                name,
                url: format!("{HF_BASE_URL}/{name}"),
            })
            .collect(),
    )
}

fn estimated_total_bytes_for_model(model_id: &str) -> Option<u64> {
    let spec = model_catalog().into_iter().find(|m| m.id == model_id)?;
    // Best-effort: used for UX progress when the server doesn't provide lengths.
    Some(spec.approx_size_mb as u64 * 1024 * 1024)
}

pub fn models_dir() -> PathBuf {
    local_asr_model::models_root_dir().join("parakeet")
}

pub fn model_dir_for_id(model_id: &str) -> Option<PathBuf> {
    resolve_model_id(model_id)?;
    Some(models_dir().join(MODEL_DIRNAME_V3_INT8))
}

pub fn is_model_downloaded(model_id: &str) -> bool {
    let Some(dir) = model_dir_for_id(model_id) else {
        return false;
    };
    let Some(files) = files_for_model_id(model_id) else {
        return false;
    };
    files
        .iter()
        .all(|f| dir.join(f.name).exists() && dir.join(f.name).is_file())
}

pub fn model_size_bytes(model_id: &str) -> u64 {
    let Some(dir) = model_dir_for_id(model_id) else {
        return 0;
    };
    let Some(files) = files_for_model_id(model_id) else {
        return 0;
    };
    files
        .iter()
        .map(|f| {
            std::fs::metadata(dir.join(f.name))
                .map(|m| m.len())
                .unwrap_or(0)
        })
        .sum()
}

fn model_key(model_id: &str) -> String {
    local_asr_model::local_model_key(local_asr_model::LocalAsrEngine::Parakeet, model_id)
}

fn missing_status(model_id: &str) -> local_asr_model::LocalAsrModelStatus {
    local_asr_model::LocalAsrModelStatus {
        state: local_asr_model::LocalAsrModelState::Missing,
        engine: local_asr_model::LocalAsrEngine::Parakeet
            .as_str()
            .to_string(),
        model_id: model_key(model_id),
        progress: None,
        error: None,
        path: None,
    }
}

fn ready_status(model_id: &str, dir: &PathBuf) -> local_asr_model::LocalAsrModelStatus {
    local_asr_model::LocalAsrModelStatus {
        state: local_asr_model::LocalAsrModelState::Ready,
        engine: local_asr_model::LocalAsrEngine::Parakeet
            .as_str()
            .to_string(),
        model_id: model_key(model_id),
        progress: None,
        error: None,
        path: Some(dir.to_string_lossy().to_string()),
    }
}

fn error_status(model_id: &str, message: String) -> local_asr_model::LocalAsrModelStatus {
    local_asr_model::LocalAsrModelStatus {
        state: local_asr_model::LocalAsrModelState::Error,
        engine: local_asr_model::LocalAsrEngine::Parakeet
            .as_str()
            .to_string(),
        model_id: model_key(model_id),
        progress: None,
        error: Some(message),
        path: None,
    }
}

fn downloading_status(
    model_id: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) -> local_asr_model::LocalAsrModelStatus {
    local_asr_model::LocalAsrModelStatus {
        state: local_asr_model::LocalAsrModelState::Downloading,
        engine: local_asr_model::LocalAsrEngine::Parakeet
            .as_str()
            .to_string(),
        model_id: model_key(model_id),
        progress: Some(local_asr_model::LocalAsrDownloadProgress {
            downloaded_bytes,
            total_bytes,
        }),
        error: None,
        path: None,
    }
}

async fn update_status(
    app: &AppHandle,
    state: &AppState,
    status: local_asr_model::LocalAsrModelStatus,
) {
    {
        let mut mgr = state.parakeet_models.lock().await;
        mgr.status = status.clone();
    }
    local_asr_model::emit_local_status(app, &status);
}

async fn refresh_status(
    app: &AppHandle,
    state: &AppState,
    model_id: &str,
) -> local_asr_model::LocalAsrModelStatus {
    {
        let mgr = state.parakeet_models.lock().await;
        if mgr.status.state == local_asr_model::LocalAsrModelState::Downloading
            && local_asr_model::parse_local_model_key(&mgr.status.model_id)
                .map(|(_, id)| id == model_id)
                .unwrap_or(false)
        {
            return mgr.status.clone();
        }
    }

    let Some(dir) = model_dir_for_id(model_id) else {
        let status = error_status(model_id, "Unknown model".to_string());
        update_status(app, state, status.clone()).await;
        return status;
    };

    let status = if is_model_downloaded(model_id) {
        ready_status(model_id, &dir)
    } else {
        missing_status(model_id)
    };

    {
        let mut mgr = state.parakeet_models.lock().await;
        mgr.status = status.clone();
    }

    status
}

async fn is_current_download(state: &AppState, cancel_flag: &Arc<AtomicBool>) -> bool {
    let mgr = state.parakeet_models.lock().await;
    mgr.download_cancel
        .as_ref()
        .map(|f| Arc::ptr_eq(f, cancel_flag))
        .unwrap_or(false)
}

async fn clear_download_state_if_current(state: &AppState, cancel_flag: &Arc<AtomicBool>) {
    let mut mgr = state.parakeet_models.lock().await;
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

async fn download_model_files(
    app: &AppHandle,
    state: &AppState,
    model_id: &str,
    cancel_flag: Arc<AtomicBool>,
) -> Result<PathBuf, String> {
    let Some(dir) = model_dir_for_id(model_id) else {
        return Err("Unknown model".to_string());
    };
    let Some(files) = files_for_model_id(model_id) else {
        return Err("Unknown model".to_string());
    };

    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create model directory: {e}"))?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        // Total request timeout (including body). Keep high to avoid aborting slow links,
        // but still bounded so we don't hang forever on a stalled transfer.
        .timeout(Duration::from_secs(6 * 60 * 60))
        .build()
        .map_err(|e| format!("Failed to configure download client: {e}"))?;

    // Best-effort total size for progress UX.
    // If HEAD fails or doesn't provide a length, we still proceed with the download.
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

    let estimated_total = estimated_total_bytes_for_model(model_id).unwrap_or(0);
    let mut total_for_pct = match total_opt {
        Some(t) if t > 0 => t,
        _ => estimated_total,
    };
    // Ensure denom isn't < numerator (can happen if our estimate is low).
    if total_for_pct > 0 {
        total_for_pct = total_for_pct.max(1);
    }
    let total_opt_for_status = match total_opt {
        Some(t) if t > 0 => Some(t),
        _ if total_for_pct > 0 => Some(total_for_pct),
        other => other,
    };
    let mut downloaded_total: u64 = 0;
    let mut last_emit = Instant::now();

    for f in &files {
        if cancel_flag.load(Ordering::Relaxed) || !is_current_download(state, &cancel_flag).await {
            return Err("Download canceled".to_string());
        }

        let resp = client
            .get(&f.url)
            .send()
            .await
            .map_err(|e| format!("Failed to start download: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Download failed with status: {}", resp.status()));
        }

        let final_path = dir.join(f.name);
        let tmp_path = dir.join(format!("{}.partial", f.name));

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
                    let denom = total_for_pct.max(downloaded_total).max(1);
                    ((downloaded_total as f64 / denom as f64) * 100.0)
                        .round()
                        .clamp(0.0, 100.0) as u8
                } else {
                    0
                };

                local_asr_model::emit_local_progress(
                    app,
                    &local_asr_model::LocalAsrModelProgress {
                        model_id: model_key(model_id),
                        downloaded: downloaded_total,
                        total: total_for_pct,
                        progress: pct,
                    },
                );

                let st = downloading_status(model_id, downloaded_total, total_opt_for_status);
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

    // Best-effort: ensure UI sees completion before we emit the final Ready status.
    local_asr_model::emit_local_progress(
        app,
        &local_asr_model::LocalAsrModelProgress {
            model_id: model_key(model_id),
            downloaded: downloaded_total,
            total: total_for_pct.max(downloaded_total).max(1),
            progress: 100,
        },
    );
    let st = downloading_status(model_id, downloaded_total, total_opt_for_status);
    update_status(app, state, st).await;

    Ok(dir)
}

pub async fn parakeet_model_status(
    state: State<'_, AppState>,
    model_id: &str,
) -> Result<local_asr_model::LocalAsrModelStatus, String> {
    let model_id = resolve_model_id(model_id).ok_or_else(|| "Unknown model".to_string())?;

    // If we're actively downloading this model, return the live status.
    {
        let mgr = state.parakeet_models.lock().await;
        let match_id = local_asr_model::parse_local_model_key(&mgr.status.model_id)
            .map(|(_, id)| id == model_id)
            .unwrap_or(false);
        if match_id && mgr.status.state == local_asr_model::LocalAsrModelState::Downloading {
            return Ok(mgr.status.clone());
        }
    }

    let st = if is_model_downloaded(model_id) {
        ready_status(
            model_id,
            &model_dir_for_id(model_id).unwrap_or_else(|| models_dir()),
        )
    } else {
        missing_status(model_id)
    };
    {
        let mut mgr = state.parakeet_models.lock().await;
        mgr.status = st.clone();
    }
    Ok(st)
}

pub async fn download_parakeet_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: &str,
) -> Result<local_asr_model::LocalAsrModelStatus, String> {
    let model_id = resolve_model_id(model_id).ok_or_else(|| "Unknown model".to_string())?;
    let current = refresh_status(&app, state.inner(), model_id).await;
    if current.state == local_asr_model::LocalAsrModelState::Ready {
        return Ok(current);
    }
    if current.state == local_asr_model::LocalAsrModelState::Downloading {
        return Ok(current);
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut mgr = state.parakeet_models.lock().await;
        if mgr.status.state == local_asr_model::LocalAsrModelState::Downloading {
            if let Some(flag) = mgr.download_cancel.take() {
                flag.store(true, Ordering::SeqCst);
            }
            if let Some(task) = mgr.download_task.take() {
                task.abort();
            }
        }

        mgr.download_cancel = Some(cancel_flag.clone());
        mgr.status = downloading_status(model_id, 0, None);
    }

    local_asr_model::emit_local_status(&app, &refresh_status(&app, state.inner(), model_id).await);

    let app_handle = app.clone();
    let model_id_string = model_id.to_string();
    let cancel_flag_clone = cancel_flag.clone();
    let task = tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        let state_ref = state.inner();

        let result = download_model_files(
            &app_handle,
            state_ref,
            &model_id_string,
            cancel_flag_clone.clone(),
        )
        .await;
        match result {
            Ok(dir) => {
                if let Err(err) = local_asr_model::write_active_local_model(
                    local_asr_model::LocalAsrEngine::Parakeet,
                    &model_id_string,
                ) {
                    let status = error_status(&model_id_string, err);
                    update_status(&app_handle, state_ref, status).await;
                    clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
                    return;
                }

                if let Ok(mut mgr) = state_ref.meeting_manager.lock() {
                    let _ = mgr.reset_local_asr_model();
                }
                if let Ok(mut svc) = state_ref.dictation.lock() {
                    let _ = svc.reset_local_asr_model();
                }

                let status = ready_status(&model_id_string, &dir);
                update_status(&app_handle, state_ref, status).await;
                clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
            }
            Err(err) => {
                if err == "Download canceled" {
                    let status = missing_status(&model_id_string);
                    update_status(&app_handle, state_ref, status).await;
                    clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
                    return;
                }
                let status = error_status(&model_id_string, err);
                update_status(&app_handle, state_ref, status).await;
                clear_download_state_if_current(state_ref, &cancel_flag_clone).await;
            }
        }
    });

    {
        let mut mgr = state.parakeet_models.lock().await;
        mgr.download_task = Some(task);
    }

    Ok(refresh_status(&app, state.inner(), model_id).await)
}

pub async fn delete_parakeet_model(
    state: State<'_, AppState>,
    model_id: &str,
) -> Result<(), String> {
    let model_id = resolve_model_id(model_id).ok_or_else(|| "Unknown model".to_string())?;

    // Cancel if currently downloading.
    {
        let mut mgr = state.parakeet_models.lock().await;
        if mgr.status.state == local_asr_model::LocalAsrModelState::Downloading {
            if let Some(flag) = mgr.download_cancel.take() {
                flag.store(true, Ordering::Relaxed);
            }
            if let Some(task) = mgr.download_task.take() {
                task.abort();
            }
            mgr.status = missing_status(model_id);
        }
    }

    if let Some(dir) = model_dir_for_id(model_id) {
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir)
                .await
                .map_err(|e| format!("Failed to delete model: {e}"))?;
        }
    }

    // If this Parakeet model was active, fall back to Whisper (legacy active id).
    let active = local_asr_model::read_active_local_model();
    if active.engine == local_asr_model::LocalAsrEngine::Parakeet && active.model_id == model_id {
        let whisper_id = whisper_model::active_model_id();
        local_asr_model::write_active_local_model(
            local_asr_model::LocalAsrEngine::Whisper,
            &whisper_id,
        )?;
        if let Ok(mut mgr) = state.meeting_manager.lock() {
            let _ = mgr.reset_local_asr_model();
        }
        if let Ok(mut svc) = state.dictation.lock() {
            let _ = svc.reset_local_asr_model();
        }
    }

    Ok(())
}
