use enigo::{Enigo, Keyboard, Settings};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, LogicalPosition, Manager, State, WebviewUrl};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tokio::sync::Mutex;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, MoveWindow,
    SetForegroundWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST, HWND_TOPMOST, SW_RESTORE,
    SW_SHOW, SWP_SHOWWINDOW,
};

mod audio;
mod config;
mod keyboard_hook;
mod postprocess;
mod stt;

use audio::{split_wav_for_api, AudioRecorder, LevelCallback};
use config::{delete_stored_api_key, store_api_key, Config};
use postprocess::polish_transcript;
use stt::{SaarasProvider, SharedProvider, SttResult};

const RECORDING_STALE_AFTER_SECONDS: u64 = 180;
const SAARAS_REST_CHUNK_SECONDS: u32 = 28;

fn startup_log(message: impl AsRef<str>) {
    let path = std::env::temp_dir().join("voca-startup.log");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "{}", message.as_ref())
        });
}

#[derive(Clone)]
struct AppState {
    provider: SharedProvider,
    config: Arc<Mutex<Config>>,
    recording: Arc<Mutex<bool>>,
    recording_started_at: Arc<Mutex<Option<Instant>>>,
    stop_signal: Arc<AtomicBool>,
    paste_target_hwnd: Arc<Mutex<Option<usize>>>,
    hotkey_status: Arc<Mutex<String>>,
}

async fn begin_recording(state: &AppState, app: &tauri::AppHandle) -> Result<(), String> {
    let mut rec = state.recording.lock().await;
    if *rec {
        let mut started_at = state.recording_started_at.lock().await;
        if started_at
            .map(|started| started.elapsed() > Duration::from_secs(RECORDING_STALE_AFTER_SECONDS))
            .unwrap_or(true)
        {
            *rec = false;
            *started_at = None;
            let _ = app.emit(
                "dictation-status",
                "Recovered stale recording state. Starting again...",
            );
        } else {
            let _ = app.emit(
                "dictation-status",
                "Already recording. Wait for the current capture to finish, or press Reset.",
            );
            return Err("Already recording".into());
        }
    }

    state.stop_signal.store(false, Ordering::Relaxed);
    *state.paste_target_hwnd.lock().await = unsafe { Some(GetForegroundWindow().0 as usize) };
    *rec = true;
    *state.recording_started_at.lock().await = Some(Instant::now());
    Ok(())
}

async fn clear_recording(state: &AppState) {
    *state.recording.lock().await = false;
    *state.recording_started_at.lock().await = None;
    state.stop_signal.store(false, Ordering::Relaxed);
}

fn remove_recording_files(audio_path: &std::path::Path, parts: &[std::path::PathBuf]) {
    for path in parts {
        if path != audio_path {
            let _ = std::fs::remove_file(path);
        }
    }
    let _ = std::fs::remove_file(audio_path);
}

async fn request_stop_recording(state: &AppState) {
    state.stop_signal.store(true, Ordering::Relaxed);
}

fn build_provider(config: &Config) -> Result<Box<dyn stt::SttProvider>, String> {
    if config.provider_name.as_deref() == Some("local") {
        return Err("The local provider is not implemented in this release".into());
    }

    let required = |value: &Option<String>, name: &str| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("Missing provider {} in config", name))
    };

    Ok(Box::new(SaarasProvider::new(
        required(&config.endpoint, "endpoint")?,
        config.api_key(),
        required(&config.model, "model")?,
        required(&config.language, "language")?,
        config.codemix.unwrap_or(true),
    )))
}

struct WindowCandidate {
    hwnd: HWND,
    area: i64,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let candidates = &mut *(lparam.0 as *mut Vec<WindowCandidate>);
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid != std::process::id() {
        return BOOL(1);
    }

    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        let width = (rect.right - rect.left) as i64;
        let height = (rect.bottom - rect.top) as i64;
        let area = width.saturating_mul(height);
        candidates.push(WindowCandidate { hwnd, area });
    }

    BOOL(1)
}

fn best_window_handle() -> Option<HWND> {
    let mut candidates: Vec<WindowCandidate> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut candidates as *mut _ as isize));
    }
    candidates.into_iter().max_by_key(|c| c.area).map(|c| c.hwnd)
}

