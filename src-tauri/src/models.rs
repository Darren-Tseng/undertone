use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Metadata for the models faster-whisper knows how to download.
struct ModelSpec {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    approx_bytes: u64,
    speed: u8,
    accuracy: u8,
    recommended: bool,
    multilingual: bool,
}

const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "tiny",
        label: "Tiny",
        description: "最快、最省資源，適合短句與低階硬體。",
        approx_bytes: 75 * 1024 * 1024,
        speed: 5,
        accuracy: 1,
        recommended: false,
        multilingual: true,
    },
    ModelSpec {
        id: "base",
        label: "Base",
        description: "比 Tiny 準一點，仍然非常快。",
        approx_bytes: 145 * 1024 * 1024,
        speed: 4,
        accuracy: 2,
        recommended: false,
        multilingual: true,
    },
    ModelSpec {
        id: "small",
        label: "Small",
        description: "速度與準確度的平衡，多數情況建議使用。",
        approx_bytes: 480 * 1024 * 1024,
        speed: 3,
        accuracy: 3,
        recommended: true,
        multilingual: true,
    },
    ModelSpec {
        id: "medium",
        label: "Medium",
        description: "更準確，需要較多記憶體與運算時間。",
        approx_bytes: 1536 * 1024 * 1024,
        speed: 2,
        accuracy: 4,
        recommended: false,
        multilingual: true,
    },
    ModelSpec {
        id: "large-v3",
        label: "Large v3",
        description: "最高準確度，建議搭配 NVIDIA GPU。",
        approx_bytes: 3072 * 1024 * 1024,
        speed: 1,
        accuracy: 5,
        recommended: false,
        multilingual: true,
    },
    ModelSpec {
        id: "distil-large-v3",
        label: "Distil Large v3",
        description: "以英文為主，速度快、品質接近 Large。",
        approx_bytes: 1536 * 1024 * 1024,
        speed: 4,
        accuracy: 4,
        recommended: false,
        multilingual: false,
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub approx_bytes: u64,
    pub speed: u8,
    pub accuracy: u8,
    pub recommended: bool,
    pub multilingual: bool,
    pub downloaded: bool,
    pub bytes: u64,
    pub paths: Vec<String>,
    pub loaded: bool,
    pub loading: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsOverview {
    pub models: Vec<ModelInfo>,
    pub cache_dir: String,
    pub total_bytes: u64,
    pub downloaded_count: usize,
}

pub fn is_known(id: &str) -> bool {
    CATALOG.iter().any(|spec| spec.id == id)
}

/// Primary model cache, matching `UNDERTONE_MODEL_DIR` used by the sidecar.
pub fn cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("models"))
        .map_err(|e| format!("找不到模型資料夾：{e}"))
}

fn cache_roots(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(dir) = cache_dir(app) {
        roots.push(dir);
    }

    // Older runs (or `download_model.py` without UNDERTONE_MODEL_DIR) may have
    // used the default Hugging Face cache instead.
    if let Ok(home) = app.path().home_dir() {
        roots.push(home.join(".cache").join("huggingface").join("hub"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(
            PathBuf::from(local)
                .join("huggingface")
                .join("hub"),
        );
    }
    if let Ok(hf_home) = std::env::var("HF_HOME") {
        roots.push(PathBuf::from(hf_home).join("hub"));
    }

    roots.sort();
    roots.dedup();
    roots
}

fn dir_matches(name: &str, id: &str) -> bool {
    let name = name.to_lowercase();
    let id = id.to_lowercase();
    name == id
        || name.ends_with(&format!("faster-whisper-{id}"))
        || name.ends_with(&format!("faster-distil-whisper-{id}"))
        || name.ends_with(&format!("--{id}"))
}

fn find_model_dirs(app: &tauri::AppHandle, id: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in cache_roots(app) {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let is_dir = entry
                .file_type()
                .map(|kind| kind.is_dir())
                .unwrap_or(false);
            if !is_dir {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if dir_matches(&name, id) {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            total += dir_size(&entry.path());
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

fn label_matches(label: &str, id: &str) -> bool {
    label == id || label.starts_with(&format!("{id} ("))
}

pub fn list(
    app: &tauri::AppHandle,
    loaded: &[String],
    loading: &[String],
    active: &str,
) -> ModelsOverview {
    let mut models = Vec::with_capacity(CATALOG.len());
    let mut total_bytes = 0;
    let mut downloaded_count = 0;

    for spec in CATALOG {
        let paths = find_model_dirs(app, spec.id);
        let bytes: u64 = paths.iter().map(|path| dir_size(path)).sum();
        let downloaded = !paths.is_empty() && bytes > 0;
        if downloaded {
            downloaded_count += 1;
            total_bytes += bytes;
        }

        models.push(ModelInfo {
            id: spec.id.to_string(),
            label: spec.label.to_string(),
            description: spec.description.to_string(),
            approx_bytes: spec.approx_bytes,
            speed: spec.speed,
            accuracy: spec.accuracy,
            recommended: spec.recommended,
            multilingual: spec.multilingual,
            downloaded,
            bytes,
            paths: paths
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            loaded: loaded.iter().any(|label| label_matches(label, spec.id)),
            loading: loading.iter().any(|label| label_matches(label, spec.id)),
            active: spec.id == active,
        });
    }

    ModelsOverview {
        models,
        cache_dir: cache_dir(app)
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default(),
        total_bytes,
        downloaded_count,
    }
}

pub fn delete(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    if !is_known(id) {
        return Err(format!("未知的模型：{id}"));
    }

    let roots = cache_roots(app);
    let dirs = find_model_dirs(app, id);
    if dirs.is_empty() {
        return Err("找不到已下載的模型檔案。".to_string());
    }

    for dir in dirs {
        // Never remove anything outside a known Hugging Face model cache.
        if !roots.iter().any(|root| dir.starts_with(root)) {
            continue;
        }
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("無法刪除模型檔案（{}）：{e}", dir.display()))?;
    }

    Ok(())
}

pub fn open_dir(app: &tauri::AppHandle) -> Result<(), String> {
    let dir = cache_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| format!("無法建立模型資料夾：{e}"))?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("無法開啟模型資料夾：{e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("無法開啟模型資料夾：{e}"))?;
    }
    Ok(())
}
