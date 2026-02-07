//! macOS implementation details for dictation:
//! - Accessibility focused element probing (best-effort)
//! - Clipboard set and paste via synthetic Cmd+V
//! - Fn double-press listener using a CGEvent tap

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use libc::c_void;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime};

use crate::AppState;

#[derive(Debug, Clone)]
pub struct FocusedTextboxInfo {
    pub editable: bool,
    pub single_line: bool,
}

pub fn clipboard_set_text(text: &str) -> Result<(), String> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn pbcopy: {e}"))?;
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().ok_or("Failed to open pbcopy stdin")?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("Failed to write to pbcopy: {e}"))?;
    }
    let status = child
        .wait()
        .map_err(|e| format!("pbcopy wait failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("pbcopy failed".to_string())
    }
}

fn clipboard_get_text() -> Result<String, String> {
    let out = Command::new("pbpaste")
        .output()
        .map_err(|e| format!("Failed to run pbpaste: {e}"))?;
    // pbpaste returns non-zero when clipboard has no compatible text type. Treat that as empty.
    if !out.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn paste_via_clipboard(text: &str, restore_clipboard: bool) -> Result<(), String> {
    const DEFAULT_CLIPBOARD_RESTORE_DELAY_MS: u64 = 250;

    let prev = if restore_clipboard {
        clipboard_get_text().ok()
    } else {
        None
    };

    clipboard_set_text(text)?;

    // Paste using synthetic keystroke. Requires Accessibility permission.
    simulate_cmd_v()?;

    if let (true, Some(prev)) = (restore_clipboard, prev) {
        // Small delay to let the target app read clipboard.
        std::thread::sleep(Duration::from_millis(DEFAULT_CLIPBOARD_RESTORE_DELAY_MS));
        let _ = clipboard_set_text(&prev);
    }

    Ok(())
}

pub fn accessibility_trusted(prompt: bool) -> bool {
    unsafe {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::from(prompt);
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef())
    }
}

pub fn focused_textbox_info() -> Result<Option<FocusedTextboxInfo>, String> {
    unsafe {
        let sys = AXUIElementCreateSystemWide();

        let focused_attr = CFString::new("AXFocusedUIElement");
        let mut value: CFTypeRef = std::ptr::null_mut();
        let res = AXUIElementCopyAttributeValue(
            sys,
            focused_attr.as_concrete_TypeRef(),
            &mut value as *mut CFTypeRef,
        );
        if res != 0 || value.is_null() {
            CFRelease(sys as *const _);
            return Ok(None);
        }

        let elem = value as AXUIElementRef;

        // Determine editability via role + editable attribute when available.
        let role = ax_string_attr(elem, "AXRole").unwrap_or_default();
        let subrole = ax_string_attr(elem, "AXSubrole").unwrap_or_default();
        let editable_attr = ax_bool_attr(elem, "AXEditable").unwrap_or(false);

        let role_lower = role.to_lowercase();
        let sub_lower = subrole.to_lowercase();

        // Common editable roles:
        // - AXTextField (single line)
        // - AXTextArea (multi line)
        // - AXSearchField (single line)
        let is_text_field = role_lower.contains("textfield")
            || role_lower.contains("searchfield")
            || sub_lower.contains("searchfield");
        let is_text_area = role_lower.contains("textarea");

        let is_editable_role = is_text_field || is_text_area;
        let editable = editable_attr || is_editable_role;
        let single_line = is_text_field && !is_text_area;

        CFRelease(value);
        CFRelease(sys as *const _);

        Ok(Some(FocusedTextboxInfo {
            editable,
            single_line,
        }))
    }
}

fn ax_string_attr(elem: AXUIElementRef, name: &str) -> Option<String> {
    unsafe {
        let attr = CFString::new(name);
        let mut value: CFTypeRef = std::ptr::null_mut();
        let res = AXUIElementCopyAttributeValue(
            elem,
            attr.as_concrete_TypeRef(),
            &mut value as *mut CFTypeRef,
        );
        if res != 0 || value.is_null() {
            return None;
        }
        let sref = value as CFStringRef;
        let s = CFString::wrap_under_get_rule(sref);
        let out = s.to_string();
        CFRelease(value);
        Some(out)
    }
}

