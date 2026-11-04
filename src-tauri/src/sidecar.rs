use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

const MAX_LOG_LINES: usize = 300;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastTranscription {
    pub at: String,
    pub model: String,
    pub device: String,
    pub compute_type: String,
    pub language: Option<String>,
    pub audio_seconds: f64,
    pub elapsed_seconds: f64,
    pub realtime_factor: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineDetails {
    pub pid: Option<u32>,
    pub python: Option<String>,
    pub faster_whisper: Option<String>,
    pub cuda_devices: u32,
    pub uptime: Option<f64>,
    pub loaded_models: Vec<String>,
    pub loading_models: Vec<String>,
    pub last_error: Option<String>,
    pub cuda_installing: bool,
    pub active_model: Option<String>,
    pub device: Option<String>,
    pub compute_type: Option<String>,
    pub total_transcriptions: u64,
    pub total_audio_seconds: f64,
    pub total_transcribe_seconds: f64,
    pub last_transcription: Option<LastTranscription>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarStatus {
    pub state: String,
    pub message: String,
    pub port: u16,
    pub log: String,
    pub details: EngineDetails,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Transcription {
    pub text: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub elapsed: Option<f64>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub compute_type: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

pub struct Sidecar {
    child: Mutex<Option<Child>>,
    logs: Arc<Mutex<VecDeque<String>>>,
    status: Mutex<(String, String)>,
    details: Mutex<EngineDetails>,
    starting: AtomicBool,
}

impl Sidecar {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            status: Mutex::new(("stopped".into(), "轉錄引擎尚未啟動".into())),
            details: Mutex::new(EngineDetails::default()),
            starting: AtomicBool::new(false),
        }
    }

    fn push_log(&self, line: String) {
        let line = line.trim_end().to_string();
        if line.is_empty() {
            return;
        }
        eprintln!("[sidecar] {line}");
        if let Ok(mut logs) = self.logs.lock() {
            logs.push_back(line);
            while logs.len() > MAX_LOG_LINES {
                logs.pop_front();
            }
        }
    }

    pub fn log_tail(&self, lines: usize) -> String {
        self.logs
            .lock()
            .map(|logs| {
                let skip = logs.len().saturating_sub(lines);
                logs.iter()
                    .skip(skip)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    pub fn set_status(&self, state: &str, message: impl Into<String>) {
        if let Ok(mut status) = self.status.lock() {
            status.0 = state.to_string();
            status.1 = message.into();
        }
    }

    pub fn status(&self, port: u16) -> SidecarStatus {
        let (state, message) = self
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| ("stopped".into(), String::new()));
        SidecarStatus {
            state,
            message,
            port,
            log: self.log_tail(120),
            details: self
                .details
                .lock()
                .map(|details| details.clone())
                .unwrap_or_default(),
        }
    }

    /// Remember which model the user is targeting, even before it finishes loading.
    pub fn set_active(&self, model: &str, device: &str, compute_type: &str) {
        if let Ok(mut details) = self.details.lock() {
            details.active_model = Some(model.to_string());
            details.device = Some(device.to_string());
            details.compute_type = Some(compute_type.to_string());
        }
    }

    /// Fold the sidecar `/health` payload into the richer status shown in the UI.
    pub fn update_health(&self, health: &serde_json::Value) {
        let Ok(mut details) = self.details.lock() else {
            return;
        };

        details.pid = health
            .get("pid")
            .and_then(|value| value.as_u64())
            .map(|pid| pid as u32);
        details.python = json_string(health, "python");
        details.faster_whisper = json_string(health, "fasterWhisper");
        details.cuda_devices = health
            .get("cudaDevices")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32;
        details.uptime = health.get("uptime").and_then(|value| value.as_f64());
        details.loaded_models = json_strings(health, "loadedModels");
        details.loading_models = json_strings(health, "loadingModels");
        details.last_error = json_string(health, "lastError");
        details.cuda_installing = health
            .get("cudaInstall")
            .and_then(|value| value.get("state"))
            .and_then(|value| value.as_str())
            == Some("running");

        // The loaded label looks like `small (cuda/float16)`; prefer the actual
        // device over the requested one (`auto`). If both a CPU and a CUDA
        // copy are resident (GPU idle release), report the GPU one.
        if let Some(active) = details.active_model.clone() {
            let gpu_label = details
                .loaded_models
                .iter()
                .find(|label| label_matches_model(label, &active) && label.contains("(cuda/"));
            let label = gpu_label.or_else(|| {
                details
                    .loaded_models
                    .iter()
                    .find(|label| label_matches_model(label, &active))
            });
            if let Some(label) = label {
                if let Some((device, compute)) = parse_model_label(label) {
                    details.device = Some(device);
                    details.compute_type = Some(compute);
                }
            }
        }
    }

    /// Keep a small local history of transcription performance for the status panel.
    pub fn record_transcription(&self, result: &Transcription) {
        let audio = result.duration.unwrap_or(0.0).max(0.0);
        let elapsed = result.elapsed.unwrap_or(0.0).max(0.0);
        let realtime_factor = if elapsed > 0.0 { audio / elapsed } else { 0.0 };

        if let Ok(mut details) = self.details.lock() {
            details.total_transcriptions += 1;
            details.total_audio_seconds += audio;
            details.total_transcribe_seconds += elapsed;
            details.last_transcription = Some(LastTranscription {
                at: chrono::Local::now().to_rfc3339(),
                model: result.model.clone().unwrap_or_default(),
                device: result.device.clone().unwrap_or_default(),
                compute_type: result.compute_type.clone().unwrap_or_default(),
                language: result.language.clone(),
                audio_seconds: audio,
                elapsed_seconds: elapsed,
                realtime_factor,
            });

            if let Some(model) = result.model.as_ref().filter(|value| !value.is_empty()) {
                details.active_model = Some(model.clone());
            }
            if let Some(device) = result.device.as_ref().filter(|value| !value.is_empty()) {
                details.device = Some(device.clone());
            }
            if let Some(compute) = result
                .compute_type
                .as_ref()
                .filter(|value| !value.is_empty())
            {
                details.compute_type = Some(compute.clone());
            }
        }
    }

    fn clear_health_details(&self) {
        if let Ok(mut details) = self.details.lock() {
            details.pid = None;
            details.python = None;
            details.faster_whisper = None;
            details.cuda_devices = 0;
            details.uptime = None;
            details.cuda_installing = false;
            details.loaded_models.clear();
            details.loading_models.clear();
        }
    }

    fn kill_child(&self) {
        self.clear_health_details();
        let mut guard = match self.child.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(mut child) = guard.take() {
            // venv python is a launcher that spawns the real interpreter, so
            // kill the whole tree instead of only the direct child.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(0x0800_0000)
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Best-effort kill used when the app exits.
    pub fn shutdown(&self) {
        self.kill_child();
        self.set_status("stopped", "已停止");
    }

    pub async fn ensure_started(
        &self,
        app: &tauri::AppHandle,
        settings: &Settings,
    ) -> Result<(), String> {
        let port = settings.sidecar_port;

        if let Some(health) = health(port).await {
            self.set_active(&settings.model, &settings.device, &settings.compute_type);
            self.update_health(&health);
            self.set_status("ready", "轉錄引擎就緒");
            return Ok(());
        }

        if self.starting.swap(true, Ordering::SeqCst) {
            return if let Some(health) = wait_health(port, 45).await {
                self.set_active(&settings.model, &settings.device, &settings.compute_type);
                self.update_health(&health);
                Ok(())
            } else {
                Err("轉錄引擎仍在啟動中，請稍候再試".to_string())
            };
        }

        let result = self.ensure_started_inner(app, settings).await;
        self.starting.store(false, Ordering::SeqCst);
        result
    }

    async fn ensure_started_inner(
        &self,
        app: &tauri::AppHandle,
        settings: &Settings,
    ) -> Result<(), String> {
        let port = settings.sidecar_port;
        self.set_status("starting", "正在啟動轉錄引擎…");
        self.kill_child();

        let (program, args, cwd) = self.resolve_command(app, settings)?;

        let mut command = Command::new(&program);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONIOENCODING", "utf-8");

        if let Ok(data_dir) = app.path().app_data_dir() {
            let model_dir = data_dir.join("models");
            let _ = std::fs::create_dir_all(&model_dir);
            command.env("UNDERTONE_MODEL_DIR", model_dir);

            // Runtime-downloaded CUDA DLLs (settings → 下載 GPU 元件).
            let cuda_dir = data_dir.join("cuda");
            let _ = std::fs::create_dir_all(&cuda_dir);
            command.env("UNDERTONE_CUDA_DIR", cuda_dir);
        }
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        self.push_log(format!("spawn: {} {}", program.display(), args.join(" ")));

        let mut child = command
            .spawn()
            .map_err(|e| format!("無法啟動轉錄引擎（{}）：{e}", program.display()))?;

        if let Some(stdout) = child.stdout.take() {
            let sidecar = self.clone_handle();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    sidecar.push_log(line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let sidecar = self.clone_handle();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    sidecar.push_log(line);
                }
            });
        }

        if let Ok(mut guard) = self.child.lock() {
            *guard = Some(child);
        }

        let deadline = Instant::now() + Duration::from_secs(45);
        while Instant::now() < deadline {
            if let Some(health) = health(port).await {
                self.set_active(&settings.model, &settings.device, &settings.compute_type);
                self.update_health(&health);
                self.set_status("ready", "轉錄引擎就緒");
                return Ok(());
            }

            let exited = self
                .child
                .lock()
                .map(|mut guard| {
                    guard
                        .as_mut()
                        .and_then(|child| child.try_wait().ok().flatten())
                        .is_some()
                })
                .unwrap_or(false);
            if exited {
                let tail = self.log_tail(12);
                let message = format!("轉錄引擎啟動失敗。\n{tail}");
                self.set_status("error", message.clone());
                return Err(message);
            }

            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        let message = format!("轉錄引擎啟動逾時。\n{}", self.log_tail(12));
        self.set_status("error", message.clone());
        Err(message)
    }

    fn clone_handle(&self) -> LogHandle {
        LogHandle {
            logs: self.logs.clone(),
        }
    }

    fn resolve_command(
        &self,
        app: &tauri::AppHandle,
        settings: &Settings,
    ) -> Result<(PathBuf, Vec<String>, Option<PathBuf>), String> {
        let port = settings.sidecar_port.to_string();
        let mut candidates: Vec<(PathBuf, Vec<String>, Option<PathBuf>)> = Vec::new();

        // 1. The source-tree virtualenv. Prefer it whenever it exists so a
        //    checkout always runs the same engine as a release build, instead
        //    of a stale bundled exe left under `target/` shadowing it. The
        //    path is baked at compile time and does not exist on end-user
        //    machines, so installed builds fall through to the bundled exe.
        let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|root| root.join("sidecar"));
        let dev_script = dev_dir
            .as_ref()
            .map(|dev| dev.join("server.py"))
            .filter(|script| script.exists());
        let dev_args = dev_script.as_ref().map(|script| {
            vec![
                script.to_string_lossy().to_string(),
                "--port".to_string(),
                port.clone(),
            ]
        });

        if let (Some(dev), Some(args)) = (dev_dir.as_ref(), dev_args.as_ref()) {
            let venv_python = dev
                .join(".venv")
                .join(if cfg!(windows) { "Scripts" } else { "bin" })
                .join(if cfg!(windows) {
                    "python.exe"
                } else {
                    "python"
                });
            if venv_python.exists() {
                candidates.push((venv_python, args.clone(), Some(dev.clone())));
            }
        }

        // 2. A PyInstaller-built sidecar bundled with the app.
        if let Ok(resource_dir) = app.path().resource_dir() {
            for dir in [resource_dir.join("sidecar"), resource_dir] {
                for name in ["undertone-sidecar.exe", "undertone-sidecar"] {
                    let exe = dir.join(name);
                    if exe.exists() {
                        candidates.push((exe, vec!["--port".to_string(), port.clone()], None));
                    }
                }
            }
        }

        // 3. Source-tree script with an explicit or system interpreter. These
        //    come after the bundled exe so a missing or broken venv does not
        //    silently pick up a random Python installation.
        if let (Some(dev), Some(args)) = (dev_dir.as_ref(), dev_args.as_ref()) {
            if !settings.python_path.trim().is_empty() {
                let explicit = PathBuf::from(settings.python_path.trim());
                if explicit.exists() {
                    candidates.push((explicit, args.clone(), Some(dev.clone())));
                }
            }
            candidates.push((PathBuf::from("python"), args.clone(), Some(dev.clone())));
        }

        // 4. Explicit interpreter override with a bundled script.
        if !settings.python_path.trim().is_empty() {
            let explicit = PathBuf::from(settings.python_path.trim());
            if explicit.exists() {
                if let Ok(resource_dir) = app.path().resource_dir() {
                    let script = resource_dir.join("sidecar").join("server.py");
                    if script.exists() {
                        candidates.push((
                            explicit,
                            vec![
                                script.to_string_lossy().to_string(),
                                "--port".to_string(),
                                port.clone(),
                            ],
                            Some(resource_dir.join("sidecar")),
                        ));
                    }
                }
            }
        }

        for (program, args, cwd) in candidates {
            let exists = program.is_absolute() || program.components().count() > 1;
            if exists && !program.exists() {
                continue;
            }
            return Ok((program, args, cwd));
        }

        Err(
            "找不到本機轉錄引擎。安裝版請重新安裝 Undertone；從原始碼執行請先跑 scripts\\setup.ps1 安裝 Python 依賴。"
                .to_string(),
        )
    }

    pub async fn transcribe(
        &self,
        wav: Vec<u8>,
        settings: &Settings,
    ) -> Result<Transcription, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(900))
            .build()
            .map_err(|e| e.to_string())?;

        let mut request = client
            .post(format!(
                "http://127.0.0.1:{}/transcribe",
                settings.sidecar_port
            ))
            .query(&[
                ("model", settings.model.clone()),
                ("language", settings.language.clone()),
                ("device", settings.device.clone()),
                ("compute_type", settings.compute_type.clone()),
                ("gpu_idle_minutes", settings.gpu_idle_minutes.to_string()),
            ])
            .header("Content-Type", "audio/wav");

        if !settings.initial_prompt.trim().is_empty() {
            request = request.query(&[("initial_prompt", settings.initial_prompt.clone())]);
        }

        let response = request
            .body(wav)
            .send()
            .await
            .map_err(|e| format!("無法連線轉錄引擎：{e}"))?;

        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| format!("讀取轉錄結果失敗：{e}"))?;

        if !status.is_success() {
            let message = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|value| {
                    value
                        .get("error")
                        .and_then(|error| error.as_str())
                        .map(str::to_string)
                })
                .unwrap_or(raw);
            return Err(message);
        }

        let result: Transcription =
            serde_json::from_str(&raw).map_err(|e| format!("解析轉錄結果失敗：{e}"))?;
        self.record_transcription(&result);
        Ok(result)
    }

    /// Push the GPU idle timeout to the sidecar without restarting it.
    pub async fn configure_gpu_idle(&self, settings: &Settings) {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(client) => client,
            Err(_) => return,
        };
        let _ = client
            .post(format!(
                "http://127.0.0.1:{}/config",
                settings.sidecar_port
            ))
            .query(&[("gpu_idle_minutes", settings.gpu_idle_minutes.to_string())])
            .send()
            .await;
    }

    /// Ask the sidecar to load the model, then surface progress in the UI.
    pub async fn warmup(&self, settings: &Settings) {
        let port = settings.sidecar_port;
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(client) => client,
            Err(_) => return,
        };

        self.set_active(&settings.model, &settings.device, &settings.compute_type);
        self.set_status("loading", format!("正在下載／載入 {} 模型…", settings.model));

        let sent = client
            .post(format!("http://127.0.0.1:{port}/warmup"))
            .query(&[
                ("model", settings.model.clone()),
                ("device", settings.device.clone()),
                ("compute_type", settings.compute_type.clone()),
                ("gpu_idle_minutes", settings.gpu_idle_minutes.to_string()),
            ])
            .send()
            .await;

        if sent.is_err() {
            self.set_status("error", "無法連繫轉錄引擎");
            return;
        }

        // Model downloads can be slow on first use, so allow up to 15 minutes
        // and keep the status message moving while we wait.
        for tick in 0..900 {
            if let Some(health) = health(port).await {
                self.update_health(&health);
                let loading = health
                    .get("loadingModels")
                    .and_then(|value| value.as_array())
                    .map(|items| !items.is_empty())
                    .unwrap_or(false);
                let loaded = health
                    .get("loadedModels")
                    .and_then(|value| value.as_array())
                    .map(|items| !items.is_empty())
                    .unwrap_or(false);
                let error = health
                    .get("lastError")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);

                if let Some(error) = error {
                    self.set_status("error", error);
                    return;
                }
                if loaded && !loading {
                    let details = self
                        .details
                        .lock()
                        .map(|details| details.clone())
                        .unwrap_or_default();
                    let device = details.device.unwrap_or_else(|| settings.device.clone());
                    let compute = details
                        .compute_type
                        .unwrap_or_else(|| settings.compute_type.clone());
                    self.set_status(
                        "ready",
                        format!("{} 模型已就緒（{device}/{compute}）", settings.model),
                    );
                    return;
                }
                if tick % 5 == 0 {
                    self.set_status(
                        "loading",
                        format!("正在下載／載入 {} 模型…（{} 秒）", settings.model, tick),
                    );
                }
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }

        self.set_status(
            "error",
            format!("{} 模型載入逾時，請檢查網路或引擎日誌。", settings.model),
        );
    }
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

