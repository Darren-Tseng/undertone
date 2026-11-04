use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CudaOverview {
    /// Folder that holds the runtime-downloaded CUDA DLLs.
    pub dir: String,
    /// Total size of the `.dll` files inside `dir`.
    pub installed_bytes: u64,
    pub files: u64,
}

/// Matches `UNDERTONE_CUDA_DIR` handed to the sidecar in `sidecar.rs`.
pub fn dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("cuda"))
        .map_err(|e| format!("找不到 GPU 元件資料夾：{e}"))
}

/// Only `.dll` files count, so the numbers match the sidecar's `/cuda`.
fn scan(path: &Path) -> (u64, u64) {
    let Ok(entries) = fs::read_dir(path) else {
        return (0, 0);
    };
    let mut files = 0;
    let mut bytes = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let (child_files, child_bytes) = scan(&entry.path());
            files += child_files;
            bytes += child_bytes;
        } else if entry
            .file_name()
            .to_string_lossy()
            .to_lowercase()
            .ends_with(".dll")
        {
            if let Ok(meta) = entry.metadata() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}

pub fn overview(app: &tauri::AppHandle) -> CudaOverview {
    let path = dir(app).ok();
    let (files, installed_bytes) = path.as_deref().map(scan).unwrap_or((0, 0));

    CudaOverview {
        dir: path
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default(),
        installed_bytes,
        files,
    }
}

pub fn open_dir(app: &tauri::AppHandle) -> Result<(), String> {
    let dir = dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| format!("無法建立 GPU 元件資料夾：{e}"))?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("無法開啟 GPU 元件資料夾：{e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("無法開啟 GPU 元件資料夾：{e}"))?;
    }
    Ok(())
}