fn ax_bool_attr(elem: AXUIElementRef, name: &str) -> Option<bool> {
    unsafe {
        let attr = CFString::new(name);
        let mut value: CFTypeRef = std::ptr::null_mut();
        let res = AXUIElementCopyAttributeValue(
            elem,
            attr.as_concrete_TypeRef(),
            &mut value as *mut CFTypeRef,
        );
        if res != 0 || value.is_null() {
            return None;
        }
        // CFBooleanRef is a CFTypeRef; compare against kCFBooleanTrue.
        let b = value == kCFBooleanTrue as *const _ as *mut _;
        CFRelease(value);
        Some(b)
    }
}

fn simulate_cmd_v() -> Result<(), String> {
    // Prefer native CGEvent injection rather than AppleScript (faster, less flaky).
    unsafe {
        let v_keycode: u16 = 0x09; // kVK_ANSI_V
        let cmd_flag: u64 = 1 << 20; // kCGEventFlagMaskCommand

        let down = CGEventCreateKeyboardEvent(std::ptr::null_mut(), v_keycode, true);
        if down.is_null() {
            return Err("CGEventCreateKeyboardEvent failed".to_string());
        }
        CGEventSetFlags(down, cmd_flag);
        CGEventPost(K_CG_HID_EVENT_TAP, down);
        CFRelease(down as *const _);

        let up = CGEventCreateKeyboardEvent(std::ptr::null_mut(), v_keycode, false);
        if up.is_null() {
            return Err("CGEventCreateKeyboardEvent failed".to_string());
        }
        CGEventSetFlags(up, cmd_flag);
        CGEventPost(K_CG_HID_EVENT_TAP, up);
        CFRelease(up as *const _);
    }

    Ok(())
}

// =============================================================================
// Fn double-press listener
// =============================================================================

