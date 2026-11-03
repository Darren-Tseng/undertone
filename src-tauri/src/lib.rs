mod audio;
mod models;
mod paste;
mod settings;
mod sidecar;
mod transcripts;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub struct AppState {
    settings: Mutex<settings::Settings>,
    recorder: Mutex<Option<audio::Recording>>,
    level: Arc<AtomicU32>,
    busy: AtomicBool,
    session: AtomicU64,
    sidecar: sidecar::Sidecar,
    hotkey_error: Mutex<Option<String>>,
}

impl AppState {
    fn new(app: &AppHandle) -> Self {
        Self {
            settings: Mutex::new(settings::load(app)),
            recorder: Mutex::new(None),
            level: Arc::new(AtomicU32::new(0f32.to_bits())),
            busy: AtomicBool::new(false),
            session: AtomicU64::new(0),
            sidecar: sidecar::Sidecar::new(),
            hotkey_error: Mutex::new(None),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeState {
    recording: bool,
    busy: bool,
    elapsed: f64,
    level: f32,
    sidecar: sidecar::SidecarStatus,
    hotkey_error: Option<String>,
}

fn emit_state(app: &AppHandle, state: &str, message: Option<String>) {
    let _ = app.emit(
        "murmur://state",
        serde_json::json!({ "state": state, "message": message }),
    );
}

fn current_settings(app: &AppHandle) -> Result<settings::Settings, String> {
    let state = app.state::<AppState>();
    let guard = state
        .settings
        .lock()
        .map_err(|_| "設定鎖定失敗".to_string())?;
    Ok(guard.clone())
}

fn temp_wav_path() -> PathBuf {
    std::env::temp_dir().join("murmur").join(format!(
        "rec-{}.wav",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")
    ))
}

fn apply_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let shortcut: Shortcut = hotkey
        .parse()
        .map_err(|err| format!("快捷鍵格式錯誤：{err}"))?;
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    manager
        .register(shortcut)
        .map_err(|err| format!("無法註冊快捷鍵（可能被其他程式占用）：{err}"))
}

fn show_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        if let Ok(Some(monitor)) = window.current_monitor() {
            let scale = monitor.scale_factor();
            let monitor_size = monitor.size();
            let width = (340.0 * scale) as i32;
            let height = (100.0 * scale) as i32;
            let x = (monitor_size.width as i32 - width) / 2;
            let y = monitor_size.height as i32 - height - (96.0 * scale) as i32;
            let _ = window.set_position(tauri::PhysicalPosition::new(x.max(0), y.max(0)));
        }
        let _ = window.show();
    }
}

fn hide_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
}

fn begin_recording(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    {
        let recorder = state.recorder.lock().map_err(|_| "錄音狀態鎖定失敗")?;
        if recorder.is_some() {
            return Ok(());
        }
    }

    if state.busy.load(Ordering::SeqCst) {
        return Err("上一段語音還在轉錄中，請稍候".to_string());
    }

    let current = { state.settings.lock().map_err(|_| "設定鎖定失敗")?.clone() };

    let recording = audio::start(
        current.input_device.clone(),
        temp_wav_path(),
        state.level.clone(),
    )?;

    {
        let mut recorder = state.recorder.lock().map_err(|_| "錄音狀態鎖定失敗")?;
        *recorder = Some(recording);
    }

    show_overlay(app);
    emit_state(app, "recording", None);

    // Safety net: never record forever. Tag the session so a stale timer
    // cannot stop a newer recording.
    let session_id = state.session.fetch_add(1, Ordering::SeqCst) + 1;
    let app_handle = app.clone();
    let max_secs = current.max_record_secs.clamp(5, 3600) as u64;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(max_secs));
        let state = app_handle.state::<AppState>();
        if state.session.load(Ordering::SeqCst) != session_id {
            return;
        }
        let still_recording = state
            .recorder
            .lock()
            .map(|recorder| recorder.is_some())
            .unwrap_or(false);
        if still_recording {
            stop_and_transcribe(app_handle.clone());
        }
    });

    Ok(())
}

