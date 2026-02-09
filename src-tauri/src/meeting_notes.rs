//! Meeting session manager for live audio transcription.
//!
//! Orchestrates audio capture via the `audio_capture` module and whisper-based
//! transcription.  Each meeting session is persisted in SQLite and transcription
//! segments are streamed to the frontend via Tauri events.

use crate::{audio_capture, db, local_asr_model, parakeet_model, whisper_model, AppState};

use rusqlite::Connection;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};
use transcribe_rs::engines::parakeet::{
    ParakeetEngine, ParakeetInferenceParams, ParakeetModelParams, TimestampGranularity,
};
use transcribe_rs::TranscriptionEngine;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

// ---------------------------------------------------------------------------
// Event payloads
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
struct TranscriptionStatus {
    recording: bool,
    paused: bool,
    session_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

pub struct MeetingSessionManager {
    active_session: Option<ActiveSession>,
    whisper_ctx: Option<Arc<WhisperContext>>,
    parakeet_engine: Option<Arc<StdMutex<ParakeetEngine>>>,
    parakeet_model_id: Option<String>,
}

struct StopTeardown {
    session_id: String,
    capture_handle: Option<audio_capture::AudioCaptureHandle>,
    inference_thread: Option<std::thread::JoinHandle<()>>,
}

struct ActiveSession {
    id: String,
    capture_handle: Option<audio_capture::AudioCaptureHandle>,
    inference_thread: Option<std::thread::JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    active_duration: Duration,
    active_segment_start: Option<Instant>,
}

// ---------------------------------------------------------------------------
// MeetingSessionManager implementation
// ---------------------------------------------------------------------------

impl MeetingSessionManager {
    /// Create a new manager with no active session and no loaded model.
    pub fn new() -> Self {
        Self {
            active_session: None,
            whisper_ctx: None,
            parakeet_engine: None,
            parakeet_model_id: None,
        }
    }

    /// Lazily load the whisper model if it has not been loaded yet.
    pub fn ensure_whisper_loaded(&mut self) -> Result<(), String> {
        if self.whisper_ctx.is_some() {
            return Ok(());
        }

        let active_id = whisper_model::active_model_id();
        if !whisper_model::is_model_downloaded(&active_id) {
            return Err(
                "Whisper model is not downloaded. Please download it first via Model settings."
                    .to_string(),
            );
        }

        let model_path = whisper_model::active_model_path();
        let path_str = model_path
            .to_str()
            .ok_or_else(|| "Model path contains invalid UTF-8".to_string())?;

        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| format!("Failed to load whisper model: {}", e))?;

        self.whisper_ctx = Some(Arc::new(ctx));
        Ok(())
    }

    fn ensure_parakeet_loaded(&mut self, model_id: &str) -> Result<(), String> {
        if self.parakeet_engine.is_some() && self.parakeet_model_id.as_deref() == Some(model_id) {
            return Ok(());
        }

        if !parakeet_model::is_model_downloaded(model_id) {
            return Err(
                "Parakeet model is not downloaded. Please download it first via Model settings."
                    .to_string(),
            );
        }

        let dir = parakeet_model::model_dir_for_id(model_id)
            .ok_or_else(|| "Unknown Parakeet model".to_string())?;

        let mut engine = ParakeetEngine::new();
        engine
            .load_model_with_params(&dir, ParakeetModelParams::int8())
            .map_err(|e| format!("Failed to load Parakeet model: {e}"))?;

        self.parakeet_engine = Some(Arc::new(StdMutex::new(engine)));
        self.parakeet_model_id = Some(model_id.to_string());
        Ok(())
    }

    fn ensure_active_local_asr_loaded(&mut self) -> Result<MeetingAsrEngine, String> {
        let active = local_asr_model::read_active_local_model();
        match active.engine {
            local_asr_model::LocalAsrEngine::Whisper => {
                self.ensure_whisper_loaded()?;
                let ctx = self
                    .whisper_ctx
                    .as_ref()
                    .ok_or_else(|| "Whisper context not loaded".to_string())?
                    .clone();
                Ok(MeetingAsrEngine::Whisper(ctx))
            }
            local_asr_model::LocalAsrEngine::Parakeet => {
                self.ensure_parakeet_loaded(&active.model_id)?;
                let eng = self
                    .parakeet_engine
                    .as_ref()
                    .ok_or_else(|| "Parakeet engine not loaded".to_string())?
                    .clone();
                Ok(MeetingAsrEngine::Parakeet(eng))
            }
        }
    }

