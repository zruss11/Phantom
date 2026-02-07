//! Global dictation / transcription integration.
//!
//! Goals:
//! - Start recording from a system-wide trigger (Fn double-press on macOS, optional global shortcut)
//! - Transcribe locally using the active Whisper model (same one used by Notes)
//! - Paste into the currently focused text field, or fall back to clipboard

use crate::{
    audio_capture, local_asr_model, parakeet_model, transcription, whisper_model, AppState,
    Settings,
};

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex as StdMutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use transcribe_rs::engines::parakeet::{ParakeetEngine, ParakeetModelParams};
use transcribe_rs::TranscriptionEngine;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri_plugin_global_shortcut::Shortcut as GlobalShortcut;

const STATUS_EVENT: &str = "DictationStatus";
const TRANSCRIPT_EVENT: &str = "DictationTranscript";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationState {
    Idle,
    Listening,
    Transcribing,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationOutcome {
    Pasted,
    CopiedToClipboard,
    ClipboardOnly,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictationStatusPayload {
    pub state: DictationState,
    pub last_transcript: Option<String>,
    pub last_outcome: Option<DictationOutcome>,
    pub error: Option<String>,
    // Permissions/health (best-effort)
    #[serde(rename = "accessibilityTrusted")]
    pub accessibility_trusted: Option<bool>,
    #[serde(rename = "fnListenerActive")]
    pub fn_listener_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictationTranscriptPayload {
    pub text: String,
    pub outcome: DictationOutcome,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ActivationMode {
    #[cfg(target_os = "macos")]
    FnHold,
    #[cfg(target_os = "macos")]
    FnDoublePress,
    GlobalShortcut(String),
}

impl ActivationMode {
    fn from_settings(settings: &Settings) -> Self {
        let activation = settings.notes_dictation_activation.as_deref().unwrap_or(
            if cfg!(target_os = "macos") {
                "fn_hold"
            } else {
                "global_shortcut"
            },
        );

        if cfg!(target_os = "macos") && activation == "fn_hold" {
            #[cfg(target_os = "macos")]
            {
                return ActivationMode::FnHold;
            }
        }

        if cfg!(target_os = "macos") && activation == "fn_double_press" {
            #[cfg(target_os = "macos")]
            {
                return ActivationMode::FnDoublePress;
            }
        }

        let shortcut = settings
            .notes_dictation_shortcut
            .clone()
            .unwrap_or_else(|| "Option+Space".to_string());
        ActivationMode::GlobalShortcut(shortcut)
    }
}

struct RecordingSession {
    stop: Arc<AtomicBool>,
    audio: Arc<StdMutex<Vec<f32>>>,
    capture: Option<audio_capture::AudioCaptureHandle>,
    collector: Option<std::thread::JoinHandle<()>>,
}

pub struct DictationManager {
    state: DictationState,
    last_transcript: Option<String>,
    last_outcome: Option<DictationOutcome>,
    last_error: Option<String>,

    whisper_ctx: Option<Arc<WhisperContext>>,
    whisper_ctx_model_id: Option<String>,
    parakeet_engine: Option<Arc<StdMutex<ParakeetEngine>>>,
    parakeet_model_id: Option<String>,

    recording: Option<RecordingSession>,
}

impl DictationManager {
    pub fn new() -> Self {
        Self {
            state: DictationState::Idle,
            last_transcript: None,
            last_outcome: None,
            last_error: None,
            whisper_ctx: None,
            whisper_ctx_model_id: None,
            parakeet_engine: None,
            parakeet_model_id: None,
            recording: None,
        }
    }

    pub fn reset_local_asr_model(&mut self) -> Result<(), String> {
        if matches!(
            self.state,
            DictationState::Listening | DictationState::Transcribing
        ) {
            return Err("Cannot change models while dictation is active".to_string());
        }
        self.whisper_ctx = None;
        self.whisper_ctx_model_id = None;
        self.parakeet_engine = None;
        self.parakeet_model_id = None;
        Ok(())
    }

    fn ensure_whisper_loaded(&mut self) -> Result<(), String> {
        let active_id = whisper_model::active_model_id();

        // If we already loaded a context for the current active model, reuse it.
        if self.whisper_ctx.is_some() && self.whisper_ctx_model_id.as_deref() == Some(&active_id) {
            return Ok(());
        }

        if !whisper_model::is_model_downloaded(&active_id) {
            return Err(
                "Whisper model is not downloaded. Download it first in Notes Settings > Models."
                    .to_string(),
            );
        }

        let model_path = whisper_model::active_model_path();
        let path_str = model_path
            .to_str()
            .ok_or_else(|| "Whisper model path contains invalid UTF-8".to_string())?;

        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| format!("Failed to load Whisper model: {e}"))?;

        self.whisper_ctx = Some(Arc::new(ctx));
        self.whisper_ctx_model_id = Some(active_id);
        Ok(())
    }

    fn ensure_parakeet_loaded(&mut self, model_id: &str) -> Result<(), String> {
        if self.parakeet_engine.is_some() && self.parakeet_model_id.as_deref() == Some(model_id) {
            return Ok(());
        }

        if !parakeet_model::is_model_downloaded(model_id) {
            return Err(
                "Parakeet model is not downloaded. Download it first in Notes Settings > Models."
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

    fn ensure_active_local_asr_loaded(&mut self) -> Result<DictationLocalAsrHandle, String> {
        let active = local_asr_model::read_active_local_model();
        match active.engine {
            local_asr_model::LocalAsrEngine::Whisper => {
                self.ensure_whisper_loaded()?;
                let ctx = self
                    .whisper_ctx
                    .as_ref()
                    .ok_or_else(|| "Whisper context missing".to_string())?
                    .clone();
                Ok(DictationLocalAsrHandle::Whisper(ctx))
            }
            local_asr_model::LocalAsrEngine::Parakeet => {
                self.ensure_parakeet_loaded(&active.model_id)?;
                let eng = self
                    .parakeet_engine
                    .as_ref()
                    .ok_or_else(|| "Parakeet engine missing".to_string())?
                    .clone();
                Ok(DictationLocalAsrHandle::Parakeet(eng))
            }
        }
    }

    fn emit_status<R: tauri::Runtime>(&self, app: &AppHandle<R>, fn_listener_active: Option<bool>) {
        let accessibility_trusted = {
            #[cfg(target_os = "macos")]
            {
                Some(macos::accessibility_trusted(false))
            }
            #[cfg(not(target_os = "macos"))]
            {
                None
            }
        };

        let payload = DictationStatusPayload {
            state: self.state.clone(),
            last_transcript: self.last_transcript.clone(),
            last_outcome: self.last_outcome.clone(),
            error: self.last_error.clone(),
            accessibility_trusted,
            fn_listener_active,
        };
        let _ = app.emit(STATUS_EVENT, payload);
    }

    pub fn status_payload(&self, fn_listener_active: Option<bool>) -> DictationStatusPayload {
        let accessibility_trusted = {
            #[cfg(target_os = "macos")]
            {
                Some(macos::accessibility_trusted(false))
            }
            #[cfg(not(target_os = "macos"))]
            {
                None
            }
        };

        DictationStatusPayload {
            state: self.state.clone(),
            last_transcript: self.last_transcript.clone(),
            last_outcome: self.last_outcome.clone(),
            error: self.last_error.clone(),
            accessibility_trusted,
            fn_listener_active,
        }
    }

    pub fn start_recording<R: tauri::Runtime>(&mut self, app: &AppHandle<R>) -> Result<(), String> {
        if !matches!(self.state, DictationState::Idle | DictationState::Error) {
            return Err("Dictation is already active".to_string());
        }

        // Clear previous error state on successful start.
        self.last_error = None;

        let (tx, rx) = mpsc::channel::<audio_capture::AudioChunk>();
        let stop = Arc::new(AtomicBool::new(false));
        let audio: Arc<StdMutex<Vec<f32>>> = Arc::new(StdMutex::new(Vec::new()));

        let capture = audio_capture::start_capture(
            audio_capture::AudioCaptureConfig {
                capture_mic: true,
                capture_system: false,
                chunk_duration_secs: 0.20, // low latency, acceptable overhead for dictation
            },
            tx,
        )?;

        let thread_stop = Arc::clone(&stop);
        let thread_audio = Arc::clone(&audio);
        let collector = std::thread::Builder::new()
            .name("dictation-audio-collector".into())
            .spawn(move || {
                // Note: we accept that the capture thread drains in chunk boundaries;
                // dictation tail loss should be small with 200ms chunks.
                while !thread_stop.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(chunk) => {
                            if let Ok(mut buf) = thread_audio.lock() {
                                buf.extend_from_slice(&chunk.samples);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .map_err(|e| format!("Failed to start dictation collector thread: {e}"))?;

        self.recording = Some(RecordingSession {
            stop,
            audio,
            capture: Some(capture),
            collector: Some(collector),
        });

        self.state = DictationState::Listening;
        self.emit_status(app, None);
        Ok(())
    }

    // Note: stopping dictation is initiated by DictationService::stop, which
    // moves the capture teardown + transcription into a background thread so we
    // don't block while holding the dictation service mutex.
}

fn transcribe_whisper(ctx: &WhisperContext, samples_16k_mono: &[f32]) -> Result<String, String> {
    let mut state = ctx
        .create_state()
        .map_err(|e| format!("Failed to create Whisper state: {e}"))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // Let Whisper auto-detect language.
    params.set_language(None);

    state
        .full(params, samples_16k_mono)
        .map_err(|e| format!("Whisper inference failed: {e}"))?;

    let mut out = String::new();
    let n = state.full_n_segments();
    for i in 0..n {
        let Some(seg) = state.get_segment(i) else {
            continue;
        };
        let s = seg
            .to_str_lossy()
            .map_err(|e| format!("Failed to read Whisper segment: {e}"))?
            .trim()
            .to_string();
        if s.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&s);
    }

    Ok(out.trim().to_string())
}

fn transcribe_parakeet(
    engine: &Arc<StdMutex<ParakeetEngine>>,
    samples_16k_mono: &[f32],
) -> Result<String, String> {
    let mut eng = engine
        .lock()
        .map_err(|e| format!("Parakeet engine lock error: {e}"))?;
    let result = eng
        .transcribe_samples(samples_16k_mono.to_vec(), None)
        .map_err(|e| format!("Parakeet inference failed: {e}"))?;
    Ok(result.text.trim().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DictationEngine {
    Local,
    ChatGpt,
}

#[derive(Clone)]
enum DictationLocalAsrHandle {
    Whisper(Arc<WhisperContext>),
    Parakeet(Arc<StdMutex<ParakeetEngine>>),
}

fn dictation_engine_from_settings(settings: &Settings) -> DictationEngine {
    match settings
        .notes_dictation_engine
        .as_deref()
        .unwrap_or("local")
    {
        "chatgpt" => DictationEngine::ChatGpt,
        // Back-compat: older builds saved "local_whisper".
        "local_whisper" | "local" => DictationEngine::Local,
        _ => DictationEngine::Local,
    }
}

fn pcm_f32_to_wav_i16_le(samples_16k_mono: &[f32], sample_rate: u32) -> Vec<u8> {
    // Minimal WAV writer (PCM 16-bit little-endian, mono).
    let n_samples = samples_16k_mono.len() as u32;
    let bytes_per_sample = 2u32;
    let num_channels = 1u16;
    let bits_per_sample = 16u16;
    let block_align = num_channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * (block_align as u32);
    let data_len = n_samples * bytes_per_sample;
    let riff_len = 4 + (8 + 16) + (8 + data_len);

    let mut out = Vec::with_capacity((8 + riff_len) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(riff_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // fmt chunk
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&(16u32).to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&(1u16).to_le_bytes()); // audio format = PCM
    out.extend_from_slice(&num_channels.to_le_bytes());
    out.extend_from_slice(&(sample_rate as u32).to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());

    for &s in samples_16k_mono {
        let clamped = s.clamp(-1.0, 1.0);
        let v = (clamped * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }

    out
}

fn insert_transcript<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    settings: &Settings,
    transcript: &str,
) -> Result<DictationOutcome, String> {
    let paste_into_inputs = settings.notes_dictation_paste_into_inputs.unwrap_or(true);
    let clipboard_fallback = settings.notes_dictation_clipboard_fallback.unwrap_or(true);
    let restore_clipboard = settings.notes_dictation_restore_clipboard.unwrap_or(true);
    let flatten_single_line = settings
        .notes_dictation_flatten_newlines_in_single_line
        .unwrap_or(true);

    #[cfg(target_os = "macos")]
    {
        let info = macos::focused_textbox_info().unwrap_or(None);
        if paste_into_inputs {
            if let Some(info) = info {
                if info.editable {
                    let mut text = transcript.to_string();
                    if flatten_single_line && info.single_line {
                        text = text.replace('\n', " ").replace('\r', " ");
                        text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    }

                    // If we are not accessibility-trusted, we can only copy to clipboard.
                    if !macos::accessibility_trusted(true) {
                        macos::clipboard_set_text(&text)?;
                        return Ok(DictationOutcome::CopiedToClipboard);
                    }

                    macos::paste_via_clipboard(&text, restore_clipboard)?;
                    return Ok(DictationOutcome::Pasted);
                }
            }
        }

        if clipboard_fallback {
            macos::clipboard_set_text(transcript)?;
            return Ok(DictationOutcome::CopiedToClipboard);
        }
        return Ok(DictationOutcome::Failed);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        let _ = paste_into_inputs;
        let _ = clipboard_fallback;
        let _ = restore_clipboard;
        let _ = flatten_single_line;
        Err("Dictation insertion is currently only supported on macOS".to_string())
    }
}

// =============================================================================
// Service wrapper (activation, wiring)
// =============================================================================

pub struct DictationService {
    mgr: DictationManager,
    enabled: bool,
    activation: ActivationMode,
    #[cfg(target_os = "macos")]
    fn_listener: Option<macos::FnKeyListener>,
    fn_listener_active: bool,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    global_shortcut: Option<GlobalShortcut>,
}

impl DictationService {
    pub fn new(settings: &Settings) -> Self {
        let enabled = settings.notes_dictation_enabled.unwrap_or(true);
        let activation = ActivationMode::from_settings(settings);
        Self {
            mgr: DictationManager::new(),
            enabled,
            activation,
            #[cfg(target_os = "macos")]
            fn_listener: None,
            fn_listener_active: false,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            global_shortcut: None,
        }
    }

    pub fn configure<R: tauri::Runtime>(
        &mut self,
        app: &AppHandle<R>,
        settings: &Settings,
    ) -> Result<(), String> {
        self.enabled = settings.notes_dictation_enabled.unwrap_or(true);
        self.activation = ActivationMode::from_settings(settings);

        // Fn listener (macOS).
        #[cfg(target_os = "macos")]
        {
            self.fn_listener_active = false;
            let want_fn = matches!(
                self.activation,
                ActivationMode::FnHold | ActivationMode::FnDoublePress
            ) && self.enabled;
            if want_fn {
                let window_ms = settings.notes_dictation_fn_window_ms.unwrap_or(350);
                let mode = match self.activation {
                    ActivationMode::FnHold => macos::FnKeyMode::HoldToTalk { hold_ms: window_ms },
                    ActivationMode::FnDoublePress => macos::FnKeyMode::DoublePress {
                        window_ms: window_ms,
                    },
                    _ => macos::FnKeyMode::HoldToTalk { hold_ms: window_ms },
                };

                // Recreate when mode changes to keep behavior consistent.
                let needs_restart = self
                    .fn_listener
                    .as_ref()
                    .map(|l| l.mode() != mode)
                    .unwrap_or(true);

                if needs_restart {
                    if let Some(l) = self.fn_listener.take() {
                        l.stop();
                    }
                    self.fn_listener = Some(macos::FnKeyListener::start(app.clone(), mode)?);
                }
                if let Some(l) = &self.fn_listener {
                    self.fn_listener_active = l.is_active();
                }
            } else {
                if let Some(l) = self.fn_listener.take() {
                    l.stop();
                }
            }
        }

        // Global shortcut registration is handled in main.rs setup where we can access
        // app.global_shortcut(). Here we only cache what we expect to match.
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            self.global_shortcut = None;
            if let ActivationMode::GlobalShortcut(s) = &self.activation {
                // Parse is platform-aware (Option/Alt/Cmd etc).
                if let Ok(hk) = s.parse::<GlobalShortcut>() {
                    self.global_shortcut = Some(hk);
                }
            }
        }

        self.mgr.emit_status(app, Some(self.fn_listener_active));
        Ok(())
    }

    pub fn reset_local_asr_model(&mut self) -> Result<(), String> {
        self.mgr.reset_local_asr_model()
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn wants_global_shortcut(&self) -> Option<GlobalShortcut> {
        match &self.activation {
            ActivationMode::GlobalShortcut(_) if self.enabled => self.global_shortcut,
            _ => None,
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn handle_global_shortcut<R: tauri::Runtime>(
        &mut self,
        app: &AppHandle<R>,
        shortcut: &GlobalShortcut,
        settings: &Settings,
    ) -> bool {
        let Some(expected) = self.wants_global_shortcut() else {
            return false;
        };
        if expected.id() != shortcut.id() {
            return false;
        }

        // Toggle start/stop for non-Fn shortcut (simple + predictable).
        match self.mgr.state {
            DictationState::Idle | DictationState::Error => {
                let _ = self.mgr.start_recording(app);
            }
            DictationState::Listening => {
                let _ = self.stop(app, settings);
            }
            DictationState::Transcribing => {}
        }
        true
    }

    pub fn start<R: tauri::Runtime>(&mut self, app: &AppHandle<R>) -> Result<(), String> {
        if !self.enabled {
            return Err("Dictation is disabled in Notes Settings".to_string());
        }
        match self.mgr.start_recording(app) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.mgr.state = DictationState::Error;
                self.mgr.last_error = Some(e.clone());
                self.mgr.emit_status(app, Some(self.fn_listener_active));
                Err(e)
            }
        }
    }

    pub fn stop<R: tauri::Runtime>(
        &mut self,
        app: &AppHandle<R>,
        settings: &Settings,
    ) -> Result<(), String> {
        if !matches!(self.mgr.state, DictationState::Listening) {
            return Err("No active dictation to stop".to_string());
        }

        // Take the recording session and mark it stopped. We do the blocking
        // teardown (sleep/join) in a background thread so the dictation service
        // mutex isn't held during that work.
        let mut rec = self
            .mgr
            .recording
            .take()
            .ok_or("Internal error: missing recording")?;
        rec.stop.store(true, Ordering::Relaxed);

        // Clear any prior error and flip to transcribing immediately so the UI
        // isn't stuck in Listening while we finalize capture.
        self.mgr.last_error = None;
        self.mgr.state = DictationState::Transcribing;
        self.mgr.emit_status(app, Some(self.fn_listener_active));

        let engine = dictation_engine_from_settings(settings);

        let local_asr = if engine == DictationEngine::Local {
            match self.mgr.ensure_active_local_asr_loaded() {
                Ok(h) => Some(h),
                Err(e) => {
                    self.mgr.state = DictationState::Error;
                    self.mgr.last_error = Some(e.clone());
                    self.mgr.emit_status(app, Some(self.fn_listener_active));
                    return Err(e);
                }
            }
        } else {
            None
        };

        let app_handle = app.clone();
        let settings = settings.clone();

        std::thread::Builder::new()
            .name("dictation-transcribe".into())
            .spawn(move || {
                // Give the capture combiner loop a beat to flush a final chunk.
                std::thread::sleep(Duration::from_millis(120));

                if let Some(mut capture) = rec.capture.take() {
                    capture.stop();
                }
                if let Some(handle) = rec.collector.take() {
                    let _ = handle.join();
                }

                let audio = match rec.audio.lock() {
                    Ok(g) => g.clone(),
                    Err(e) => {
                        tracing::error!("Dictation audio mutex poisoned: {e}");
                        e.into_inner().clone()
                    }
                };

                if audio.len() < 16_000 / 5 {
                    // Less than ~200ms audio after resampling -> treat as no-op.
                    let state = app_handle.state::<AppState>().inner().clone();
                    let mut svc = match state.dictation.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            tracing::error!("Dictation service mutex poisoned while stopping: {e}");
                            e.into_inner()
                        }
                    };
                    svc.mgr.state = DictationState::Idle;
                    svc.mgr.last_error = Some("No audio captured".to_string());
                    svc.mgr.emit_status(&app_handle, Some(svc.fn_listener_active));
                    return;
                }

                let transcript = match engine {
                    DictationEngine::Local => match local_asr {
                        Some(DictationLocalAsrHandle::Whisper(ctx)) => {
                            transcribe_whisper(&ctx, &audio)
                        }
                        Some(DictationLocalAsrHandle::Parakeet(eng)) => {
                            transcribe_parakeet(&eng, &audio)
                        }
                        None => Err("Local ASR handle missing".to_string()),
                    },
                    DictationEngine::ChatGpt => {
                        let wav = pcm_f32_to_wav_i16_le(&audio, 16_000);
                        let result = tauri::async_runtime::block_on(async {
                            let (token, account_id) = transcription::get_codex_auth()?;
                            let account_id =
                                account_id.ok_or("Codex account_id required for transcription")?;
                            let fut = transcription::transcribe_bytes(
                                &wav,
                                "dictation.wav",
                                "audio/wav",
                                None,
                                &token,
                                &account_id,
                            );
                            match tokio::time::timeout(Duration::from_secs(60), fut).await {
                                Ok(r) => r,
                                Err(_) => Err("Dictation transcription timed out".to_string()),
                            }
                        });
                        result
                    }
                };

                let transcript = match transcript {
                    Ok(t) => t,
                    Err(e) => {
                        let state = app_handle.state::<AppState>().inner().clone();
                        let mut svc = match state.dictation.lock() {
                            Ok(g) => g,
                            Err(poisoned) => {
                                tracing::error!(
                                    "Dictation service mutex poisoned during error update: {poisoned}"
                                );
                                poisoned.into_inner()
                            }
                        };
                        svc.mgr.state = DictationState::Error;
                        svc.mgr.last_error = Some(e.clone());
                        svc.mgr.emit_status(&app_handle, Some(svc.fn_listener_active));
                        return;
                    }
                };

                let enabled = settings.notes_dictation_enabled.unwrap_or(true);
                let outcome = if !enabled {
                    #[cfg(target_os = "macos")]
                    {
                        match macos::clipboard_set_text(&transcript) {
                            Ok(()) => Ok(DictationOutcome::ClipboardOnly),
                            Err(e) => Err(e),
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        Ok(DictationOutcome::ClipboardOnly)
                    }
                } else {
                    insert_transcript(&app_handle, &settings, &transcript)
                };

                // Emit transcript payload.
                match &outcome {
                    Ok(o) => {
                        let _ = app_handle.emit(
                            TRANSCRIPT_EVENT,
                            DictationTranscriptPayload {
                                text: transcript.clone(),
                                outcome: o.clone(),
                                error: None,
                            },
                        );
                    }
                    Err(e) => {
                        let _ = app_handle.emit(
                            TRANSCRIPT_EVENT,
                            DictationTranscriptPayload {
                                text: transcript.clone(),
                                outcome: DictationOutcome::Failed,
                                error: Some(e.clone()),
                            },
                        );
                    }
                }

                // Persist state for UI reads.
                let state = app_handle.state::<AppState>().inner().clone();
                let dictation_mutex = state.dictation.clone();
                let lock = dictation_mutex.lock();
                let mut svc = match lock {
                    Ok(g) => g,
                    Err(poisoned) => {
                        tracing::error!(
                            "Dictation service mutex poisoned during final state update: {poisoned}"
                        );
                        poisoned.into_inner()
                    }
                };
                svc.mgr.state = DictationState::Idle;
                svc.mgr.last_transcript = Some(transcript);
                svc.mgr.last_outcome = Some(outcome.clone().unwrap_or(DictationOutcome::Failed));
                svc.mgr.last_error = outcome.err();
                svc.mgr.emit_status(&app_handle, Some(svc.fn_listener_active));
            })
            .map_err(|e| format!("Failed to spawn dictation transcription: {e}"))?;

        Ok(())
    }

    pub fn status(&self) -> DictationStatusPayload {
        self.mgr.status_payload(Some(self.fn_listener_active))
    }
}

// =============================================================================
// Tauri commands
// =============================================================================

#[tauri::command]
pub fn dictation_get_status(
    state: tauri::State<'_, AppState>,
) -> Result<DictationStatusPayload, String> {
    let svc = state
        .dictation
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    Ok(svc.status())
}

#[tauri::command]
pub fn dictation_start(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut svc = state
        .dictation
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    svc.start(&app)
}

#[tauri::command]
pub fn dictation_stop(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings.blocking_lock().clone();
    let mut svc = state
        .dictation
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    svc.stop(&app, &settings)
}

// =============================================================================
// App-level helpers
// =============================================================================

/// Global shortcut hook from the plugin handler.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn handle_global_shortcut(app: &AppHandle, shortcut: &GlobalShortcut) -> bool {
    let state = app.state::<AppState>().inner().clone();
    let settings = state.settings.blocking_lock().clone();
    let mut svc = match state.dictation.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };
    svc.handle_global_shortcut(app, shortcut, &settings)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn handle_global_shortcut(app: &AppHandle, _shortcut: &()) -> bool {
    let _ = app;
    false
}