fn restore_paste_target(hwnd_value: Option<usize>) {
    if let Some(value) = hwnd_value {
        if value != 0 {
            let hwnd = HWND(value as *mut core::ffi::c_void);
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            std::thread::sleep(Duration::from_millis(80));
        }
    }
}

fn build_main_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    startup_log("building main window");
    tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("voca")
        .inner_size(1200.0, 800.0)
        .resizable(false)
        .decorations(true)
        .visible(true)
        .build()
        .map_err(|e| e.to_string())
}

fn build_recording_overlay(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    let window = tauri::WebviewWindowBuilder::new(
        app,
        "recording-overlay",
        WebviewUrl::App("overlay.html".into()),
    )
    .title("Voca recording")
    .inner_size(264.0, 72.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;

    let _ = window.set_ignore_cursor_events(true);
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size().to_logical::<f64>(scale);
        let origin = monitor.position().to_logical::<f64>(scale);
        let x = origin.x + (size.width - 264.0) / 2.0;
        let y = origin.y + size.height - 120.0;
        let _ = window.set_position(LogicalPosition::new(x, y));
    }

    Ok(window)
}

fn show_recording_overlay(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("recording-overlay") {
        let _ = window.show();
    }
}

fn hide_recording_overlay(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("recording-overlay") {
        let _ = window.hide();
    }
}

fn hide_recording_overlay_after(app: &tauri::AppHandle, delay_ms: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        hide_recording_overlay(&app);
    });
}

fn reveal_main_window(app: &tauri::AppHandle) {
    startup_log("reveal_main_window called");
    if let Some(window) = app.get_webview_window("main") {
        startup_log("main window already exists");
        let _ = window.show();
        let _ = window.unminimize();
        if let Ok(hwnd) = window.hwnd() {
            unsafe {
                let fg = GetForegroundWindow();
                let current = GetCurrentThreadId();
                let fg_thread = GetWindowThreadProcessId(fg, None);
                let _ = AttachThreadInput(current, fg_thread, true);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = MoveWindow(hwnd, 80, 80, 1200, 800, true);
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 80, 80, 1200, 800, SWP_SHOWWINDOW);
                let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 80, 80, 1200, 800, SWP_SHOWWINDOW);
                let _ = SetForegroundWindow(hwnd);
                let _ = AttachThreadInput(current, fg_thread, false);
            }
        } else {
            let _ = window.set_focus();
        }
        return;
    }

    match build_main_window(app) {
        Ok(window) => {
        startup_log("main window built");
        let _ = window.set_title("voca");
        let _ = window.show();
        let _ = window.unminimize();
        if let Ok(hwnd) = window.hwnd() {
            unsafe {
                let fg = GetForegroundWindow();
                let current = GetCurrentThreadId();
                let fg_thread = GetWindowThreadProcessId(fg, None);
                let _ = AttachThreadInput(current, fg_thread, true);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = MoveWindow(hwnd, 80, 80, 1200, 800, true);
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 80, 80, 1200, 800, SWP_SHOWWINDOW);
                let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 80, 80, 1200, 800, SWP_SHOWWINDOW);
                let _ = SetForegroundWindow(hwnd);
                let _ = AttachThreadInput(current, fg_thread, false);
            }
        } else {
            let _ = window.set_focus();
        }
        }
        Err(e) => {
            startup_log(format!("main window build failed: {}", e));
        }
    }

    if let Some(hwnd) = best_window_handle() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = MoveWindow(hwnd, 80, 80, 1200, 800, true);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 80, 80, 1200, 800, SWP_SHOWWINDOW);
            let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 80, 80, 1200, 800, SWP_SHOWWINDOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

