# AGENTS.md

Murmur is a Windows-first, local-only voice dictation app: React/Vite UI → Tauri 2 (Rust) → Python `faster-whisper` sidecar on `127.0.0.1:8756`. The folder is named `epicenter`, but the product, npm package, Rust crate, and identifier are **murmur** / `so.murmur.app`.

## Commands

- `npm run app` — full dev app (`tauri dev`). This is the only way to exercise Tauri IPC; `npm run dev` starts Vite alone and renders a dead UI.
- `npm run build` — `tsc --noEmit && vite build`; this is also the typecheck.
- `cargo test --manifest-path src-tauri/Cargo.toml` — Rust tests (only in `transcripts.rs`).
- Single test: `cargo test --manifest-path src-tauri/Cargo.toml saves_lists_searches_and_deletes`
- `cargo check --manifest-path src-tauri/Cargo.toml` — fast Rust feedback (Tauri debug builds are slow).
- First-time engine setup: `powershell -ExecutionPolicy Bypass -File scripts/setup.ps1` (or `npm run setup`). It creates `sidecar/.venv`, installs `requirements.txt`, and downloads the `small` model. Required before dictation works; the UI itself still boots without it.
- Installer: `npm run installer`（先 PyInstaller 打包引擎，再 `tauri build`）→ `src-tauri/target/release/bundle/nsis/`. Pushing a `v*` tag triggers `.github/workflows/release.yml`, which builds and publishes the installer on GitHub Releases.
- Bundled engine: `powershell -ExecutionPolicy Bypass -File scripts/build-sidecar.ps1` → `sidecar/dist/murmur-sidecar.exe`. `tauri.conf.json` maps it to `resources/sidecar/murmur-sidecar.exe`, so `npm run tauri build` fails if the exe has not been built. The installer ships without Whisper weights; the model downloads on first run.

There is no ESLint, Prettier, frontend test runner, or pre-commit config — do not invent those commands. The only CI is the release workflow.

## Environment

- Requires Node 20+, Rust MSVC, Python **3.10–3.12** (3.11 recommended), WebView2.
- Windows assumptions are baked in: WASAPI capture via `cpal`, synthetic Ctrl+V via `enigo`, `explorer` for reveal, `taskkill /T` for the sidecar process tree.
- No ffmpeg; the sidecar decodes with PyAV.
- CUDA is optional. Missing cuBLAS/cuDNN falls back to CPU automatically; `server.py` probes with silence during warmup because the failure only appears on first real compute.
- Settings: `%APPDATA%\so.murmur.app\settings.json`. Model cache: `%APPDATA%\so.murmur.app\models\` (exported to the sidecar as `MURMUR_MODEL_DIR`). Transcripts: `%USERPROFILE%\Murmur\transcripts\YYYY-MM-DD\HHMMSS.md` (workspace configurable).

## Architecture / wiring

- `src/main.tsx` mounts two UIs from one HTML: `App` for window label `main`, `Overlay` for label `overlay` (or `?window=overlay`). The overlay is a separate transparent, always-on-top, borderless window created in `lib.rs`.
- All IPC is wrapped in `src/lib/api.ts`. A new Rust `#[tauri::command]` must also be added to `invoke_handler![...]` in `src-tauri/src/lib.rs`.
- Serde uses `rename_all = "camelCase"`: Rust `auto_paste` ↔ TS `autoPaste` in both settings and runtime state.
- Rust modules: `lib.rs` orchestration/state/commands, `audio.rs` recording, `sidecar.rs` process + HTTP lifecycle, `transcripts.rs` Markdown storage/search, `settings.rs` JSON persistence, `paste.rs` clipboard/keystroke.
- Sidecar HTTP: `GET /health`, `GET /models`, `POST /warmup`, `POST /transcribe` (raw WAV body; model/language/device/compute_type as query params), `POST /shutdown`. `/health` only counts if `app == "murmur-sidecar"`.
- Dev sidecar resolution order in `sidecar.rs`: bundled resource exe → `sidecar/.venv/Scripts/python.exe sidecar/server.py` → `settings.python_path` → `python`. If something healthy is already on the port, Rust reuses it instead of spawning; kill stray `python` processes on 8756 before testing spawn/lifecycle changes.
- Events: `murmur://state` (dictation lifecycle) and `murmur://transcript` (new entry). UI state also comes from polling `get_runtime_state` — update both when changing dictation state.

## Gotchas

- Closing the main window hides to the tray; it does not quit. Use the tray “結束” item (or kill the process) to stop the app.
- Default hotkey is `Ctrl+Alt+Space`; one press starts, the next stops. Recording auto-stops at `max_record_secs` (clamped 5–3600, default 300).
- Auto-paste targets whichever window has focus. Do not switch windows during recording. `auto_paste` and `copy_to_clipboard` are independent.
- All user-facing strings, including Rust error messages, are Traditional Chinese (zh-TW). Match that style for new UI and errors.
- TypeScript is strict with `noUnusedLocals`/`noUnusedParameters`; `npm run build` fails on unused code.
- Do not hand-edit generated/build output: `src-tauri/gen/`, `src-tauri/target/`, `dist/`, `sidecar/.venv/`, `sidecar/build/`, `sidecar/dist/`.
