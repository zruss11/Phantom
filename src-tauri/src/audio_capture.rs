//! Real-time audio capture from the microphone (cross-platform via cpal)
//! and system audio (macOS only via ScreenCaptureKit), resampled to 16kHz
//! mono f32 PCM for Whisper inference.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{
    audioadapter::{Adapter, AdapterMut},
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configuration for an audio capture session.
pub struct AudioCaptureConfig {
    /// Whether to capture from the default microphone.
    pub capture_mic: bool,
    /// Whether to capture system audio (macOS only; ignored on other platforms).
    pub capture_system: bool,
    /// Duration of each audio chunk in seconds.
    /// At 16 kHz, 5.0 seconds produces 80 000 samples per chunk.
    pub chunk_duration_secs: f32,
}

/// A chunk of 16 kHz mono f32 PCM audio.
pub struct AudioChunk {
    /// Interleaved 16 kHz mono samples in the range -1.0..1.0.
    pub samples: Vec<f32>,
    /// Wall-clock offset in milliseconds from the start of capture, measured
    /// at the start of this chunk's audio window.
    pub timestamp_ms: u64,
}

/// Handle returned by [`start_capture`] that owns the capture threads and the
/// cpal stream.  Call [`stop`](AudioCaptureHandle::stop) to tear everything
/// down cleanly.
pub struct AudioCaptureHandle {
    stop_flag: Arc<AtomicBool>,
    mic_thread: Option<std::thread::JoinHandle<()>>,
    #[cfg(target_os = "macos")]
    system_thread: Option<std::thread::JoinHandle<()>>,
}

// ---------------------------------------------------------------------------
// Resampling helper
// ---------------------------------------------------------------------------