async fn run_dictation(state: AppState, app: tauri::AppHandle) -> Result<SttResult, String> {
    let config = state.config.lock().await.clone();
    if config.provider_name.as_deref() != Some("local") && config.api_key().is_none() {
        let message = "Add your Sarvam API key in Voca before dictating";
        let _ = app.emit("dictation-error", message);
        return Err(message.into());
    }

    begin_recording(&state, &app).await?;

    show_recording_overlay(&app);
    let _ = app.emit("dictation-started", ());
    let _ = app.emit("dictation-status", "Recording audio...");

    let temp_dir = std::env::temp_dir();
    let audio_path = temp_dir.join("voca-recording.wav");

    let recorder = AudioRecorder::new();
    let level_app = app.clone();
    let on_level: LevelCallback = Arc::new(move |level| {
        let _ = level_app.emit("mic-level", level);
    });
    let summary = match recorder.record_until_silence_or_stop_with_levels(
        &audio_path,
        state.stop_signal.clone(),
        on_level,
    ) {
        Ok(summary) => summary,
        Err(e) => {
            clear_recording(&state).await;
            let _ = app.emit("dictation-error", format!("Recording failed: {}", e));
            hide_recording_overlay_after(&app, 1_200);
            return Err(format!("Recording failed: {}", e));
        }
    };

    let _ = app.emit("dictation-processing", ());

    let audio_size = std::fs::metadata(&audio_path)
        .map(|m| m.len())
        .unwrap_or_default();
    let _ = app.emit(
        "dictation-status",
        format!(
            "Recorded {:.1}s ({} KB, {} Hz, {} ch, peak {:.0}%). Sending to Sarvam...",
            summary.duration_ms as f32 / 1000.0,
            audio_size / 1024,
            summary.sample_rate,
            summary.channels,
            summary.peak_level * 100.0
        ),
    );

    let audio_parts = match split_wav_for_api(&audio_path, SAARAS_REST_CHUNK_SECONDS) {
        Ok(parts) => parts,
        Err(e) => {
            let _ = std::fs::remove_file(&audio_path);
            clear_recording(&state).await;
            let _ = app.emit("dictation-error", format!("Audio preparation failed: {}", e));
            hide_recording_overlay_after(&app, 1_200);
            return Err(format!("Audio preparation failed: {}", e));
        }
    };

    let guard = state.provider.lock().await;
    let mut result = SttResult {
        text: String::new(),
        confidence: None,
        language: None,
    };
    for (index, part) in audio_parts.iter().enumerate() {
        if audio_parts.len() > 1 {
            let _ = app.emit(
                "dictation-status",
                format!("Transcribing part {} of {}...", index + 1, audio_parts.len()),
            );
        }

        match guard.transcribe(part).await {
            Ok(part_result) => {
                if !result.text.is_empty() && !part_result.text.trim().is_empty() {
                    result.text.push(' ');
                }
                result.text.push_str(part_result.text.trim());
                result.language = result.language.or(part_result.language);
                result.confidence = result.confidence.or(part_result.confidence);
            }
            Err(e) => {
                drop(guard);
                remove_recording_files(&audio_path, &audio_parts);
                clear_recording(&state).await;
                let _ = app.emit("dictation-error", format!("Transcription failed: {}", e));
                hide_recording_overlay_after(&app, 1_200);
                return Err(format!("Transcription failed: {}", e));
            }
        }
    }
    drop(guard);
    remove_recording_files(&audio_path, &audio_parts);

    let _ = app.emit("dictation-status", "Polishing transcript...");
    result.text = match polish_transcript(&result.text, &config).await {
        Ok(text) => text,
        Err(e) => {
            let _ = app.emit(
                "dictation-status",
                format!("Polish failed; using transcript: {}", e),
            );
            postprocess::light_polish(&result.text)
        }
    };

    let text = result.text.clone();
    let _ = app.clipboard().write_text(text.clone());
    std::thread::sleep(std::time::Duration::from_millis(100));
    let paste_target = *state.paste_target_hwnd.lock().await;

    let paste_result = (|| -> Result<(), String> {
        restore_paste_target(paste_target);
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;

        #[cfg(target_os = "macos")]
        {
            enigo.key(enigo::Key::Meta, enigo::Direction::Press).map_err(|e| e.to_string())?;
            enigo.key(enigo::Key::Unicode('v'), enigo::Direction::Click).map_err(|e| e.to_string())?;
            enigo.key(enigo::Key::Meta, enigo::Direction::Release).map_err(|e| e.to_string())?;
        }

        #[cfg(not(target_os = "macos"))]
        {
            enigo.key(enigo::Key::Control, enigo::Direction::Press).map_err(|e| e.to_string())?;
            enigo.key(enigo::Key::Unicode('v'), enigo::Direction::Click).map_err(|e| e.to_string())?;
            enigo.key(enigo::Key::Control, enigo::Direction::Release).map_err(|e| e.to_string())?;
        }

        Ok(())
    })();

    clear_recording(&state).await;
    let _ = app.emit("dictation-finished", &result);
    hide_recording_overlay_after(&app, 700);
    if let Err(e) = paste_result {
        let _ = app.emit(
            "dictation-status",
            format!("Transcript ready and copied. Auto-paste failed: {}", e),
        );
    }

    Ok(result)
}