    /// Clear any loaded local ASR engine state so the next session loads the
    /// current active local model from disk.
    pub fn reset_local_asr_model(&mut self) -> Result<(), String> {
        if self.active_session.is_some() {
            return Err("Cannot change models while a meeting is recording".to_string());
        }
        self.whisper_ctx = None;
        self.parakeet_engine = None;
        self.parakeet_model_id = None;
        Ok(())
    }

    /// Start a new meeting session with audio capture and live transcription.
    ///
    /// Returns the generated session ID on success.
    pub fn start(
        &mut self,
        db: Arc<StdMutex<Connection>>,
        app: AppHandle,
        title: Option<String>,
        capture_mic: bool,
        capture_system: bool,
    ) -> Result<String, String> {
        if self.active_session.is_some() {
            return Err("A meeting session is already active".to_string());
        }

        let asr_engine = self.ensure_active_local_asr_loaded()?;

        // Generate a unique session ID.
        let session_id = format!(
            "meeting-{}-{}",
            chrono::Utc::now().timestamp_millis(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        // Persist the new session in the database.
        let now = chrono::Utc::now().timestamp();
        let record = db::MeetingSessionRecord {
            id: session_id.clone(),
            title: title.clone(),
            status: "recording".to_string(),
            capture_mic,
            capture_system,
            started_at: Some(now),
            stopped_at: None,
            duration_ms: 0,
            created_at: now,
            updated_at: now,
        };

        {
            let db_guard = db.lock().map_err(|e| format!("DB lock error: {}", e))?;
            db::insert_meeting_session(&db_guard, &record)
                .map_err(|e| format!("Failed to insert meeting session: {}", e))?;
        }

        // Set up the audio chunk channel.
        let (sender, receiver) = mpsc::channel::<audio_capture::AudioChunk>();

        // Configure and start audio capture.
        let capture_config = audio_capture::AudioCaptureConfig {
            capture_mic,
            capture_system,
            chunk_duration_secs: 5.0,
        };
        let capture_handle = audio_capture::start_capture(capture_config, sender)
            .map_err(|e| format!("Failed to start audio capture: {}", e))?;

        // Shared flags for the inference thread.
        let stop_flag = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        // Clone values for the inference thread.
        let thread_engine = asr_engine.clone();
        let thread_stop = stop_flag.clone();
        let thread_paused = paused.clone();
        let thread_db = db.clone();
        let thread_app = app.clone();
        let thread_session_id = session_id.clone();

        let inference_thread = std::thread::spawn(move || {
            run_inference_loop(
                thread_engine,
                receiver,
                thread_stop,
                thread_paused,
                thread_db,
                thread_app,
                thread_session_id,
            );
        });

        self.active_session = Some(ActiveSession {
            id: session_id.clone(),
            capture_handle: Some(capture_handle),
            inference_thread: Some(inference_thread),
            stop_flag,
            paused,
            active_duration: Duration::from_secs(0),
            active_segment_start: Some(Instant::now()),
        });

        let _ = app.emit(
            "TranscriptionStatus",
            TranscriptionStatus {
                recording: true,
                paused: false,
                session_id: Some(session_id.clone()),
            },
        );

        Ok(session_id)
    }

    /// Pause the active meeting session.
    pub fn pause(&mut self, app: &AppHandle) -> Result<(), String> {
        let session = self
            .active_session
            .as_mut()
            .ok_or("No active meeting session to pause")?;

        session.paused.store(true, Ordering::Relaxed);
        if let Some(seg_start) = session.active_segment_start.take() {
            session.active_duration += seg_start.elapsed();
        }

        let _ = app.emit(
            "TranscriptionStatus",
            TranscriptionStatus {
                recording: true,
                paused: true,
                session_id: Some(session.id.clone()),
            },
        );

        Ok(())
    }

    /// Resume the active meeting session.
    pub fn resume(&mut self, app: &AppHandle) -> Result<(), String> {
        let session = self
            .active_session
            .as_mut()
            .ok_or("No active meeting session to resume")?;

        session.paused.store(false, Ordering::Relaxed);
        if session.active_segment_start.is_none() {
            session.active_segment_start = Some(Instant::now());
        }

        let _ = app.emit(
            "TranscriptionStatus",
            TranscriptionStatus {
                recording: true,
                paused: false,
                session_id: Some(session.id.clone()),
            },
        );

        Ok(())
    }

    /// Stop the active meeting session, finalize the DB record, and clean up.
    fn stop(
        &mut self,
        db: Arc<StdMutex<Connection>>,
        app: &AppHandle,
    ) -> Result<StopTeardown, String> {
        let mut session = self
            .active_session
            .take()
            .ok_or("No active meeting session to stop")?;

        let session_id = session.id.clone();

        // Signal the inference thread to stop.
        session.stop_flag.store(true, Ordering::Relaxed);

        // Extract blocking teardown so we can release the meeting_manager mutex before stopping
        // capture and joining the inference thread.
        let teardown = StopTeardown {
            session_id,
            capture_handle: session.capture_handle.take(),
            inference_thread: session.inference_thread.take(),
        };

        // Update the database record.
        let now = chrono::Utc::now().timestamp();
        let mut active = session.active_duration;
        if let Some(seg_start) = session.active_segment_start.take() {
            active += seg_start.elapsed();
        }
        let duration_ms = active.as_millis() as i64;

        {
            let db_guard = db.lock().map_err(|e| format!("DB lock error: {}", e))?;
            db::update_meeting_session_status(
                &db_guard,
                &session.id,
                "stopped",
                Some(now),
                Some(duration_ms),
            )
            .map_err(|e| format!("Failed to update meeting session: {}", e))?;
        }

        let _ = app.emit(
            "TranscriptionStatus",
            TranscriptionStatus {
                recording: false,
                paused: false,
                session_id: Some(session.id.clone()),
            },
        );

        Ok(teardown)
    }

    /// Return the current state of the meeting session manager as JSON.
    pub fn state(&self) -> serde_json::Value {
        match &self.active_session {
            Some(session) => {
                let paused = session.paused.load(Ordering::Relaxed);
                let mut active = session.active_duration;
                if let Some(seg_start) = session.active_segment_start {
                    active += seg_start.elapsed();
                }
                serde_json::json!({
                    "recording": true,
                    "paused": paused,
                    "session_id": session.id,
                    "elapsed_seconds": active.as_secs() as i64,
                })
            }
            None => serde_json::json!({
                "recording": false,
                "paused": false,
                "session_id": null,
                "elapsed_seconds": 0,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Whisper inference loop (runs on a dedicated thread)
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum MeetingAsrEngine {
    Whisper(Arc<WhisperContext>),
    Parakeet(Arc<StdMutex<ParakeetEngine>>),
}

fn run_inference_loop(
    engine: MeetingAsrEngine,
    receiver: mpsc::Receiver<audio_capture::AudioChunk>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    db: Arc<StdMutex<Connection>>,
    app: AppHandle,
    session_id: String,
) {
    loop {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(chunk) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if paused.load(Ordering::Relaxed) {
                    continue;
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                // Run inference on the received audio chunk.
                let result = match &engine {
                    MeetingAsrEngine::Whisper(ctx) => {
                        process_audio_chunk_whisper(ctx, &chunk, &db, &app, &session_id)
                    }
                    MeetingAsrEngine::Parakeet(eng) => {
                        process_audio_chunk_parakeet(eng, &chunk, &db, &app, &session_id)
                    }
                };
                if let Err(e) = result {
                    tracing::error!(session_id = %session_id, "Transcription error: {}", e);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Process a single audio chunk through whisper and persist any resulting
/// transcription segments.
fn process_audio_chunk_whisper(
    ctx: &WhisperContext,
    chunk: &audio_capture::AudioChunk,
    db: &Arc<StdMutex<Connection>>,
    app: &AppHandle,
    session_id: &str,
) -> Result<(), String> {
    let mut state = ctx
        .create_state()
        .map_err(|e| format!("Failed to create whisper state: {}", e))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state
        .full(params, &chunk.samples)
        .map_err(|e| format!("Whisper full() failed: {}", e))?;

    let num_segments = state.full_n_segments();
    let mut wrote_segment = false;

    for i in 0..num_segments {
        let segment = match state.get_segment(i) {
            Some(seg) => seg,
            None => continue,
        };
        let text = segment
            .to_str_lossy()
            .map_err(|e| format!("Failed to get segment text: {}", e))?
            .trim()
            .to_string();

        if text.is_empty() {
            continue;
        }

        // Whisper timestamps are in centiseconds; convert to milliseconds.
        let start = segment.start_timestamp() * 10;
        let end = segment.end_timestamp() * 10;

        // Adjust timestamps relative to the capture session start time
        // (`AudioChunk.timestamp_ms` is a wall-clock offset from session start).
        let abs_start = chunk.timestamp_ms as i64 + start;
        let abs_end = chunk.timestamp_ms as i64 + end;

        // Persist the segment.
        let seg_id = {
            let db_guard = db.lock().map_err(|e| format!("DB lock error: {}", e))?;
            db::save_meeting_segment(&db_guard, session_id, &text, abs_start, abs_end, None)
                .map_err(|e| format!("Failed to save meeting segment: {}", e))?
        };

        tracing::info!(
            session_id = %session_id,
            segment_id = seg_id,
            "Transcribed segment: {}",
            text
        );

        // Emit the segment to the frontend.
        let _ = app.emit(
            "TranscriptionSegment",
            serde_json::json!({
                "id": seg_id,
                "session_id": session_id,
                "text": text,
                "start_ms": abs_start,
                "end_ms": abs_end,
                // Frontend expects seconds for display.
                "timestamp": (abs_start as f64) / 1000.0,
            }),
        );

        wrote_segment = true;
    }

    if wrote_segment {
        let app_handle = app.clone();
        let sid = session_id.to_string();
        tauri::async_runtime::spawn(async move {
            crate::semantic_indexer::schedule_index_entity_with_delay(
                &app_handle,
                crate::semantic_search::ENTITY_TYPE_NOTE,
                &sid,
                Duration::from_secs(5),
            )
            .await;
        });
    }

    Ok(())
}

fn process_audio_chunk_parakeet(
    engine: &Arc<StdMutex<ParakeetEngine>>,
    chunk: &audio_capture::AudioChunk,
    db: &Arc<StdMutex<Connection>>,
    app: &AppHandle,
    session_id: &str,
) -> Result<(), String> {
    let mut eng = engine
        .lock()
        .map_err(|e| format!("Parakeet engine lock error: {e}"))?;

    let params = ParakeetInferenceParams {
        timestamp_granularity: TimestampGranularity::Segment,
        ..Default::default()
    };
    let result = eng
        .transcribe_samples(chunk.samples.clone(), Some(params))
        .map_err(|e| format!("Parakeet inference failed: {e}"))?;

    let Some(segments) = result.segments else {
        return Ok(());
    };

    let mut wrote_segment = false;
    for seg in segments {
        let text = seg.text.trim().to_string();
        if text.is_empty() {
            continue;
        }

        let start = (seg.start * 1000.0) as i64;
        let end = (seg.end * 1000.0) as i64;
        let abs_start = chunk.timestamp_ms as i64 + start;
        let abs_end = chunk.timestamp_ms as i64 + end;

        let seg_id = {
            let db_guard = db.lock().map_err(|e| format!("DB lock error: {}", e))?;
            db::save_meeting_segment(&db_guard, session_id, &text, abs_start, abs_end, None)
                .map_err(|e| format!("Failed to save meeting segment: {}", e))?
        };

        tracing::info!(
            session_id = %session_id,
            segment_id = seg_id,
            "Transcribed segment: {}",
            text
        );

        let _ = app.emit(
            "TranscriptionSegment",
            serde_json::json!({
                "id": seg_id,
                "session_id": session_id,
                "text": text,
                "start_ms": abs_start,
                "end_ms": abs_end,
                // Frontend expects seconds for display.
                "timestamp": (abs_start as f64) / 1000.0,
            }),
        );

        wrote_segment = true;
    }

    if wrote_segment {
        let app_handle = app.clone();
        let sid = session_id.to_string();
        tauri::async_runtime::spawn(async move {
            crate::semantic_indexer::schedule_index_entity_with_delay(
                &app_handle,
                crate::semantic_search::ENTITY_TYPE_NOTE,
                &sid,
                Duration::from_secs(5),
            )
            .await;
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn meeting_start(
    app: AppHandle,
    state: State<'_, AppState>,
    title: Option<String>,
    capture_mic: Option<bool>,
    capture_system: Option<bool>,
) -> Result<serde_json::Value, String> {
    let db = state.db.clone();
    let meeting_manager = state.meeting_manager.clone();
    let capture_mic = capture_mic.unwrap_or(true);
    let capture_system = capture_system.unwrap_or(false);

    let session_id = tauri::async_runtime::spawn_blocking(move || {
        let mut mgr = meeting_manager
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        mgr.start(db, app, title, capture_mic, capture_system)
    })
    .await
    .map_err(|e| format!("Meeting task join error: {}", e))??;
    Ok(serde_json::json!({ "session_id": session_id }))
}

#[tauri::command]
pub async fn meeting_pause(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut mgr = state
        .meeting_manager
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    mgr.pause(&app)
}

#[tauri::command]
pub async fn meeting_resume(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut mgr = state
        .meeting_manager
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    mgr.resume(&app)
}

#[tauri::command]
pub async fn meeting_stop(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.clone();
    let meeting_manager = state.meeting_manager.clone();
    let app_for_stop = app.clone();

    let session_id = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let teardown = {
            let mut mgr = meeting_manager
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            mgr.stop(db, &app_for_stop)?
        };

        // Blocking teardown outside the meeting_manager mutex.
        if let Some(mut handle) = teardown.capture_handle {
            handle.stop();
        }
        if let Some(thread) = teardown.inference_thread {
            let _ = thread.join();
        }

        Ok(teardown.session_id)
    })
    .await
    .map_err(|e| format!("Meeting task join error: {}", e))??;

    crate::semantic_indexer::schedule_index_entity(
        &app,
        crate::semantic_search::ENTITY_TYPE_NOTE,
        &session_id,
    )
    .await;

    Ok(())
}

#[tauri::command]
pub async fn meeting_update_title(
    state: State<'_, AppState>,
    session_id: String,
    title: Option<String>,
) -> Result<(), String> {
    let normalized = title
        .as_deref()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());

    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    db::update_meeting_session_title(&db, &session_id, normalized.as_deref())
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn meeting_state(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let mgr = state
        .meeting_manager
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    Ok(mgr.state())
}

#[derive(Serialize)]
pub(crate) struct MeetingSessionDto {
    id: String,
    title: Option<String>,
    status: String,
    capture_mic: bool,
    capture_system: bool,
    started_at: Option<i64>,
    stopped_at: Option<i64>,
    // Seconds for frontend formatting.
    duration: i64,
    // Epoch millis for frontend date grouping.
    created_at: i64,
    updated_at: i64,
}

#[tauri::command]
pub async fn meeting_list_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<MeetingSessionDto>, String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    let sessions = db::list_meeting_sessions(&db).map_err(|e| format!("DB error: {}", e))?;
    Ok(sessions
        .into_iter()
        .map(|s| MeetingSessionDto {
            id: s.id,
            title: s.title,
            status: s.status,
            capture_mic: s.capture_mic,
            capture_system: s.capture_system,
            started_at: s.started_at,
            stopped_at: s.stopped_at,
            duration: s.duration_ms / 1000,
            created_at: s.created_at * 1000,
            updated_at: s.updated_at * 1000,
        })
        .collect())
}

#[tauri::command]
pub async fn meeting_create_text_note(
    state: State<'_, AppState>,
    title: Option<String>,
    content: Option<String>,
) -> Result<MeetingSessionDto, String> {
    let normalized_title = title
        .as_deref()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());

    let session_id = format!(
        "note-{}-{}",
        chrono::Utc::now().timestamp_millis(),
        &uuid::Uuid::new_v4().to_string()[..8]
    );

    let now = chrono::Utc::now().timestamp();
    let record = db::MeetingSessionRecord {
        id: session_id.clone(),
        title: normalized_title.clone(),
        status: "text".to_string(),
        capture_mic: false,
        capture_system: false,
        started_at: None,
        stopped_at: None,
        duration_ms: 0,
        created_at: now,
        updated_at: now,
    };

    let normalized_content = content
        .as_deref()
        .map(|t| t.to_string())
        .unwrap_or_default();

    let db_guard = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    db::insert_meeting_session(&db_guard, &record).map_err(|e| format!("DB error: {}", e))?;

    if !normalized_content.trim().is_empty() {
        db::save_meeting_segment(&db_guard, &session_id, &normalized_content, 0, 0, None)
            .map_err(|e| format!("DB error: {}", e))?;
        db::touch_meeting_session_updated_at(&db_guard, &session_id)
            .map_err(|e| format!("DB error: {}", e))?;
    }

    Ok(MeetingSessionDto {
        id: record.id,
        title: record.title,
        status: record.status,
        capture_mic: record.capture_mic,
        capture_system: record.capture_system,
        started_at: record.started_at,
        stopped_at: record.stopped_at,
        duration: 0,
        created_at: record.created_at * 1000,
        updated_at: record.updated_at * 1000,
    })
}

#[tauri::command]
pub async fn meeting_update_text_note(
    state: State<'_, AppState>,
    session_id: String,
    content: Option<String>,
) -> Result<(), String> {
    let normalized_content = content.unwrap_or_default();

    let db_guard = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    let sess = db::get_meeting_session(&db_guard, &session_id)
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "Session not found".to_string())?;
    if sess.status != "text" {
        return Err("Not a text note session".to_string());
    }

    db::delete_meeting_segments_for_session(&db_guard, &session_id)
        .map_err(|e| format!("DB error: {}", e))?;
    if !normalized_content.trim().is_empty() {
        db::save_meeting_segment(&db_guard, &session_id, &normalized_content, 0, 0, None)
            .map_err(|e| format!("DB error: {}", e))?;
    }
    db::touch_meeting_session_updated_at(&db_guard, &session_id)
        .map_err(|e| format!("DB error: {}", e))?;

    Ok(())
}

#[derive(Serialize)]
pub(crate) struct MeetingSegmentDto {
    id: i64,
    session_id: String,
    text: String,
    start_ms: i64,
    end_ms: i64,
    speaker: Option<String>,
    timestamp: f64,
}

#[tauri::command]
pub async fn meeting_get_transcript(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    let segments =
        db::get_meeting_segments(&db, &session_id).map_err(|e| format!("DB error: {}", e))?;
    let dto: Vec<MeetingSegmentDto> = segments
        .into_iter()
        .map(|s| MeetingSegmentDto {
            id: s.id,
            session_id: s.session_id,
            text: s.text,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            speaker: s.speaker,
            timestamp: (s.start_ms as f64) / 1000.0,
        })
        .collect();
    Ok(serde_json::json!({ "segments": dto }))
}

#[tauri::command]
pub async fn meeting_delete_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    db::delete_meeting_session(&db, &session_id).map_err(|e| format!("DB error: {}", e))
}

#[tauri::command]
pub async fn meeting_export_transcript(
    state: State<'_, AppState>,
    session_id: String,
    format: String,
) -> Result<String, String> {
    let (session, segments) = {
        let db_guard = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
        let sess = db::get_meeting_session(&db_guard, &session_id)
            .map_err(|e| format!("DB error: {}", e))?
            .ok_or_else(|| "Session not found".to_string())?;
        let segs = db::get_meeting_segments(&db_guard, &session_id)
            .map_err(|e| format!("DB error: {}", e))?;
        (sess, segs)
    };

    if session.status == "text" {
        let content = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return match format.as_str() {
            "txt" => Ok(content),
            "md" => {
                let heading = session
                    .title
                    .clone()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| "Note".to_string());
                let mut out = format!("# {}\n\n", heading);
                if !content.trim().is_empty() {
                    out.push_str(&content);
                    out.push('\n');
                }
                Ok(out)
            }
            "json" => serde_json::to_string_pretty(&serde_json::json!({
                "id": session.id,
                "title": session.title,
                "status": session.status,
                "content": content,
                "created_at": session.created_at,
                "updated_at": session.updated_at,
            }))
            .map_err(|e| format!("JSON error: {}", e)),
            _ => Err(format!("Unsupported format: {}", format)),
        };
    }

    match format.as_str() {
        "txt" => Ok(segments
            .iter()
            .map(|s| {
                let mins = s.start_ms / 60000;
                let secs = (s.start_ms % 60000) / 1000;
                format!("[{:02}:{:02}] {}", mins, secs, s.text)
            })
            .collect::<Vec<_>>()
            .join("\n")),
        "md" => {
            let mut out = String::from("# Meeting Transcript\n\n");
            for s in &segments {
                let mins = s.start_ms / 60000;
                let secs = (s.start_ms % 60000) / 1000;
                out.push_str(&format!("**[{:02}:{:02}]** {}\n\n", mins, secs, s.text));
            }
            Ok(out)
        }
        "json" => serde_json::to_string_pretty(&segments).map_err(|e| format!("JSON error: {}", e)),
        _ => Err(format!("Unsupported format: {}", format)),
    }
}