/// Resample a mono f32 buffer from `from_rate` Hz to 16 000 Hz.
///
/// If `from_rate` is already 16 000 the input is returned as-is (cloned).
/// Otherwise `rubato::SincFixedIn` is used for high-quality sinc
/// interpolation.
pub fn resample_to_16k(samples: &[f32], from_rate: u32) -> Vec<f32> {
    const TARGET_RATE: u32 = 16_000;

    if from_rate == TARGET_RATE || samples.is_empty() {
        return samples.to_vec();
    }

    let resample_ratio = TARGET_RATE as f64 / from_rate as f64;

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Linear,
        window: WindowFunction::BlackmanHarris2,
    };

    struct MonoAdapter<'a> {
        buf: &'a [f32],
    }

    impl<'a> Adapter<'a, f32> for MonoAdapter<'a> {
        unsafe fn read_sample_unchecked(&self, _channel: usize, frame: usize) -> f32 {
            *self.buf.get_unchecked(frame)
        }

        fn channels(&self) -> usize {
            1
        }

        fn frames(&self) -> usize {
            self.buf.len()
        }
    }

    struct MonoAdapterMut<'a> {
        buf: &'a mut [f32],
    }

    impl<'a> Adapter<'a, f32> for MonoAdapterMut<'a> {
        unsafe fn read_sample_unchecked(&self, _channel: usize, frame: usize) -> f32 {
            *self.buf.get_unchecked(frame)
        }

        fn channels(&self) -> usize {
            1
        }

        fn frames(&self) -> usize {
            self.buf.len()
        }
    }

    impl<'a> AdapterMut<'a, f32> for MonoAdapterMut<'a> {
        unsafe fn write_sample_unchecked(
            &mut self,
            _channel: usize,
            frame: usize,
            value: &f32,
        ) -> bool {
            *self.buf.get_unchecked_mut(frame) = *value;
            false
        }
    }

    // Use rubato's async sinc resampler and process the full clip into a buffer.
    let chunk_size = samples.len().max(1);
    let mut resampler = match Async::<f32>::new_sinc(
        resample_ratio,
        1.0, // max_resample_ratio_relative — no dynamic ratio changes
        &params,
        chunk_size,
        1, // mono
        FixedAsync::Input,
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to create resampler: {e}");
            return Vec::new();
        }
    };

    let out_len = resampler.process_all_needed_output_len(samples.len());
    let mut out = vec![0.0f32; out_len];
    let input = MonoAdapter { buf: samples };
    let mut output = MonoAdapterMut { buf: &mut out };

    match resampler.process_all_into_buffer(&input, &mut output, samples.len(), None) {
        Ok((_nbr_in, nbr_out)) => {
            out.truncate(nbr_out);
            out
        }
        Err(e) => {
            tracing::error!("Resampling failed: {e}");
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Capture entry-point
// ---------------------------------------------------------------------------

/// Start capturing audio according to `config`, sending 16 kHz mono chunks
/// over `sender`.
///
/// Returns a handle that must be [`stop`](AudioCaptureHandle::stop)ped when
/// capture should end.
pub fn start_capture(
    config: AudioCaptureConfig,
    sender: Sender<AudioChunk>,
) -> Result<AudioCaptureHandle, String> {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let start_time = Instant::now();

    // -- Mic capture ----------------------------------------------------------
    let mic_thread = if config.capture_mic {
        let stop = Arc::clone(&stop_flag);
        let tx = sender.clone();
        let chunk_dur = config.chunk_duration_secs;

        Some(
            std::thread::Builder::new()
                .name("audio-mic-capture".into())
                .spawn(move || {
                    if let Err(e) = run_mic_capture(stop, tx, chunk_dur, start_time) {
                        tracing::error!("Mic capture thread exited with error: {e}");
                    }
                })
                .map_err(|e| format!("Failed to spawn mic capture thread: {e}"))?,
        )
    } else {
        None
    };

    // -- System audio capture (macOS only) ------------------------------------
    #[cfg(target_os = "macos")]
    let system_thread = if config.capture_system {
        let stop = Arc::clone(&stop_flag);
        let tx = sender.clone();
        let chunk_dur = config.chunk_duration_secs;
        Some(
            std::thread::Builder::new()
                .name("audio-system-capture".into())
                .spawn(move || {
                    if let Err(e) = run_system_audio_capture(stop, tx, chunk_dur, start_time) {
                        tracing::error!("System audio capture thread exited with error: {e}");
                    }
                })
                .map_err(|e| format!("Failed to spawn system audio thread: {e}"))?,
        )
    } else {
        None
    };

    #[cfg(not(target_os = "macos"))]
    if config.capture_system {
        tracing::warn!(
            "System audio capture is only supported on macOS; ignoring capture_system flag"
        );
    }

    Ok(AudioCaptureHandle {
        stop_flag,
        mic_thread,
        #[cfg(target_os = "macos")]
        system_thread,
    })
}

// ---------------------------------------------------------------------------
// AudioCaptureHandle
// ---------------------------------------------------------------------------

impl AudioCaptureHandle {
    /// Signal all capture threads to stop and wait for them to exit.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);

        if let Some(handle) = self.mic_thread.take() {
            let _ = handle.join();
        }

        #[cfg(target_os = "macos")]
        if let Some(handle) = self.system_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AudioCaptureHandle {
    fn drop(&mut self) {
        // Ensure capture threads are stopped even if the caller forgets.
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Microphone capture internals
// ---------------------------------------------------------------------------

/// Run the mic capture loop on the current thread.  Returns when the stop
/// flag is set or an unrecoverable error occurs.
fn run_mic_capture(
    stop: Arc<AtomicBool>,
    sender: Sender<AudioChunk>,
    chunk_duration_secs: f32,
    start_time: Instant,
) -> Result<(), String> {
    let host = cpal::default_host();

    let device = host
        .default_input_device()
        .ok_or_else(|| "No default audio input device available".to_string())?;

    let supported_config = device
        .default_input_config()
        .map_err(|e| format!("Failed to get default input config: {e}"))?;

    let native_rate = supported_config.sample_rate();
    let native_channels = supported_config.channels() as usize;
    let sample_format = supported_config.sample_format();

    tracing::info!(
        "Mic capture: device default config — rate={native_rate}, channels={native_channels}, format={sample_format:?}"
    );

    // Number of *mono* samples at the native rate that constitute one chunk.
    let samples_per_chunk = (chunk_duration_secs * native_rate as f32) as usize;

    // Shared buffer between the cpal callback and the combiner loop.
    // The callback only ever calls `try_lock` so it will never block the
    // real-time audio thread.
    let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(samples_per_chunk)));

    let stream_config = supported_config.config();

    // Build the cpal input stream.  We match on the sample format so that we
    // can provide the correctly-typed callback to `build_input_stream`.
    let buf_cb = Arc::clone(&buffer);
    let ch = native_channels;

    let err_callback = |err: cpal::StreamError| {
        tracing::error!("cpal stream error: {err}");
    };

    // The stream must be kept alive for as long as we want audio.  We move it
    // into this scope and drop it at the end.
    let _stream: cpal::Stream = match sample_format {
        cpal::SampleFormat::F32 => build_input_stream_typed::<f32>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::I16 => build_input_stream_typed::<i16>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::U16 => build_input_stream_typed::<u16>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::I8 => build_input_stream_typed::<i8>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::I32 => build_input_stream_typed::<i32>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::I64 => build_input_stream_typed::<i64>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::U8 => build_input_stream_typed::<u8>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::U32 => build_input_stream_typed::<u32>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::U64 => build_input_stream_typed::<u64>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        cpal::SampleFormat::F64 => build_input_stream_typed::<f64>(
            &device,
            &stream_config,
            Arc::clone(&buf_cb),
            ch,
            err_callback,
        )?,
        format => {
            return Err(format!("Unsupported sample format: {format:?}"));
        }
    };

    tracing::info!("Mic capture: stream started");

    // Combiner loop — runs on this thread, polls the shared buffer every 100 ms.
    let poll_interval = std::time::Duration::from_millis(100);

    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(poll_interval);

        // Try to drain the buffer.  If the lock is contended (callback is
        // writing) we just skip this iteration — the samples stay in the
        // buffer and will be picked up next time.
        let drained = {
            let Ok(mut buf) = buffer.try_lock() else {
                continue;
            };
            if buf.len() < samples_per_chunk {
                continue;
            }
            // Drain exactly one chunk worth of mono samples.
            let chunk: Vec<f32> = buf.drain(..samples_per_chunk).collect();
            chunk
        };

        // `timestamp_ms` should represent the start of the chunk's audio window
        // (not when the combiner happens to drain it).
        let now_ms = start_time.elapsed().as_millis() as u64;
        let chunk_duration_ms = (chunk_duration_secs * 1000.0) as u64;
        let timestamp_ms = now_ms.saturating_sub(chunk_duration_ms);

        // Resample to 16 kHz.
        let resampled = resample_to_16k(&drained, native_rate);

        if sender
            .send(AudioChunk {
                samples: resampled,
                timestamp_ms,
            })
            .is_err()
        {
            // Receiver dropped — stop capturing.
            tracing::info!("Mic capture: receiver dropped, stopping");
            break;
        }
    }

    tracing::info!("Mic capture: stopping");
    Ok(())
}

/// Build a typed cpal input stream that converts incoming samples to f32 mono
/// and pushes them into the shared buffer using `try_lock` (non-blocking).
fn build_input_stream_typed<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    err_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32> + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                // Convert to f32 and down-mix to mono.
                // This runs on the real-time audio thread — we only use
                // try_lock to avoid blocking.
                let mono_samples: Vec<f32> = if channels == 1 {
                    data.iter()
                        .map(|&s| <f32 as cpal::FromSample<T>>::from_sample_(s))
                        .collect()
                } else {
                    data.chunks_exact(channels)
                        .map(|frame| {
                            let sum: f32 = frame
                                .iter()
                                .map(|&s| <f32 as cpal::FromSample<T>>::from_sample_(s))
                                .sum();
                            sum / channels as f32
                        })
                        .collect()
                };

                if let Ok(mut buf) = buffer.try_lock() {
                    buf.extend_from_slice(&mono_samples);
                }
                // If try_lock fails we silently drop this batch of samples.
                // This is preferable to blocking the audio thread.
            },
            err_callback,
            None, // no timeout
        )
        .map_err(|e| format!("Failed to build input stream: {e}"))?;

    stream
        .play()
        .map_err(|e| format!("Failed to start input stream: {e}"))?;

    Ok(stream)
}