#[tauri::command]
async fn test_microphone(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<String, String> {
    begin_recording(state.inner(), &app).await?;

    let _ = app.emit("mic-test-started", ());

    let recorder = AudioRecorder::new();
    let result = recorder.probe_microphone();

    clear_recording(state.inner()).await;

    match result {
        Ok(message) => {
            let _ = app.emit("mic-test-finished", message.clone());
            Ok(message)
        }
        Err(e) => {
            let _ = app.emit("mic-test-error", e.clone());
            Err(e)
        }
    }
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    Ok(state.config.lock().await.clone())
}

#[tauri::command]
async fn get_hotkey_status(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.hotkey_status.lock().await.clone())
}

#[tauri::command]
fn get_config_path() -> Result<String, String> {
    Config::path("voca")
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "Unable to resolve config path".into())
}

#[tauri::command]
async fn save_config(state: State<'_, AppState>, config: Config) -> Result<Config, String> {
    let current = state.config.lock().await.clone();
    let merged = merge_config_update(current, config);

    merged.save("voca")?;

    {
        let mut current = state.config.lock().await;
        *current = merged.clone();
    }

    {
        let mut provider = state.provider.lock().await;
        *provider = build_provider(&merged)?;
    }

    Ok(merged)
}

#[tauri::command]
async fn set_api_key(state: State<'_, AppState>, api_key: String) -> Result<String, String> {
    store_api_key(&api_key)?;
    let config = state.config.lock().await.clone();
    let mut provider = state.provider.lock().await;
    *provider = build_provider(&config)?;
    Ok("API key saved securely in Windows Credential Manager".into())
}

#[tauri::command]
async fn has_api_key(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.config.lock().await.api_key().is_some())
}

#[tauri::command]
async fn clear_api_key(state: State<'_, AppState>) -> Result<String, String> {
    delete_stored_api_key()?;
    let config = state.config.lock().await.clone();
    let mut provider = state.provider.lock().await;
    *provider = build_provider(&config)?;
    Ok("Saved API key removed".into())
}

fn merge_config_update(current: Config, update: Config) -> Config {
    Config {
        provider_name: update.provider_name,
        endpoint: update.endpoint.or(current.endpoint),
        api_key_env_var: update.api_key_env_var.or(current.api_key_env_var),
        model: update.model.or(current.model),
        language: update.language.or(current.language),
        codemix: update.codemix.or(current.codemix),
        hotkey: update.hotkey.or(current.hotkey),
        polish_mode: update.polish_mode.or(current.polish_mode),
        polish_endpoint: update.polish_endpoint.or(current.polish_endpoint),
        polish_model: update.polish_model.or(current.polish_model),
        polish_api_key_env_var: update
            .polish_api_key_env_var
            .or(current.polish_api_key_env_var),
    }
}

#[tauri::command]
async fn get_provider_name(state: State<'_, AppState>) -> Result<String, String> {
    let guard = state.provider.lock().await;
    Ok(guard.name().to_string())
}