fn toggle_dictation(app: AppHandle) {
    let state = app.state::<AppState>();
    let recording = state
        .recorder
        .lock()
        .map(|recorder| recorder.is_some())
        .unwrap_or(false);

    if recording {
        stop_and_transcribe(app);
    } else if state.busy.load(Ordering::SeqCst) {
        emit_state(&app, "busy", Some("正在轉錄，請稍候…".to_string()));
    } else if let Err(err) = begin_recording(&app) {
        emit_state(&app, "error", Some(err));
    }
}

fn stop_and_transcribe(app: AppHandle) {
    let state = app.state::<AppState>();
    let recording = state
        .recorder
        .lock()
        .ok()
        .and_then(|mut recorder| recorder.take());

    let Some(recording) = recording else {
        return;
    };

    state.busy.store(true, Ordering::SeqCst);
    emit_state(&app, "transcribing", None);

    tauri::async_runtime::spawn(async move {
        let result = transcribe_recording(&app, recording).await;
        let state = app.state::<AppState>();
        state.busy.store(false, Ordering::SeqCst);

        match result {
            Ok(entry) => {
                let _ = app.emit("murmur://transcript", &entry);
                emit_state(&app, "done", Some(entry.text.clone()));

                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1400)).await;
                    let state = app_handle.state::<AppState>();
                    let busy = state.busy.load(Ordering::SeqCst);
                    let recording = state
                        .recorder
                        .lock()
                        .map(|recorder| recorder.is_some())
                        .unwrap_or(false);
                    if !busy && !recording {
                        hide_overlay(&app_handle);
                        emit_state(&app_handle, "idle", None);
                    }
                });
            }
            Err(err) => {
                emit_state(&app, "error", Some(err));

                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(2800)).await;
                    let still_busy = app_handle.state::<AppState>().busy.load(Ordering::SeqCst);
                    if !still_busy {
                        hide_overlay(&app_handle);
                        emit_state(&app_handle, "idle", None);
                    }
                });
            }
        }
    });
}

async fn transcribe_recording(
    app: &AppHandle,
    recording: audio::Recording,
) -> Result<transcripts::TranscriptEntry, String> {
    let state = app.state::<AppState>();
    let current = { state.settings.lock().map_err(|_| "設定鎖定失敗")?.clone() };

    let recorded = tauri::async_runtime::spawn_blocking(move || recording.stop())
        .await
        .map_err(|err| format!("錄音執行緒失敗：{err}"))??;

    let wav = std::fs::read(&recorded.path).map_err(|err| format!("讀取錄音失敗：{err}"))?;
    let _ = std::fs::remove_file(&recorded.path);

    if recorded.duration < 0.35 {
        return Err("錄音時間太短".to_string());
    }

    state.sidecar.ensure_started(app, &current).await?;
    let result = state.sidecar.transcribe(wav, &current).await?;
    let text = result.text.trim().to_string();
    if text.is_empty() {
        return Err("沒有辨識到語音".to_string());
    }

    let language = result
        .language
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| current.language.clone());

    let entry = transcripts::save(
        &current,
        transcripts::NewTranscript {
            text: &text,
            duration: recorded.duration,
            model: &current.model,
            language: &language,
        },
    )?;

    if current.copy_to_clipboard {
        let _ = paste::copy(&text);
    }

    if current.auto_paste {
        let text = text.clone();
        if let Ok(Err(err)) =
            tauri::async_runtime::spawn_blocking(move || paste::paste(&text)).await
        {
            eprintln!("[murmur] paste failed: {err}");
        }
    }

    Ok(entry)
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Result<settings::Settings, String> {
    state
        .settings
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| "設定鎖定失敗".to_string())
}

