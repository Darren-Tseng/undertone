# 第三方元件授權

Murmur 本體採用 [MIT](LICENSE)。安裝檔內含下列第三方元件，各自依其原授權散布。
本清單只列出主要元件；各套件內含的子元件仍依其原授權條款。

## 轉錄引擎（隨安裝檔附帶）

| 元件 | 授權 | 說明 |
| --- | --- | --- |
| [faster-whisper](https://github.com/SYSTRAN/faster-whisper) | MIT | Whisper 推論實作 |
| [CTranslate2](https://github.com/OpenNMT/CTranslate2) | MIT | 推論引擎（內含 oneDNN，Apache-2.0） |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) | MIT | VAD 語音活動偵測 |
| [PyAV](https://github.com/PyAV-Org/PyAV) | BSD-3-Clause | 音訊解碼（內含 FFmpeg，LGPL/GPL 依建置選項） |
| [PyInstaller](https://pyinstaller.org/) | GPL-2.0-or-later（含 bootloader exception） | 打包 Python 引擎 |
| Python | PSF-2.0 | 內嵌直譯器 |

## Whisper 模型（使用者首次啟動時下載，不隨安裝檔散布）

| 元件 | 授權 | 說明 |
| --- | --- | --- |
| [Whisper](https://github.com/openai/whisper) 權重 | MIT | OpenAI 釋出的模型 |
| [Systran/faster-whisper-*](https://huggingface.co/Systran) | MIT | 轉換為 CTranslate2 格式的權重 |

## 應用程式

| 元件 | 授權 |
| --- | --- |
| [Tauri](https://tauri.app/) | MIT / Apache-2.0 |
| [React](https://react.dev/) | MIT |
| [Vite](https://vite.dev/) | MIT |
| [cpal](https://github.com/RustAudio/cpal) | Apache-2.0 |
| [enigo](https://github.com/enigo-rs/enigo) | MIT |
| [arboard](https://github.com/1Password/arboard) | MIT / Apache-2.0 |
| [reqwest](https://github.com/seanmonstar/reqwest) | MIT / Apache-2.0 |
| [hound](https://github.com/ruuda/hound) | Apache-2.0 |
| [chrono](https://github.com/chronotope/chrono) | MIT / Apache-2.0 |
| [tokio](https://github.com/tokio-rs/tokio) | MIT |
| [serde](https://github.com/serde-rs/serde) | MIT / Apache-2.0 |

## 重新產生清單

Python 端可用：

```powershell
sidecar\.venv\Scripts\python.exe -m pip install pip-licenses
sidecar\.venv\Scripts\python.exe -m pip_licenses --format=markdown
```

Rust 端可用 `cargo install cargo-license` 後執行
`cargo license --manifest-path src-tauri/Cargo.toml`。
