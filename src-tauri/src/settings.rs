use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Global shortcut in the `Ctrl+Alt+Space` format.
    pub hotkey: String,
    /// tiny | base | small | medium | large-v3 | distil-large-v3
    pub model: String,
    /// auto | zh | en | ja | ...
    pub language: String,
    /// auto | cpu | cuda
    pub device: String,
    /// auto | int8 | int8_float16 | float16 | float32
    pub compute_type: String,
    pub auto_paste: bool,
    pub copy_to_clipboard: bool,
    /// When false, transcriptions are pasted/copied but not written to disk.
    pub save_transcripts: bool,
    pub max_record_secs: u32,
    /// Minutes of GPU inactivity before the engine releases VRAM (0 = never).
    pub gpu_idle_minutes: u32,
    pub input_device: Option<String>,
    pub workspace_dir: String,
    pub sidecar_port: u16,
    pub initial_prompt: String,
    /// Optional override for the Python interpreter.
    pub python_path: String,
    /// auto | light | dark
    pub theme: String,
    /// ask | minimize | quit — what the main window's close button does.
    pub close_action: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Alt+Space".into(),
            model: "small".into(),
            language: "zh".into(),
            device: "auto".into(),
            compute_type: "auto".into(),
            auto_paste: true,
            copy_to_clipboard: true,
            save_transcripts: true,
            max_record_secs: 300,
            gpu_idle_minutes: 10,
            input_device: None,
            workspace_dir: String::new(),
            sidecar_port: 8756,
            initial_prompt: "以下是繁體中文的逐字稿，請使用繁體中文與正確的標點符號。".into(),
            python_path: String::new(),
            theme: "auto".into(),
            close_action: "ask".into(),
        }
    }
}

pub fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("找不到設定資料夾：{e}"))
}

fn settings_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("settings.json"))
}

pub fn load(app: &tauri::AppHandle) -> Settings {
    let mut settings: Settings = settings_file(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();

    if settings.workspace_dir.trim().is_empty() {
        settings.workspace_dir = app
            .path()
            .home_dir()
            .map(|home| home.join("Undertone"))
            .unwrap_or_else(|_| PathBuf::from("Undertone"))
            .to_string_lossy()
            .to_string();
    }

    if settings.max_record_secs == 0 {
        settings.max_record_secs = 300;
    }

    if settings.gpu_idle_minutes > 1440 {
        settings.gpu_idle_minutes = 1440;
    }

    if !matches!(settings.close_action.as_str(), "ask" | "minimize" | "quit") {
        settings.close_action = "ask".to_string();
    }

    settings
}

pub fn save(app: &tauri::AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_file(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("無法建立設定資料夾：{e}"))?;
    }
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| format!("無法寫入設定：{e}"))
}