#[tauri::command]
async fn save_settings(
    app: AppHandle,
    settings: settings::Settings,
) -> Result<settings::Settings, String> {
    let state = app.state::<AppState>();
    let mut next = settings;

    next.hotkey = next.hotkey.trim().to_string();
    next.model = next.model.trim().to_string();
    next.workspace_dir = next.workspace_dir.trim().to_string();
    next.sidecar_port = next.sidecar_port.max(1024);
    next.max_record_secs = next.max_record_secs.clamp(5, 3600);

    if next.hotkey.is_empty() {
        return Err("快捷鍵不可為空".to_string());
    }
    if next.model.is_empty() {
        next.model = "small".to_string();
    }
    if next.workspace_dir.is_empty() {
        return Err("工作區路徑不可為空".to_string());
    }

    let previous = {
        state
            .settings
            .lock()
            .map_err(|_| "設定鎖定失敗".to_string())?
            .clone()
    };

    apply_hotkey(&app, &next.hotkey).map_err(|err| {
        // Do not leave the user without a shortcut if the new one is taken.
        let _ = apply_hotkey(&app, &previous.hotkey);
        err
    })?;
    *state
        .hotkey_error
        .lock()
        .map_err(|_| "快捷鍵狀態鎖定失敗".to_string())? = None;

    crate::settings::save(&app, &next)?;
    *state
        .settings
        .lock()
        .map_err(|_| "設定鎖定失敗".to_string())? = next.clone();

    if previous.sidecar_port != next.sidecar_port {
        state.sidecar.shutdown();
        let app_handle = app.clone();
        let next_for_task = next.clone();
        tauri::async_runtime::spawn(async move {
            let state = app_handle.state::<AppState>();
            match state
                .sidecar
                .ensure_started(&app_handle, &next_for_task)
                .await
            {
                Ok(()) => state.sidecar.warmup(&next_for_task).await,
                Err(err) => state.sidecar.set_status("error", err),
            }
        });
    } else if previous.model != next.model
        || previous.device != next.device
        || previous.compute_type != next.compute_type
    {
        let app_handle = app.clone();
        let next_for_task = next.clone();
        tauri::async_runtime::spawn(async move {
            app_handle
                .state::<AppState>()
                .sidecar
                .warmup(&next_for_task)
                .await;
        });
    }

    Ok(next)
}

#[tauri::command]
fn get_runtime_state(app: AppHandle) -> Result<RuntimeState, String> {
    let state = app.state::<AppState>();
    let current = state
        .settings
        .lock()
        .map_err(|_| "設定鎖定失敗".to_string())?
        .clone();

    let (recording, elapsed) = state
        .recorder
        .lock()
        .map(|recorder| match recorder.as_ref() {
            Some(recording) => (true, recording.elapsed()),
            None => (false, 0.0),
        })
        .unwrap_or((false, 0.0));

    Ok(RuntimeState {
        recording,
        busy: state.busy.load(Ordering::SeqCst),
        elapsed,
        level: f32::from_bits(state.level.load(Ordering::Relaxed)),
        sidecar: state.sidecar.status(current.sidecar_port),
        hotkey_error: state
            .hotkey_error
            .lock()
            .ok()
            .and_then(|value| value.clone()),
    })
}

#[tauri::command]
fn start_dictation(app: AppHandle) -> Result<(), String> {
    begin_recording(&app).map_err(|err| {
        emit_state(&app, "error", Some(err.clone()));
        err
    })
}

#[tauri::command]
fn stop_dictation(app: AppHandle) {
    stop_and_transcribe(app);
}

#[tauri::command]
fn cancel_dictation(app: AppHandle) {
    let state = app.state::<AppState>();
    let recording = state
        .recorder
        .lock()
        .ok()
        .and_then(|mut recorder| recorder.take());

    if let Some(recording) = recording {
        std::thread::spawn(move || {
            if let Ok(recorded) = recording.stop() {
                let _ = std::fs::remove_file(recorded.path);
            }
        });
    }

    hide_overlay(&app);
    emit_state(&app, "idle", None);
}