fn json_strings(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn label_matches_model(label: &str, model: &str) -> bool {
    label == model || label.starts_with(&format!("{model} ("))
}

/// `small (cuda/float16)` → `("cuda", "float16")`.
fn parse_model_label(label: &str) -> Option<(String, String)> {
    let inner = label.split_once(" (")?.1.trim_end_matches(')');
    let (device, compute) = inner.split_once('/')?;
    Some((device.to_string(), compute.to_string()))
}

/// `GET /cuda` — installation/progress details for the settings page.
pub async fn cuda_status(port: u16) -> Option<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let response = client
        .get(format!("http://127.0.0.1:{port}/cuda"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

/// `POST /cuda/install` — returns as soon as the download thread starts.
pub async fn start_cuda_install(port: u16) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(format!("http://127.0.0.1:{port}/cuda/install"))
        .send()
        .await
        .map_err(|e| format!("無法連線轉錄引擎：{e}"))?;

    if response.status().is_success() {
        return Ok(());
    }

    let raw = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.as_str())
                .map(str::to_string)
        })
        .unwrap_or(raw);
    Err(message)
}

pub async fn health(port: u16) -> Option<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1400))
        .build()
        .ok()?;
    let response = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: serde_json::Value = response.json().await.ok()?;
    if value.get("app").and_then(|app| app.as_str()) == Some("undertone-sidecar") {
        Some(value)
    } else {
        None
    }
}

async fn wait_health(port: u16, secs: u64) -> Option<serde_json::Value> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if let Some(value) = health(port).await {
            return Some(value);
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    None
}

struct LogHandle {
    logs: Arc<Mutex<VecDeque<String>>>,
}

impl LogHandle {
    fn push_log(&self, line: String) {
        let line = line.trim_end().to_string();
        if line.is_empty() {
            return;
        }
        eprintln!("[sidecar] {line}");
        if let Ok(mut logs) = self.logs.lock() {
            logs.push_back(line);
            while logs.len() > MAX_LOG_LINES {
                logs.pop_front();
            }
        }
    }
}
