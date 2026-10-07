use enigo::{Enigo, Keyboard, Settings};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State, WebviewUrl};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tokio::sync::Mutex;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

mod audio;
mod config;
mod evaluation;
mod keyboard_hook;
mod postprocess;
mod stt;

use audio::{split_wav_for_api, AudioRecorder};
use config::{delete_stored_api_key, store_api_key, Config};
use evaluation::{
    evaluate_models, session_to_csv, EvaluationMetadata, EvaluationRun, EvaluationSession,
};
use postprocess::polish_transcript;
use stt::{SaarasProvider, SharedProvider, SttResult};

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
    recording: Arc<Mutex<RecordingLifecycle>>,
    next_recording_id: Arc<AtomicU64>,
    hotkey_status: Arc<Mutex<String>>,
    evaluation: Arc<Mutex<EvaluationSession>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyAction {
    NormalDictation,
    Evaluation,
}

fn hotkey_action(evaluation: &EvaluationSession) -> HotkeyAction {
    if evaluation.enabled {
        HotkeyAction::Evaluation
    } else {
        HotkeyAction::NormalDictation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingPhase {
    Recording,
    Stopping,
}

#[derive(Debug)]
struct ActiveRecording {
    id: u64,
    phase: RecordingPhase,
    stop_signal: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
struct RecordingLifecycle {
    active: Option<ActiveRecording>,
}

#[derive(Debug, Clone)]
struct RecordingSession {
    id: u64,
    stop_signal: Arc<AtomicBool>,
    paste_target_hwnd: Option<usize>,
}

impl RecordingLifecycle {
    fn begin(
        &mut self,
        id: u64,
        stop_signal: Arc<AtomicBool>,
        paste_target_hwnd: Option<usize>,
    ) -> Result<RecordingSession, String> {
        if let Some(active) = self.active.as_ref() {
            let message = match active.phase {
                RecordingPhase::Recording => "Already recording",
                RecordingPhase::Stopping => "The previous recording is still shutting down",
            };
            return Err(message.into());
        }

        self.active = Some(ActiveRecording {
            id,
            phase: if stop_signal.load(Ordering::Acquire) {
                RecordingPhase::Stopping
            } else {
                RecordingPhase::Recording
            },
            stop_signal: stop_signal.clone(),
        });
        Ok(RecordingSession {
            id,
            stop_signal,
            paste_target_hwnd,
        })
    }

    fn request_stop(&mut self) -> bool {
        if let Some(active) = self.active.as_mut() {
            active.phase = RecordingPhase::Stopping;
            active.stop_signal.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }

    fn finish(&mut self, id: u64) -> bool {
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            self.active = None;
            true
        } else {
            false
        }
    }

    fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

async fn begin_recording(
    state: &AppState,
    initially_stopped: bool,
) -> Result<RecordingSession, String> {
    let id = state.next_recording_id.fetch_add(1, Ordering::Relaxed);
    let stop_signal = Arc::new(AtomicBool::new(initially_stopped));
    let paste_target_hwnd = unsafe { Some(GetForegroundWindow().0 as usize) };
    state
        .recording
        .lock()
        .await
        .begin(id, stop_signal, paste_target_hwnd)
}

async fn finish_recording(state: &AppState, id: u64) {
    state.recording.lock().await.finish(id);
}

async fn request_stop_recording(state: &AppState) -> bool {
    state.recording.lock().await.request_stop()
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

fn restore_paste_target(hwnd_value: Option<usize>) -> Result<(), String> {
    let value = hwnd_value.filter(|value| *value != 0).ok_or_else(|| {
        "The application that was focused before recording is no longer available".to_string()
    })?;
    let hwnd = HWND(value as *mut core::ffi::c_void);

    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return Err("The original paste target window is no longer valid".into());
        }
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    std::thread::sleep(Duration::from_millis(100));

    if unsafe { GetForegroundWindow() } != hwnd {
        return Err("The original paste target did not become active".into());
    }

    Ok(())
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

fn reveal_main_window(app: &tauri::AppHandle) {
    startup_log("reveal_main_window called");
    if let Some(window) = app.get_webview_window("main") {
        startup_log("main window already exists");
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }

    match build_main_window(app) {
        Ok(window) => {
            startup_log("main window built");
            let _ = window.set_title("voca");
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        Err(e) => {
            startup_log(format!("main window build failed: {}", e));
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DictationDiagnostics {
    duration_ms: u64,
    chunk_count: usize,
    sarvam_request_ms: u64,
    postprocess_ms: u64,
    paste_ms: u64,
    total_after_recording_ms: u64,
    selected_language_code: String,
    codemix: bool,
    returned_language_code: Option<String>,
    model: String,
    sample_rate: u32,
    channel_count: u16,
    peak_level_percent: f32,
    insertion_succeeded: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DictationResult {
    raw_transcript: String,
    final_transcript: String,
    diagnostics: DictationDiagnostics,
}

fn append_sarvam_chunk(assembled: &mut String, chunk: &str) {
    if !assembled.is_empty()
        && !chunk.is_empty()
        && !assembled.chars().last().is_some_and(char::is_whitespace)
        && !chunk.chars().next().is_some_and(char::is_whitespace)
    {
        assembled.push(' ');
    }
    assembled.push_str(chunk);
}

fn create_recording_temp_dir() -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("voca-dictation-")
        .tempdir()
        .map_err(|e| format!("Unable to create temporary audio directory: {}", e))
}

fn paste_from_clipboard(paste_target: Option<usize>) -> Result<(), String> {
    restore_paste_target(paste_target)?;
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Unable to initialize keyboard input: {}", e))?;

    enigo
        .key(enigo::Key::Control, enigo::Direction::Press)
        .map_err(|e| format!("Unable to press Ctrl for paste: {}", e))?;
    let paste_result = enigo
        .key(enigo::Key::Unicode('v'), enigo::Direction::Click)
        .map_err(|e| format!("Unable to send Ctrl+V: {}", e));
    let release_result = enigo
        .key(enigo::Key::Control, enigo::Direction::Release)
        .map_err(|e| format!("Unable to release Ctrl after paste: {}", e));

    paste_result?;
    release_result?;
    Ok(())
}

async fn run_dictation_session(
    state: &AppState,
    app: &tauri::AppHandle,
    config: &Config,
    session: &RecordingSession,
) -> Result<DictationResult, String> {
    let temp_dir = create_recording_temp_dir()?;
    let audio_path = temp_dir.path().join("recording.wav");

    let recorder = AudioRecorder::new();
    let summary = recorder
        .record_until_silence_or_stop(&audio_path, session.stop_signal.clone())
        .map_err(|e| format!("Recording failed: {}", e))?;

    if !summary.speech_detected {
        return Err("No speech detected; nothing was sent to Sarvam".into());
    }

    let after_recording_started = Instant::now();
    let audio_size = std::fs::metadata(&audio_path)
        .map(|metadata| metadata.len())
        .unwrap_or_default();
    let _ = app.emit(
        "dictation-status",
        format!(
            "Recorded {:.1}s ({} KB, {} Hz, {} ch). Sending to Sarvam...",
            summary.duration_ms as f32 / 1000.0,
            audio_size / 1024,
            summary.sample_rate,
            summary.channels,
        ),
    );

    let audio_parts = split_wav_for_api(&audio_path, SAARAS_REST_CHUNK_SECONDS)
        .map_err(|e| format!("Audio preparation failed: {}", e))?;
    let chunk_count = audio_parts.len();

    let sarvam_started = Instant::now();
    let guard = state.provider.lock().await;
    let mut raw_result = SttResult {
        text: String::new(),
        confidence: None,
        language: None,
        language_probability: None,
    };
    for (index, part) in audio_parts.iter().enumerate() {
        if chunk_count > 1 {
            let _ = app.emit(
                "dictation-status",
                format!("Transcribing part {} of {}...", index + 1, chunk_count),
            );
        }

        let part_result = guard
            .transcribe(part)
            .await
            .map_err(|e| format!("Transcription failed: {}", e))?;
        append_sarvam_chunk(&mut raw_result.text, &part_result.text);
        raw_result.language = raw_result.language.or(part_result.language);
        raw_result.confidence = raw_result.confidence.or(part_result.confidence);
        raw_result.language_probability = raw_result
            .language_probability
            .or(part_result.language_probability);
    }
    drop(guard);
    let sarvam_request_ms = sarvam_started.elapsed().as_millis() as u64;

    let _ = app.emit("dictation-status", "Preparing final output...");
    let postprocess_started = Instant::now();
    let final_transcript = match polish_transcript(&raw_result.text, config).await {
        Ok(text) => text,
        Err(e) => {
            let _ = app.emit(
                "dictation-status",
                format!("Polish failed; using light cleanup: {}", e),
            );
            postprocess::light_polish(&raw_result.text)
        }
    };
    let postprocess_ms = postprocess_started.elapsed().as_millis() as u64;

    let paste_started = Instant::now();
    let paste_result = (|| -> Result<(), String> {
        app.clipboard()
            .write_text(final_transcript.clone())
            .map_err(|e| format!("Unable to write transcript to clipboard: {}", e))?;
        paste_from_clipboard(session.paste_target_hwnd)
    })();
    let paste_ms = paste_started.elapsed().as_millis() as u64;

    let diagnostics = DictationDiagnostics {
        duration_ms: summary.duration_ms,
        chunk_count,
        sarvam_request_ms,
        postprocess_ms,
        paste_ms,
        total_after_recording_ms: after_recording_started.elapsed().as_millis() as u64,
        selected_language_code: config.language.clone().unwrap_or_else(|| "unknown".into()),
        codemix: config.codemix.unwrap_or(true),
        returned_language_code: raw_result.language.clone(),
        model: config.model.clone().unwrap_or_else(|| "unknown".into()),
        sample_rate: summary.sample_rate,
        channel_count: summary.channels,
        peak_level_percent: summary.peak_level * 100.0,
        insertion_succeeded: paste_result.is_ok(),
    };
    let result = DictationResult {
        raw_transcript: raw_result.text,
        final_transcript,
        diagnostics,
    };

    let _ = app.emit("dictation-result", &result);
    paste_result
        .map(|_| result)
        .map_err(|e| format!("Insertion failed: {}", e))
}

async fn run_dictation(
    state: AppState,
    app: tauri::AppHandle,
    recording_ready: Option<mpsc::Sender<()>>,
) -> Result<DictationResult, String> {
    startup_log("normal hotkey action entered");
    let config = state.config.lock().await.clone();
    if config.provider_name.as_deref() != Some("local") && config.api_key().is_none() {
        let message = "Add your Sarvam API key in Voca before dictating";
        startup_log("normal hotkey action rejected: API key unavailable");
        let _ = app.emit("dictation-error", message);
        return Err(message.into());
    }

    let session = match begin_recording(&state, false).await {
        Ok(session) => session,
        Err(error) => {
            let _ = app.emit("dictation-error", &error);
            return Err(error);
        }
    };
    if let Some(sender) = recording_ready {
        let _ = sender.send(());
    }

    let _ = app.emit("dictation-started", ());
    let _ = app.emit("dictation-status", "Recording audio...");
    let outcome = run_dictation_session(&state, &app, &config, &session).await;
    finish_recording(&state, session.id).await;

    match outcome {
        Ok(result) => {
            let _ = app.emit("dictation-finished", ());
            Ok(result)
        }
        Err(error) => {
            let _ = app.emit("dictation-error", &error);
            Err(error)
        }
    }
}

async fn run_evaluation_session(
    state: &AppState,
    app: &tauri::AppHandle,
    config: &Config,
    session: &RecordingSession,
) -> Result<EvaluationRun, String> {
    let temp_dir = create_recording_temp_dir()?;
    let audio_path = temp_dir.path().join("evaluation.wav");
    let recorder = AudioRecorder::new();
    let summary = recorder
        .record_until_silence_or_stop(&audio_path, session.stop_signal.clone())
        .map_err(|error| format!("Recording failed: {}", error))?;

    startup_log(format!(
        "evaluation recording finished: duration_ms={} speech_detected={}",
        summary.duration_ms, summary.speech_detected
    ));

    if !summary.speech_detected {
        return Err("No speech detected; nothing was sent to Sarvam".into());
    }

    let (run_number, expected_text) = {
        let evaluation = state.evaluation.lock().await;
        (
            evaluation.next_run_number(),
            evaluation.expected_text.clone(),
        )
    };
    let mode = if config.codemix.unwrap_or(true) {
        "codemix"
    } else {
        "transcribe"
    };
    let metadata = EvaluationMetadata {
        run_number,
        expected_text,
        selected_language_code: config.language.clone().unwrap_or_else(|| "unknown".into()),
        mode: mode.into(),
        audio_duration_ms: summary.duration_ms,
        sample_rate: summary.sample_rate,
        channels: summary.channels,
    };

    let _ = app.emit(
        "dictation-status",
        format!("Running V3/V4 evaluation {}...", run_number),
    );
    let provider = state.provider.lock().await;
    let result = evaluate_models(provider.as_ref(), &audio_path, metadata).await;
    drop(provider);

    startup_log(format!(
        "evaluation models finished: run={} v3_success={} v4_success={}",
        result.run_number, result.v3.success, result.v4.success
    ));

    state.evaluation.lock().await.runs.push(result.clone());
    if let Err(error) = app.emit("evaluation-result", &result) {
        startup_log(format!("evaluation-result emit failed: {}", error));
    }
    Ok(result)
}

async fn run_evaluation(
    state: AppState,
    app: tauri::AppHandle,
    recording_ready: Option<mpsc::Sender<()>>,
) -> Result<EvaluationRun, String> {
    startup_log("evaluation hotkey action entered");
    let config = state.config.lock().await.clone();
    if config.provider_name.as_deref() != Some("local") && config.api_key().is_none() {
        let message = "Add your Sarvam API key in Voca before evaluating";
        startup_log("evaluation hotkey action rejected: API key unavailable");
        let _ = app.emit("dictation-error", message);
        return Err(message.into());
    }

    let session = match begin_recording(&state, false).await {
        Ok(session) => session,
        Err(error) => {
            let _ = app.emit("dictation-error", &error);
            return Err(error);
        }
    };
    if let Some(sender) = recording_ready {
        let _ = sender.send(());
    }

    let _ = app.emit("dictation-started", ());
    let _ = app.emit("dictation-status", "Recording one evaluation utterance...");
    let outcome = run_evaluation_session(&state, &app, &config, &session).await;
    finish_recording(&state, session.id).await;

    match outcome {
        Ok(result) => {
            startup_log(format!("evaluation run {} completed", result.run_number));
            let _ = app.emit("dictation-finished", ());
            Ok(result)
        }
        Err(error) => {
            startup_log(format!("evaluation run failed: {}", error));
            let _ = app.emit("dictation-error", &error);
            Err(error)
        }
    }
}

#[tauri::command]
async fn test_microphone(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let session = begin_recording(state.inner(), false).await?;

    let _ = app.emit("mic-test-started", ());

    let recorder = AudioRecorder::new();
    let result = recorder.probe_microphone();

    finish_recording(state.inner(), session.id).await;

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
async fn set_evaluation_settings(
    state: State<'_, AppState>,
    enabled: bool,
    expected_text: Option<String>,
) -> Result<EvaluationSession, String> {
    let mut evaluation = state.evaluation.lock().await;
    evaluation.set_settings(enabled, expected_text);
    Ok(evaluation.clone())
}

#[tauri::command]
async fn get_evaluation_session(state: State<'_, AppState>) -> Result<EvaluationSession, String> {
    Ok(state.evaluation.lock().await.clone())
}

#[tauri::command]
async fn clear_evaluation_session(state: State<'_, AppState>) -> Result<EvaluationSession, String> {
    let mut evaluation = state.evaluation.lock().await;
    evaluation.clear();
    Ok(evaluation.clone())
}

#[tauri::command]
async fn export_evaluation_results(state: State<'_, AppState>) -> Result<String, String> {
    let session = state.evaluation.lock().await.clone();
    if session.runs.is_empty() {
        return Err("There are no evaluation runs to export".into());
    }

    let json = serde_json::to_string_pretty(&session)
        .map_err(|error| format!("Unable to serialize evaluation JSON: {}", error))?;
    let csv = session_to_csv(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let Some(mut json_path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("voca-evaluation-session.json")
            .save_file()
        else {
            return Ok("Export cancelled".to_string());
        };
        if json_path.extension().is_none() {
            json_path.set_extension("json");
        }
        let mut csv_path = json_path.clone();
        csv_path.set_extension("csv");

        std::fs::write(&json_path, json)
            .map_err(|error| format!("Unable to write JSON export: {}", error))?;
        std::fs::write(&csv_path, csv)
            .map_err(|error| format!("Unable to write CSV export: {}", error))?;
        Ok(format!(
            "Exported {} and {}",
            json_path.display(),
            csv_path.display()
        ))
    })
    .await
    .map_err(|error| format!("Export task failed: {}", error))?
}

#[tauri::command]
async fn toggle_dictation(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let action = {
        let evaluation = state.evaluation.lock().await;
        hotkey_action(&evaluation)
    };

    match action {
        HotkeyAction::NormalDictation => {
            run_dictation(state.inner().clone(), app, None).await?;
        }
        HotkeyAction::Evaluation => {
            run_evaluation(state.inner().clone(), app, None).await?;
        }
    }

    Ok(())
}

#[tauri::command]
async fn is_recording(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.recording.lock().await.is_active())
}

#[tauri::command]
async fn reset_recording(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    if request_stop_recording(state.inner()).await {
        let _ = app.emit(
            "dictation-status",
            "Stop requested. Waiting for the active recording to shut down...",
        );
        Ok("recording-stop-requested".into())
    } else {
        Ok("no-active-recording".into())
    }
}

#[tauri::command]
async fn stop_recording(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    if request_stop_recording(state.inner()).await {
        let _ = app.emit("dictation-status", "Stopping recording...");
        Ok("recording-stop-requested".into())
    } else {
        Ok("no-active-recording".into())
    }
}

#[tauri::command]
async fn reveal_window(app: tauri::AppHandle) -> Result<String, String> {
    reveal_main_window(&app);
    Ok("window-revealed".into())
}

pub fn run() {
    let config = Config::load("voca");
    let hotkey_str = config
        .hotkey
        .clone()
        .unwrap_or_else(|| "CmdOrCtrl+Shift+S".into());

    let state = AppState {
        provider: Arc::new(Mutex::new(
            build_provider(&config).expect("shipped provider configuration must be valid"),
        )),
        config: Arc::new(Mutex::new(config)),
        recording: Arc::new(Mutex::new(RecordingLifecycle::default())),
        next_recording_id: Arc::new(AtomicU64::new(1)),
        hotkey_status: Arc::new(Mutex::new("Hotkey registration pending...".into())),
        evaluation: Arc::new(Mutex::new(EvaluationSession::default())),
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
            set_evaluation_settings,
            get_evaluation_session,
            clear_evaluation_session,
            export_evaluation_results,
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
                            startup_log("hook receiver: Pressed");
                            let (recording_ready_tx, recording_ready_rx) = mpsc::channel();
                            tauri::async_runtime::spawn(async move {
                                let action = {
                                    let evaluation = state.evaluation.lock().await;
                                    hotkey_action(&evaluation)
                                };
                                match action {
                                    HotkeyAction::Evaluation => {
                                        let _ = run_evaluation(
                                            state,
                                            app_clone,
                                            Some(recording_ready_tx),
                                        )
                                        .await;
                                    }
                                    HotkeyAction::NormalDictation => {
                                        let _ = run_dictation(
                                            state,
                                            app_clone,
                                            Some(recording_ready_tx),
                                        )
                                        .await;
                                    }
                                }
                            });
                            // Do not consume the corresponding release until the recording
                            // lifecycle owns this accepted hotkey press (or startup fails).
                            let _ = recording_ready_rx.recv();
                        }
                        keyboard_hook::HookEvent::Released => {
                            startup_log("hook receiver: Released");
                            tauri::async_runtime::spawn(async move {
                                if request_stop_recording(&state).await {
                                    let _ =
                                        app_clone.emit("dictation-status", "Stopping recording...");
                                }
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
    use super::{
        append_sarvam_chunk, create_recording_temp_dir, hotkey_action, merge_config_update,
        HotkeyAction, RecordingLifecycle, RecordingPhase,
    };
    use crate::config::Config;
    use crate::evaluation::EvaluationSession;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn recording_lifecycle_keeps_stop_signal_set_until_owner_finishes() {
        let stop_signal = Arc::new(AtomicBool::new(false));
        let mut lifecycle = RecordingLifecycle::default();
        let session = lifecycle.begin(1, stop_signal.clone(), Some(123)).unwrap();

        assert!(lifecycle.request_stop());
        assert!(stop_signal.load(Ordering::Acquire));
        assert_eq!(
            lifecycle.active.as_ref().map(|active| active.phase),
            Some(RecordingPhase::Stopping)
        );
        assert!(lifecycle
            .begin(2, Arc::new(AtomicBool::new(false)), Some(456))
            .is_err());
        assert!(!lifecycle.finish(2));
        assert!(lifecycle.is_active());
        assert!(stop_signal.load(Ordering::Acquire));

        assert!(lifecycle.finish(session.id));
        assert!(!lifecycle.is_active());
        assert!(lifecycle
            .begin(2, Arc::new(AtomicBool::new(false)), Some(456))
            .is_ok());
    }

    #[test]
    fn evaluation_off_routes_hotkey_to_normal_dictation() {
        assert_eq!(
            hotkey_action(&EvaluationSession::default()),
            HotkeyAction::NormalDictation
        );
        assert_eq!(
            hotkey_action(&EvaluationSession {
                enabled: true,
                ..Default::default()
            }),
            HotkeyAction::Evaluation
        );
    }

    #[test]
    fn sarvam_chunk_join_preserves_each_response_and_only_adds_needed_separator() {
        let mut assembled = String::new();
        append_sarvam_chunk(&mut assembled, " first ");
        append_sarvam_chunk(&mut assembled, "second");
        append_sarvam_chunk(&mut assembled, "third");

        assert_eq!(assembled, " first second third");
    }

    #[test]
    fn recording_temp_directories_are_unique_and_removed_on_drop() {
        let first = create_recording_temp_dir().unwrap();
        let second = create_recording_temp_dir().unwrap();
        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();
        assert_ne!(first_path, second_path);

        std::fs::write(first_path.join("recording.wav"), b"temporary audio").unwrap();
        drop(first);

        assert!(!first_path.exists());
        assert!(second_path.exists());
    }

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