#[tauri::command]
async fn sidecar_status(app: AppHandle) -> Result<sidecar::SidecarStatus, String> {
    let state = app.state::<AppState>();
    let current = {
        state
            .settings
            .lock()
            .map_err(|_| "設定鎖定失敗".to_string())?
            .clone()
    };
    let sidecar = &state.sidecar;

    match sidecar::health(current.sidecar_port).await {
        Some(health) => {
            sidecar.set_active(&current.model, &current.device, &current.compute_type);
            sidecar.update_health(&health);
            let loading_label = health
                .get("loadingModels")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_str());
            let current_loaded = health
                .get("loadedModels")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items.iter().any(|value| {
                        value.as_str().map(|label| {
                            label == current.model
                                || label.starts_with(&format!("{} (", current.model))
                        }).unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            if let Some(label) = loading_label {
                let model = label
                    .split(" (")
                    .next()
                    .unwrap_or(current.model.as_str());
                sidecar.set_status(
                    "loading",
                    format!("正在下載／載入 {model} 模型…"),
                );
            } else if current_loaded {
                sidecar.set_status("ready", format!("{} 模型已就緒", current.model));
            } else {
                sidecar.set_status("ready", "轉錄引擎就緒");
            }
        }
        None => {
            let status = sidecar.status(current.sidecar_port);
            // A slow health check during model loading is not a failure.
            if status.state == "ready" || status.state == "stopped" {
                sidecar.set_status("stopped", "轉錄引擎未正常回應");
            }
        }
    }

    Ok(sidecar.status(current.sidecar_port))
}

#[tauri::command]
async fn restart_sidecar(app: AppHandle) -> Result<sidecar::SidecarStatus, String> {
    let current = current_settings(&app)?;

    {
        let state = app.state::<AppState>();
        state.sidecar.shutdown();
        state.sidecar.ensure_started(&app, &current).await?;
    }

    let app_handle = app.clone();
    let current_for_task = current.clone();
    tauri::async_runtime::spawn(async move {
        app_handle
            .state::<AppState>()
            .sidecar
            .warmup(&current_for_task)
            .await;
    });

    Ok(app.state::<AppState>().sidecar.status(current.sidecar_port))
}

#[tauri::command]
async fn list_models(app: AppHandle) -> Result<models::ModelsOverview, String> {
    let current = current_settings(&app)?;
    let state = app.state::<AppState>();

    if let Some(health) = sidecar::health(current.sidecar_port).await {
        state
            .sidecar
            .set_active(&current.model, &current.device, &current.compute_type);
        state.sidecar.update_health(&health);
    }

    let status = state.sidecar.status(current.sidecar_port);
    Ok(models::list(
        &app,
        &status.details.loaded_models,
        &status.details.loading_models,
        &current.model,
    ))
}

#[tauri::command]
async fn delete_model(app: AppHandle, id: String) -> Result<(), String> {
    let current = current_settings(&app)?;
    if id == current.model {
        return Err("目前設定使用的模型無法刪除，請先切換到其他模型。".to_string());
    }

    let state = app.state::<AppState>();
    if let Some(health) = sidecar::health(current.sidecar_port).await {
        state.sidecar.update_health(&health);
    }
    let status = state.sidecar.status(current.sidecar_port);
    let in_use = status.details.loaded_models.iter().any(|label| {
        label == &id || label.starts_with(&format!("{id} ("))
    });
    if in_use {
        return Err("模型正在引擎記憶體中使用中，請先切換模型或重新啟動引擎。".to_string());
    }

    models::delete(&app, &id)
}

#[tauri::command]
fn open_models_dir(app: AppHandle) -> Result<(), String> {
    models::open_dir(&app)
}

#[tauri::command]
async fn prepare_model(
    app: AppHandle,
    model: String,
) -> Result<sidecar::SidecarStatus, String> {
    if !models::is_known(&model) {
        return Err(format!("未知的模型：{model}"));
    }

    let current = current_settings(&app)?;
    let state = app.state::<AppState>();
    state.sidecar.ensure_started(&app, &current).await?;

    let mut target = current.clone();
    target.model = model.clone();
    state.sidecar.warmup(&target).await;

    // Preloading a model does not change the configured dictation model.
    state
        .sidecar
        .set_active(&current.model, &current.device, &current.compute_type);
    if let Some(health) = sidecar::health(current.sidecar_port).await {
        state.sidecar.update_health(&health);
    }

    let status = state.sidecar.status(current.sidecar_port);
    if status.state == "error" {
        return Err(status.message);
    }

    let loaded = status.details.loaded_models.iter().any(|label| {
        label == &model || label.starts_with(&format!("{model} ("))
    });
    if !loaded {
        return Err(format!("{model} 模型尚未完成載入，請稍後再試。"));
    }

    Ok(status)
}

#[tauri::command]
fn list_input_devices() -> Result<Vec<String>, String> {
    audio::input_devices()
}

#[tauri::command]
fn list_transcripts(
    app: AppHandle,
    query: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<transcripts::TranscriptEntry>, String> {
    let current = current_settings(&app)?;
    Ok(transcripts::list(
        &current,
        query.as_deref().unwrap_or(""),
        limit.unwrap_or(200).clamp(1, 1000),
    ))
}

#[tauri::command]
fn delete_transcript(app: AppHandle, path: String) -> Result<(), String> {
    let current = current_settings(&app)?;
    transcripts::delete(&current, &path)
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    paste::copy(&text)
}

#[tauri::command]
fn reveal_transcript(path: String) -> Result<(), String> {
    transcripts::reveal(&path)
}

#[tauri::command]
fn open_workspace(app: AppHandle) -> Result<(), String> {
    let current = current_settings(&app)?;
    transcripts::open_workspace(&current)
}

fn focus_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "開啟 Murmur", true, None::<&str>)?;
    let toggle_item = MenuItem::with_id(app, "toggle", "開始／停止聽寫", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "結束", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &toggle_item, &quit_item])?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("Murmur 語音聽寫")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => focus_main(app),
            "toggle" => toggle_dictation(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                focus_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

fn setup_sidecar(app: &tauri::AppHandle) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let current = match current_settings(&app_handle) {
            Ok(settings) => settings,
            Err(_) => return,
        };

        let state = app_handle.state::<AppState>();
        state.sidecar.set_status("starting", "正在啟動轉錄引擎…");

        match state.sidecar.ensure_started(&app_handle, &current).await {
            Ok(()) => state.sidecar.warmup(&current).await,
            Err(err) => state.sidecar.set_status("error", err),
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_dictation(app.clone());
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_runtime_state,
            start_dictation,
            stop_dictation,
            cancel_dictation,
            sidecar_status,
            restart_sidecar,
            list_models,
            delete_model,
            open_models_dir,
            prepare_model,
            list_input_devices,
            list_transcripts,
            delete_transcript,
            copy_text,
            reveal_transcript,
            open_workspace,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(AppState::new(&handle));

            let overlay = tauri::WebviewWindowBuilder::new(
                app,
                "overlay",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Murmur")
            .inner_size(340.0, 100.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .visible(false)
            .build();

            if let Err(err) = overlay {
                eprintln!("[murmur] failed to build overlay window: {err}");
            }

            let hotkey = {
                let state = handle.state::<AppState>();
                state
                    .settings
                    .lock()
                    .map(|settings| settings.hotkey.clone())
                    .unwrap_or_else(|_| "Ctrl+Alt+Space".to_string())
            };

            if let Err(err) = apply_hotkey(&handle, &hotkey) {
                eprintln!("[murmur] hotkey registration failed: {err}");
                if let Ok(mut slot) = handle.state::<AppState>().hotkey_error.lock() {
                    *slot = Some(err);
                }
            }

            setup_tray(app)?;
            setup_sidecar(&handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Murmur");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<AppState>() {
                state.sidecar.shutdown();
            }
        }
    });
}