#[derive(Debug, Clone, Copy)]
enum FnEvent {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FnKeyMode {
    /// Press and hold Fn to start dictation; releasing Fn stops and inserts transcript.
    /// A quick tap is ignored so macOS can open the emoji/character viewer normally.
    HoldToTalk { hold_ms: u32 },
    /// Legacy behavior: double-press Fn to start dictation; releasing Fn stops and inserts transcript.
    DoublePress { window_ms: u32 },
}

pub struct FnKeyListener {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
    active: Arc<AtomicBool>,
    mode: FnKeyMode,
}

impl FnKeyListener {
    pub fn start<R: Runtime>(app: AppHandle<R>, mode: FnKeyMode) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));

        let thread_stop = Arc::clone(&stop);
        let thread_active = Arc::clone(&active);

        let (tx, rx) = mpsc::channel::<FnEvent>();

        // Event tap thread: feeds Fn key down/up events into channel.
        // Note: Fn key handling is notoriously inconsistent; this is best-effort and
        // users can switch to the global shortcut mode in settings.
        let tap_stop = Arc::clone(&stop);
        let tap_active = Arc::clone(&active);
        std::thread::Builder::new()
            .name("dictation-fn-eventtap".into())
            .spawn(move || unsafe {
                let mask = (1u64 << K_CG_EVENT_KEY_DOWN)
                    | (1u64 << K_CG_EVENT_KEY_UP)
                    | (1u64 << K_CG_EVENT_FLAGS_CHANGED);

                let boxed_sender = Box::new(tx);
                let user_info = Box::into_raw(boxed_sender) as *mut c_void;

                let tap = CGEventTapCreate(
                    K_CG_SESSION_EVENT_TAP,
                    K_CG_HEAD_INSERT_EVENT_TAP,
                    0,
                    mask,
                    Some(event_tap_callback),
                    user_info,
                );

                if tap.is_null() {
                    // Reclaim the sender box we allocated.
                    let _ = Box::from_raw(user_info as *mut mpsc::Sender<FnEvent>);
                    tap_active.store(false, Ordering::Relaxed);
                    return;
                }

                tap_active.store(true, Ordering::Relaxed);

                let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
                if source.is_null() {
                    CFRelease(tap as *const _);
                    // Reclaim the sender box we allocated.
                    let _ = Box::from_raw(user_info as *mut mpsc::Sender<FnEvent>);
                    tap_active.store(false, Ordering::Relaxed);
                    return;
                }

                let rl = CFRunLoopGetCurrent();
                CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);

                while !tap_stop.load(Ordering::Relaxed) {
                    CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 0);
                }

                CFRunLoopRemoveSource(rl, source, kCFRunLoopCommonModes);
                CFRelease(source as *const _);
                CFRelease(tap as *const _);

                // Safety: when the tap is created, user_info points to a Box<Sender>.
                // We must reclaim it once the runloop ends.
                let _ = Box::from_raw(user_info as *mut mpsc::Sender<FnEvent>);
            })
            .map_err(|e| format!("Failed to spawn event tap thread: {e}"))?;

        let join = std::thread::Builder::new()
            .name("dictation-fn-detector".into())
            .spawn(move || {
                let mut last_down: Option<Instant> = None;
                let mut armed = false;
                let mut started = false;
                let mut is_down = false;

                while !thread_stop.load(Ordering::Relaxed) {
                    // In hold-to-talk mode, we need to wake up when the hold threshold is reached.
                    // In double-press mode, a slower poll is fine.
                    let timeout = match mode {
                        FnKeyMode::HoldToTalk { hold_ms } => {
                            if is_down && !started {
                                let elapsed = last_down.map(|t| t.elapsed()).unwrap_or_default();
                                let want = Duration::from_millis(hold_ms as u64);
                                if elapsed >= want {
                                    // Threshold already met; don't block.
                                    Duration::from_millis(0)
                                } else {
                                    // Wake at threshold (or soon) to start dictation.
                                    want.saturating_sub(elapsed).min(Duration::from_millis(60))
                                }
                            } else {
                                Duration::from_millis(250)
                            }
                        }
                        FnKeyMode::DoublePress { .. } => Duration::from_millis(250),
                    };

                    // In hold-to-talk mode, start dictation once the hold threshold is reached.
                    if let FnKeyMode::HoldToTalk { hold_ms } = mode {
                        if is_down && !started {
                            let elapsed = last_down.map(|t| t.elapsed()).unwrap_or_default();
                            if elapsed >= Duration::from_millis(hold_ms as u64) {
                                started = true;
                                armed = false;
                                if let Some(state) = app.try_state::<AppState>() {
                                    if let Ok(mut svc) = state.dictation.lock() {
                                        let _ = svc.start(&app);
                                    }
                                }
                            }
                        }
                    }

                    let evt = match rx.recv_timeout(timeout) {
                        Ok(e) => e,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };

                    match evt {
                        FnEvent::Down => {
                            if is_down {
                                continue;
                            }
                            is_down = true;
                            let now = Instant::now();
                            match mode {
                                FnKeyMode::HoldToTalk { .. } => {
                                    // Arm a hold timer. We only start if the key remains down past the threshold.
                                    armed = true;
                                    started = false;
                                }
                                FnKeyMode::DoublePress { window_ms } => {
                                    if let Some(prev) = last_down {
                                        if now.duration_since(prev).as_millis() <= window_ms as u128
                                        {
                                            armed = false;
                                            started = true;
                                            // Start dictation immediately on second press.
                                            if let Some(state) = app.try_state::<AppState>() {
                                                if let Ok(mut svc) = state.dictation.lock() {
                                                    let _ = svc.start(&app);
                                                }
                                            }
                                        } else {
                                            armed = true;
                                        }
                                    } else {
                                        armed = true;
                                    }
                                }
                            }
                            last_down = Some(now);
                        }
                        FnEvent::Up => {
                            if !is_down {
                                continue;
                            }
                            is_down = false;
                            if started {
                                // Hold-to-talk: stop on the next Fn up after start.
                                if let Some(state) = app.try_state::<AppState>() {
                                    let settings = state.settings.blocking_lock().clone();
                                    if let Ok(mut svc) = state.dictation.lock() {
                                        let _ = svc.stop(&app, &settings);
                                    }
                                }
                                started = false;
                                armed = false;
                                last_down = None;
                            } else if armed {
                                // Hold-to-talk: quick tap is ignored (let macOS show emoji viewer).
                                // Double-press: remain armed for a second press.
                            }
                        }
                    }
                }

                thread_active.store(false, Ordering::Relaxed);
            })
            .map_err(|e| format!("Failed to spawn fn detector thread: {e}"))?;

        Ok(Self {
            stop,
            join: Some(join),
            active,
            mode,
        })
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn mode(&self) -> FnKeyMode {
        self.mode
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

// =============================================================================
// FFI
// =============================================================================

type AXUIElementRef = *mut c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;

    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kCFBooleanTrue: *const c_void;

    fn CGEventCreateKeyboardEvent(source: *mut c_void, keycode: u16, keydown: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: i32, event: *mut c_void);

    fn CGEventTapCreate(
        tap: i32,
        place: i32,
        options: i32,
        events_of_interest: u64,
        callback: Option<extern "C" fn(i32, i32, *mut c_void, *mut c_void) -> *mut c_void>,
        user_info: *mut c_void,
    ) -> *mut c_void;

    fn CFMachPortCreateRunLoopSource(
        allocator: *mut c_void,
        port: *mut c_void,
        order: i32,
    ) -> *mut c_void;

    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRemoveSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRunInMode(
        mode: *const c_void,
        seconds: f64,
        return_after_source_handled: i32,
    ) -> i32;

    static kCFRunLoopDefaultMode: *const c_void;
    static kCFRunLoopCommonModes: *const c_void;

    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
}