#[tauri::command]
async fn toggle_dictation(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<SttResult, String> {
    run_dictation(state.inner().clone(), app).await
}

#[tauri::command]
async fn is_recording(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(*state.recording.lock().await)
}

#[tauri::command]
async fn reset_recording(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<String, String> {
    request_stop_recording(state.inner()).await;
    clear_recording(state.inner()).await;
    let _ = app.emit("dictation-status", "Recording state reset.");
    Ok("recording-reset".into())
}

#[tauri::command]
async fn stop_recording(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<String, String> {
    request_stop_recording(state.inner()).await;
    let _ = app.emit("dictation-status", "Stopping recording...");
    Ok("recording-stop-requested".into())
}

#[tauri::command]
async fn reveal_window(app: tauri::AppHandle) -> Result<String, String> {
    reveal_main_window(&app);
    Ok("window-revealed".into())
}

pub fn run() {
    let config = Config::load("voca");
    let hotkey_str = config.hotkey.clone().unwrap_or_else(|| "CmdOrCtrl+Shift+S".into());

    let state = AppState {
        provider: Arc::new(Mutex::new(
            build_provider(&config).expect("shipped provider configuration must be valid"),
        )),
        config: Arc::new(Mutex::new(config)),
        recording: Arc::new(Mutex::new(false)),
        recording_started_at: Arc::new(Mutex::new(None)),
        stop_signal: Arc::new(AtomicBool::new(false)),
        paste_target_hwnd: Arc::new(Mutex::new(None)),
        hotkey_status: Arc::new(Mutex::new("Hotkey registration pending...".into())),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            get_config_path,
            save_config,
            set_api_key,
            has_api_key,
            clear_api_key,
            get_hotkey_status,
            get_provider_name,
            test_microphone,
            toggle_dictation,
            is_recording,
            stop_recording,
            reset_recording,
            reveal_window,
        ])
        .setup(move |app| {
            startup_log("setup started");
            let app_handle = app.handle().clone();
            reveal_main_window(&app_handle);
            if let Err(error) = build_recording_overlay(&app_handle) {
                startup_log(format!("recording overlay build failed: {}", error));
            }

            // Tray icon
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "settings" => {
                        reveal_main_window(app);
                    }
                    _ => {}
                })
                .build(app)?;

            let (hook_tx, hook_rx) = mpsc::channel();
            let status = match keyboard_hook::start(hotkey_str.as_str(), hook_tx) {
                Ok(status) => status,
                Err(e) => format!("Keyboard hook failed: {}", e),
            };
            let hook_app = app_handle.clone();
            let hook_state = app.state::<AppState>().inner().clone();
            std::thread::spawn(move || {
                for event in hook_rx {
                    let app_clone = hook_app.clone();
                    let state = hook_state.clone();
                    match event {
                        keyboard_hook::HookEvent::Pressed => {
                            tauri::async_runtime::spawn(async move {
                                let _ = run_dictation(state, app_clone).await;
                            });
                        }
                        keyboard_hook::HookEvent::Released => {
                            tauri::async_runtime::spawn(async move {
                                request_stop_recording(&state).await;
                                let _ = app_clone.emit("dictation-status", "Stopping recording...");
                            });
                        }
                    }
                }
            });

            if let Some(state) = app.try_state::<AppState>() {
                let mut guard = state.hotkey_status.blocking_lock();
                *guard = status.clone();
            }
            let _ = app_handle.emit("hotkey-status", status);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_, _| {});
}

#[cfg(test)]
mod tests {
    use super::merge_config_update;
    use crate::config::Config;

    #[test]
    fn save_config_update_preserves_hidden_provider_fields() {
        let current = Config {
            endpoint: Some("https://example.invalid/speech-to-text".into()),
            api_key_env_var: Some("SAARAS_API_KEY".into()),
            model: Some("test-model".into()),
            language: Some("hi-IN".into()),
            codemix: Some(true),
            hotkey: Some("Ctrl+Alt+Shift+S".into()),
            polish_mode: Some("light".into()),
            polish_endpoint: Some("http://localhost:11434/v1/chat/completions".into()),
            polish_model: Some("local-model".into()),
            polish_api_key_env_var: Some("LOCAL_KEY".into()),
            ..Default::default()
        };
        let update = Config {
            language: Some("ta-IN".into()),
            codemix: Some(false),
            ..Default::default()
        };

        let merged = merge_config_update(current, update);

        assert_eq!(
            merged.endpoint.as_deref(),
            Some("https://example.invalid/speech-to-text")
        );
        assert_eq!(merged.api_key_env_var.as_deref(), Some("SAARAS_API_KEY"));
        assert_eq!(merged.model.as_deref(), Some("test-model"));
        assert_eq!(merged.language.as_deref(), Some("ta-IN"));
        assert_eq!(merged.codemix, Some(false));
        assert_eq!(merged.hotkey.as_deref(), Some("Ctrl+Alt+Shift+S"));
        assert_eq!(merged.polish_mode.as_deref(), Some("light"));
        assert_eq!(
            merged.polish_endpoint.as_deref(),
            Some("http://localhost:11434/v1/chat/completions")
        );
        assert_eq!(merged.polish_model.as_deref(), Some("local-model"));
        assert_eq!(merged.polish_api_key_env_var.as_deref(), Some("LOCAL_KEY"));
    }
}