// ---------------------------------------------------------------------------
// System audio capture (macOS only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn run_system_audio_capture(
    stop: Arc<AtomicBool>,
    sender: Sender<AudioChunk>,
    chunk_duration_secs: f32,
    start_time: Instant,
) -> Result<(), String> {
    use screencapturekit::{
        shareable_content::SCShareableContent,
        stream::{
            configuration::SCStreamConfiguration, content_filter::SCContentFilter,
            output_trait::SCStreamOutputTrait, output_type::SCStreamOutputType, SCStream,
        },
        CMSampleBuffer,
    };
    use std::sync::mpsc::channel;

    struct AudioStreamOutput {
        sender: Sender<CMSampleBuffer>,
    }

    impl SCStreamOutputTrait for AudioStreamOutput {
        fn did_output_sample_buffer(
            &self,
            sample_buffer: CMSampleBuffer,
            _of_type: SCStreamOutputType,
        ) {
            let _ = self.sender.send(sample_buffer);
        }
    }

    fn bytes_look_like_f32(samples: &[f32]) -> bool {
        if samples.is_empty() {
            return false;
        }
        let mut ok = 0usize;
        for s in samples.iter().take(16) {
            if s.is_finite() && s.abs() <= 2.0 {
                ok += 1;
            }
        }
        ok >= 10
    }

    fn decode_bytes_to_f32_mono(bytes: &[u8], channels: usize) -> Vec<f32> {
        if bytes.is_empty() || channels == 0 {
            return Vec::new();
        }

        // Try f32 first (common for ScreenCaptureKit). Fall back to i16 if it
        // doesn't look plausible.
        let mut decoded_f32: Vec<f32> = Vec::new();
        if bytes.len() % 4 == 0 {
            decoded_f32 = bytes
                .chunks_exact(4)
                .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
        }

        let decoded: Vec<f32> = if !decoded_f32.is_empty() && bytes_look_like_f32(&decoded_f32) {
            decoded_f32
        } else if bytes.len() % 2 == 0 {
            bytes
                .chunks_exact(2)
                .map(|c| i16::from_ne_bytes([c[0], c[1]]) as f32 / 32768.0)
                .collect()
        } else {
            Vec::new()
        };

        if decoded.is_empty() {
            return decoded;
        }

        if channels == 1 {
            return decoded;
        }

        // Downmix interleaved audio to mono.
        let frames = decoded.len() / channels;
        let mut mono = Vec::with_capacity(frames);
        for i in 0..frames {
            let mut sum = 0.0f32;
            for ch in 0..channels {
                sum += decoded[i * channels + ch];
            }
            mono.push(sum / channels as f32);
        }
        mono
    }

    fn sample_buffer_to_mono(sample: &CMSampleBuffer) -> Vec<f32> {
        let Some(list) = sample.audio_buffer_list() else {
            return Vec::new();
        };

        let num = list.num_buffers();
        if num == 0 {
            return Vec::new();
        }

        // One buffer with N channels => interleaved.
        if num == 1 {
            let b = list.get(0).unwrap();
            return decode_bytes_to_f32_mono(b.data(), b.number_channels as usize);
        }

        // Multiple buffers => treat each as a non-interleaved channel and average.
        let mut chans: Vec<Vec<f32>> = Vec::new();
        for b in &list {
            let mono = decode_bytes_to_f32_mono(b.data(), 1);
            if !mono.is_empty() {
                chans.push(mono);
            }
        }
        if chans.is_empty() {
            return Vec::new();
        }

        let min_len = chans.iter().map(|c| c.len()).min().unwrap_or(0);
        if min_len == 0 {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(min_len);
        for i in 0..min_len {
            let mut sum = 0.0f32;
            for c in &chans {
                sum += c[i];
            }
            out.push(sum / chans.len() as f32);
        }
        out
    }

    tracing::info!("System audio capture: initializing ScreenCaptureKit stream");

    let system_sample_rate: u32 = 48_000;
    let config = SCStreamConfiguration::new()
        .with_captures_audio(true)
        .with_excludes_current_process_audio(true)
        .with_channel_count(2)
        .with_sample_rate(system_sample_rate as i32);

    let mut displays = SCShareableContent::get()
        .map_err(|e| format!("Failed to get shareable content: {e:?}"))?
        .displays();

    let display = displays
        .pop()
        .ok_or_else(|| "No displays available for ScreenCaptureKit".to_string())?;

    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build();

    let (tx, rx) = channel::<CMSampleBuffer>();
    let mut stream = SCStream::new(&filter, &config);
    stream.add_output_handler(AudioStreamOutput { sender: tx }, SCStreamOutputType::Audio);

    stream
        .start_capture()
        .map_err(|e| format!("Failed to start system audio capture: {e:?}"))?;

    tracing::info!("System audio capture: stream started");

    let samples_per_chunk = (chunk_duration_secs * 16_000.0) as usize;
    let mut accum: Vec<f32> = Vec::with_capacity(samples_per_chunk * 2);

    while !stop.load(Ordering::SeqCst) {
        let sample = match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(s) => s,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };

        let mono = sample_buffer_to_mono(&sample);
        if mono.is_empty() {
            continue;
        }

        // Resample to 16 kHz for whisper.
        let resampled = resample_to_16k(&mono, system_sample_rate);
        accum.extend_from_slice(&resampled);

        while accum.len() >= samples_per_chunk {
            let chunk_samples: Vec<f32> = accum.drain(..samples_per_chunk).collect();
            // `timestamp_ms` should represent the start of the chunk's audio window.
            let now_ms = start_time.elapsed().as_millis() as u64;
            let chunk_duration_ms = (chunk_duration_secs * 1000.0) as u64;
            let timestamp_ms = now_ms.saturating_sub(chunk_duration_ms);
            if sender
                .send(AudioChunk {
                    samples: chunk_samples,
                    timestamp_ms,
                })
                .is_err()
            {
                tracing::info!("System audio capture: receiver dropped, stopping");
                break;
            }
        }
    }

    stream.stop_capture().ok();
    tracing::info!("System audio capture: stopping");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_passthrough_when_already_16k() {
        let input: Vec<f32> = (0..1600).map(|i| (i as f32 / 1600.0).sin()).collect();
        let output = resample_to_16k(&input, 16_000);
        assert_eq!(input.len(), output.len());
        assert_eq!(input, output);
    }

    #[test]
    fn test_resample_empty_input() {
        let output = resample_to_16k(&[], 48_000);
        assert!(output.is_empty());
    }

    #[test]
    fn test_resample_48k_to_16k_produces_shorter_output() {
        // 48000 samples at 48 kHz = 1 second of audio.
        // At 16 kHz that should be ~16000 samples.
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 / 48_000.0).sin()).collect();
        let output = resample_to_16k(&input, 48_000);
        // Allow some tolerance for filter delay / ramp-up.
        let expected = 16_000usize;
        let diff = (output.len() as isize - expected as isize).unsigned_abs();
        assert!(
            diff < 200,
            "Expected ~{expected} samples, got {} (diff {diff})",
            output.len()
        );
    }

    #[test]
    fn test_resample_44100_to_16k() {
        let input: Vec<f32> = (0..44_100).map(|i| (i as f32 / 44_100.0).sin()).collect();
        let output = resample_to_16k(&input, 44_100);
        let expected = 16_000usize;
        let diff = (output.len() as isize - expected as isize).unsigned_abs();
        assert!(
            diff < 200,
            "Expected ~{expected} samples, got {} (diff {diff})",
            output.len()
        );
    }

    #[test]
    fn test_audio_capture_config_defaults() {
        let config = AudioCaptureConfig {
            capture_mic: true,
            capture_system: false,
            chunk_duration_secs: 5.0,
        };
        assert!(config.capture_mic);
        assert!(!config.capture_system);
        assert!((config.chunk_duration_secs - 5.0).abs() < f32::EPSILON);
    }
}