const K_CG_HID_EVENT_TAP: i32 = 0;
const K_CG_SESSION_EVENT_TAP: i32 = 1;
const K_CG_HEAD_INSERT_EVENT_TAP: i32 = 0;

const K_CG_EVENT_KEY_DOWN: i32 = 10;
const K_CG_EVENT_KEY_UP: i32 = 11;
const K_CG_EVENT_FLAGS_CHANGED: i32 = 12;

const KVK_FUNCTION: i64 = 0x3F;

extern "C" fn event_tap_callback(
    _proxy: i32,
    event_type: i32,
    event: *mut c_void,
    user_info: *mut c_void,
) -> *mut c_void {
    unsafe {
        // Key code field is 9 in CGEventField.
        let keycode = CGEventGetIntegerValueField(event, 9);
        if keycode == KVK_FUNCTION {
            let tx = &*(user_info as *mut mpsc::Sender<FnEvent>);
            match event_type {
                K_CG_EVENT_FLAGS_CHANGED => {
                    // Fn is represented as a modifier; flags-changed is the most reliable.
                    let flags = CGEventGetFlags(event);
                    let is_down = (flags & K_CG_EVENT_FLAG_MASK_SECONDARY_FN) != 0;
                    let _ = tx.send(if is_down { FnEvent::Down } else { FnEvent::Up });
                }
                K_CG_EVENT_KEY_DOWN => {
                    let _ = tx.send(FnEvent::Down);
                }
                K_CG_EVENT_KEY_UP => {
                    let _ = tx.send(FnEvent::Up);
                }
                _ => {}
            }
        }
        event
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventGetIntegerValueField(event: *mut c_void, field: i32) -> i64;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
}

// `kCGEventFlagMaskSecondaryFn` (Carbon) = 0x0080_0000.
const K_CG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;
