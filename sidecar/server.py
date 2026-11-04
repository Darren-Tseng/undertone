#!/usr/bin/env python3
"""Undertone sidecar.

A tiny local HTTP server that owns the faster-whisper model. The Tauri app
spawns this process, posts raw WAV bytes to /transcribe, and gets plain text
back. Nothing ever leaves the machine except the optional model download.
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import io
import json
import os
import re
import shutil
import sys
import tempfile
import threading
import time
import traceback
import urllib.request
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

MAX_BODY_BYTES = 200 * 1024 * 1024  # 200 MB, plenty for dictation + files

_models: dict[tuple[str, str, str], object] = {}
_models_lock = threading.Lock()
_warmup_thread: threading.Thread | None = None

_state = {
    "startedAt": time.time(),
    "loadedModels": [],
    "loadingModels": [],
    "lastError": None,
    "cudaUnavailable": False,
    # GPU idle release: after this many minutes without dictation, unload the
    # CUDA model so it stops holding VRAM. 0 disables the behaviour.
    "gpuIdleMinutes": 0,
    "gpuReleased": False,
    "lastGpuReleaseAt": None,
    "lastActivityAt": time.time(),
    "requestedDevice": "auto",
    "requestedModel": "small",
    "cudaInstall": {
        "state": "idle",  # idle | running | done | error
        "phase": "",
        "downloadedBytes": 0,
        "totalBytes": 0,
        "error": None,
        "finishedAt": None,
    },
}

_cache: dict[str, object] = {}

# `os.add_dll_directory` returns a handle that must stay alive, otherwise the
# directory is dropped from the DLL search path once the handle is collected.
_DLL_DIR_HANDLES: list[object] = []

# CUDA runtime wheels the engine needs. They are downloaded from PyPI at
# runtime so the installed app does not have to ship ~2 GB of DLLs.
CUDA_PACKAGES = (
    "nvidia-cublas-cu12",
    "nvidia-cudnn-cu12",
    "nvidia-cuda-runtime-cu12",
    "nvidia-cuda-nvrtc-cu12",
)
_PYPI_JSON = "https://pypi.org/pypi/{name}/json"
_cuda_lock = threading.Lock()


def cuda_dir() -> str:
    """Where runtime-downloaded CUDA DLLs live (`UNDERTONE_CUDA_DIR`)."""
    explicit = os.environ.get("UNDERTONE_CUDA_DIR", "").strip()
    if explicit:
        return explicit
    appdata = os.environ.get("APPDATA") or os.path.expanduser("~")
    return os.path.join(appdata, "so.undertone.app", "cuda")


def _dll_dirs(root: str) -> list[str]:
    """Every directory under `root` that directly contains a DLL."""
    found: list[str] = []
    if not os.path.isdir(root):
        return found
    for dirpath, _dirnames, filenames in os.walk(root):
        if any(name.lower().endswith((".dll", ".so", ".dylib")) for name in filenames):
            found.append(dirpath)
    return found


def _register_dll_paths(paths: list[str]) -> None:
    seen: set[str] = set()
    extra_paths: list[str] = []
    for path in paths:
        key = os.path.normcase(path)
        if key in seen or not os.path.isdir(path):
            continue
        seen.add(key)
        extra_paths.append(path)
        try:
            _DLL_DIR_HANDLES.append(os.add_dll_directory(path))
        except Exception:
            pass

    # ctranslate2 resolves cublas/cudnn with a plain LoadLibrary call, which
    # ignores `add_dll_directory`; PATH is what it actually searches.
    if extra_paths:
        current = os.environ.get("PATH", "")
        os.environ["PATH"] = os.pathsep.join(extra_paths + [current])


def _enable_bundled_cuda_libs() -> None:
    """Expose installed and runtime-downloaded CUDA DLLs to ctranslate2."""
    if os.name != "nt":
        return

    search_roots: list[str] = []

    # PyInstaller onefile builds extract to a temp dir (`sys._MEIPASS`); the
    # collected `nvidia` packages live there instead of site-packages, and the
    # `nvidia` namespace import may not resolve. Search both locations.
    try:
        import nvidia
    except Exception:
        nvidia = None  # noqa: F841
    else:
        # `nvidia` is a namespace package: its sub-packages have `__file__ == None`,
        # so build the paths from the namespace `__path__` instead.
        search_roots.extend(list(getattr(nvidia, "__path__", ())))
    meipass = getattr(sys, "_MEIPASS", None)
    if meipass:
        candidate = os.path.join(meipass, "nvidia")
        if os.path.isdir(candidate):
            search_roots.append(candidate)

    paths: list[str] = []
    for root in search_roots:
        for package in ("cublas", "cudnn", "cuda_runtime", "cuda_nvrtc"):
            base = os.path.join(root, package)
            for sub in ("bin", "lib", os.path.join("lib", "x64")):
                path = os.path.join(base, sub)
                if os.path.isdir(path):
                    paths.append(path)

    # DLLs installed from the settings page (pip wheels extracted by us).
    paths.extend(_dll_dirs(cuda_dir()))
    _register_dll_paths(paths)


_enable_bundled_cuda_libs()


def log(message: str) -> None:
    print(f"[undertone] {message}", file=sys.stderr, flush=True)


# -- GPU idle release -----------------------------------------------------

_busy_lock = threading.Lock()
_busy_count = 0
_idle_thread: threading.Thread | None = None


def _begin_busy() -> None:
    global _busy_count
    with _busy_lock:
        _busy_count += 1
    _state["lastActivityAt"] = time.time()


def _end_busy() -> None:
    global _busy_count
    with _busy_lock:
        if _busy_count > 0:
            _busy_count -= 1
    _state["lastActivityAt"] = time.time()


def _is_busy() -> bool:
    with _busy_lock:
        return _busy_count > 0


def configure_gpu_idle(minutes: int) -> None:
    """Update the idle timeout (minutes, 0 = never release) and start the watcher."""
    _state["gpuIdleMinutes"] = max(0, int(minutes))
    ensure_idle_watcher()


def ensure_idle_watcher() -> None:
    global _idle_thread
    if _idle_thread is not None and _idle_thread.is_alive():
        return
    _idle_thread = threading.Thread(target=_idle_loop, name="gpu-idle", daemon=True)
    _idle_thread.start()


def _idle_loop() -> None:
    while True:
        time.sleep(10)
        minutes = int(_state.get("gpuIdleMinutes") or 0)
        if minutes <= 0 or _is_busy():
            continue
        idle_for = time.time() - float(_state["lastActivityAt"])
        if idle_for < minutes * 60:
            continue
        try:
            _release_gpu_models()
        except Exception as exc:  # noqa: BLE001 - never kill the watcher
            log(f"GPU idle release failed: {exc}")


def _release_gpu_models() -> bool:
    """Drop every CUDA WhisperModel so its VRAM is returned to the system."""
    with _models_lock:
        if _busy_count > 0 or _state["loadingModels"]:
            return False
        keys = [key for key in _models if key[1] == "cuda"]
        if not keys:
            return False
        for key in keys:
            _models.pop(key, None)
            label = f"{key[0]} ({key[1]}/{key[2]})"
            if label in _state["loadedModels"]:
                _state["loadedModels"].remove(label)

    gc.collect()
    _state["gpuReleased"] = True
    _state["lastGpuReleaseAt"] = time.time()
    log("GPU idle timeout: released CUDA model(s) to free VRAM")

    # With `auto`, the next dictation should be answered immediately on CPU
    # while the GPU model reloads in the background.
    if (_state.get("requestedDevice") or "auto") == "auto":
        preferred = _state.get("requestedModel")
        model_name = (
            preferred
            if any(key[0] == preferred for key in keys)
            else keys[0][0]
        )
        threading.Thread(
            target=_preload_cpu_model,
            args=(model_name,),
            name="cpu-preload",
            daemon=True,
        ).start()
    return True


def _preload_cpu_model(model_name: str) -> None:
    try:
        compute_type = resolve_compute_type("auto", "cpu")
        started = time.time()
        get_model(model_name, "cpu", compute_type)
        log(
            f"preloaded CPU model={model_name} ({compute_type}) "
            f"in {time.time() - started:.1f}s"
        )
    except Exception as exc:  # noqa: BLE001 - logged only, GPU is already free
        log(f"CPU preload failed: {exc}")


_CJK = r"\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff"


def normalize_text(text: str) -> str:
    """Tidy up Chinese output: full-width punctuation, no stray spaces."""
    import re

    text = re.sub(rf"(?<=[{_CJK}])\s+(?=[{_CJK}])", "", text)
    text = re.sub(rf"(?<=[{_CJK}])\s*,\s*(?=[{_CJK}])", "，", text)
    text = re.sub(rf"(?<=[{_CJK}])\s*\.\s*(?=[{_CJK}])", "。", text)
    text = re.sub(rf"(?<=[{_CJK}])\s*;\s*(?=[{_CJK}])", "；", text)
    text = re.sub(rf"(?<=[{_CJK}])\s*:\s*(?=[{_CJK}])", "：", text)
    text = re.sub(rf"(?<=[{_CJK}])\s*\?\s*(?=[{_CJK}])", "？", text)
    text = re.sub(rf"(?<=[{_CJK}])\s*!\s*(?=[{_CJK}])", "！", text)
    return text.strip()


def _faster_whisper_version() -> str:
    cached = _cache.get("version")
    if cached is not None:
        return str(cached)
    try:
        import faster_whisper

        version = getattr(faster_whisper, "__version__", "unknown")
    except Exception:
        version = "not-installed"
    _cache["version"] = version
    return str(version)


def _cuda_device_count() -> int:
    cached = _cache.get("cuda")
    if cached is not None:
        return int(cached)
    try:
        import ctranslate2

        count = int(ctranslate2.get_cuda_device_count())
    except Exception:
        count = 0
    _cache["cuda"] = count
    return count


def _version_key(value: str) -> tuple[int, ...]:
    return tuple(int(part) for part in re.findall(r"\d+", value))


def _wheel_matches(filename: str) -> bool:
    """Accept a CPython 3.x / abi3 Windows wheel for this interpreter."""
    if not filename.endswith(".whl") or "win_amd64" not in filename:
        return False
    parts = filename[:-4].split("-")
    if len(parts) < 3:
        return False
    python_tag, abi_tag = parts[-3], parts[-2]
    for tag in python_tag.split("."):
        if tag in ("py3", "py2"):
            return True
        if tag.startswith("cp3") and tag[2:].isdigit():
            minor = int(tag[3:])
            if minor == sys.version_info.minor and abi_tag in (tag, "abi3", "none"):
                return True
            if minor < sys.version_info.minor and abi_tag == "abi3":
                return True
    return False


def _pick_cuda_wheel(name: str) -> dict:
    """Newest PyPI release that actually ships a Windows wheel.

    The newest overall release is often Linux-only, so `pip install` would
    resolve an older version too; mirror that behaviour here.
    """
    request = urllib.request.Request(
        _PYPI_JSON.format(name=name), headers={"User-Agent": "undertone/1.0"}
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        meta = json.load(response)

    candidates: list[tuple[tuple[int, ...], dict]] = []
    for version, files in (meta.get("releases") or {}).items():
        for file in files:
            if file.get("yanked"):
                continue
            if not _wheel_matches(file.get("filename", "")):
                continue
            candidates.append((_version_key(version), file))

    if not candidates:
        raise RuntimeError(f"PyPI 上找不到 {name} 的 Windows 版本")
    candidates.sort(key=lambda item: item[0])
    return candidates[-1][1]


def _download_file(url: str, dest: str) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": "undertone/1.0"})
    with urllib.request.urlopen(request, timeout=120) as response, open(
        dest, "wb"
    ) as handle:
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            handle.write(chunk)
            _state["cudaInstall"]["downloadedBytes"] += len(chunk)


def _sha256(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _extract_dlls(wheel_path: str, root: str) -> None:
    """Extract only `nvidia/**/*.dll`; headers and static libs are dead weight."""
    with zipfile.ZipFile(wheel_path) as archive:
        for info in archive.infolist():
            name = info.filename
            if not name.startswith("nvidia/") or not name.lower().endswith(".dll"):
                continue
            relative = os.path.normpath(name)
            if relative.startswith("..") or os.path.isabs(relative):
                continue
            target = os.path.join(root, relative)
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with archive.open(info) as source, open(target, "wb") as output:
                shutil.copyfileobj(source, output)


def _cuda_installed() -> tuple[int, int]:
    files = 0
    size = 0
    for dirpath, _dirnames, filenames in os.walk(cuda_dir()):
        for name in filenames:
            if not name.lower().endswith(".dll"):
                continue
            files += 1
            try:
                size += os.path.getsize(os.path.join(dirpath, name))
            except OSError:
                pass
    return files, size


def cuda_status() -> dict:
    files, size = _cuda_installed()
    return {
        "supported": os.name == "nt",
        "dir": cuda_dir(),
        "files": files,
        "installedBytes": size,
        "cudaDevices": _cuda_device_count(),
        "packages": list(CUDA_PACKAGES),
        "install": dict(_state["cudaInstall"]),
    }


def start_cuda_install() -> dict:
    """Kick off a background download; poll `cuda_status()` for progress."""
    with _cuda_lock:
        if _state["cudaInstall"]["state"] == "running":
            return cuda_status()
        _state["cudaInstall"] = {
            "state": "running",
            "phase": "正在取得 CUDA 套件資訊…",
            "downloadedBytes": 0,
            "totalBytes": 0,
            "error": None,
            "finishedAt": None,
        }
        threading.Thread(
            target=_install_cuda_worker, name="cuda-install", daemon=True
        ).start()
    return cuda_status()


def _install_cuda_worker() -> None:
    install = _state["cudaInstall"]
    tmp_dir: str | None = None
    try:
        wheels: list[tuple[str, dict]] = []
        for name in CUDA_PACKAGES:
            wheels.append((name, _pick_cuda_wheel(name)))
        install["totalBytes"] = sum(int(file.get("size") or 0) for _, file in wheels)

        root = cuda_dir()
        os.makedirs(root, exist_ok=True)
        tmp_dir = tempfile.mkdtemp(prefix="undertone-cuda-")

        for index, (name, file) in enumerate(wheels, start=1):
            install["phase"] = f"正在下載 {name}（{index}/{len(wheels)}）"
            wheel_path = os.path.join(tmp_dir, file["filename"])
            _download_file(file["url"], wheel_path)
            expected = (file.get("digests") or {}).get("sha256")
            if expected and _sha256(wheel_path) != expected:
                raise RuntimeError(f"{name} 下載檔案校驗失敗，請重試")
            install["phase"] = f"正在安裝 {name}（{index}/{len(wheels)}）"
            _extract_dlls(wheel_path, root)
            try:
                os.remove(wheel_path)
            except OSError:
                pass

        install["phase"] = "正在啟用 CUDA…"
        _enable_bundled_cuda_libs()
        _cache.pop("cuda", None)
        _state["cudaUnavailable"] = False
        count = _cuda_device_count()
        install.update(
            state="done",
            phase=f"完成（偵測到 {count} 個 CUDA 裝置）",
            finishedAt=time.time(),
        )
        log(f"CUDA runtime installed to {root} ({count} device(s))")
    except Exception as exc:  # noqa: BLE001 - report everything to the app
        install.update(
            state="error",
            phase="下載失敗",
            error=str(exc),
            finishedAt=time.time(),
        )
        log(f"CUDA install failed: {exc}\n{traceback.format_exc()}")
    finally:
        if tmp_dir:
            shutil.rmtree(tmp_dir, ignore_errors=True)


def resolve_device(requested: str) -> str:
    requested = (requested or "auto").lower()
    if requested in ("cuda", "auto"):
        if not _state["cudaUnavailable"] and _cuda_device_count() > 0:
            return "cuda"
        if requested == "cuda":
            log("CUDA requested but unavailable, using CPU")
    return "cpu"


def resolve_compute_type(requested: str, device: str) -> str:
    requested = (requested or "auto").lower()
    if requested not in ("", "auto"):
        return requested
    return "float16" if device == "cuda" else "int8"


def get_model(model_name: str, device: str, compute_type: str):
    """Load (and cache) a WhisperModel. Loading is serialized on purpose."""
    from faster_whisper import WhisperModel

    key = (model_name, device, compute_type)
    with _models_lock:
        cached = _models.get(key)
        if cached is not None:
            return cached

        log(f"loading model={model_name} device={device} compute_type={compute_type}")
        started = time.time()
        model = WhisperModel(
            model_name,
            device=device,
            compute_type=compute_type,
            download_root=os.environ.get("UNDERTONE_MODEL_DIR") or None,
        )
        _models[key] = model
        label = f"{model_name} ({device}/{compute_type})"
        if label not in _state["loadedModels"]:
            _state["loadedModels"].append(label)
        if device == "cuda":
            _state["gpuReleased"] = False
        _state["lastError"] = None
        log(f"loaded {label} in {time.time() - started:.1f}s")
        return model


def drop_model(model_name: str, device: str, compute_type: str) -> None:
    with _models_lock:
        _models.pop((model_name, device, compute_type), None)
    label = f"{model_name} ({device}/{compute_type})"
    if label in _state["loadedModels"]:
        _state["loadedModels"].remove(label)


def mark_cuda_unavailable(exc: Exception) -> None:
    _state["cudaUnavailable"] = True
    _state["lastError"] = f"CUDA 無法使用，已改用 CPU：{exc}"
    log(f"CUDA unavailable ({exc}); using CPU from now on")


def silence_wav(seconds: float = 0.6, rate: int = 16000) -> bytes:
    import wave

    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes(b"\x00\x00" * int(rate * seconds))
    return buffer.getvalue()


def run_transcription(model, wav_bytes, language, initial_prompt, beam_size, vad=True):
    segments, info = model.transcribe(
        io.BytesIO(wav_bytes),
        language=language,
        beam_size=beam_size,
        vad_filter=vad,
        vad_parameters={"min_silence_duration_ms": 400, "speech_pad_ms": 200},
        condition_on_previous_text=False,
        initial_prompt=initial_prompt or None,
        temperature=0.0,
        word_timestamps=False,
    )
    collected = [
        {"start": round(segment.start, 3), "end": round(segment.end, 3), "text": segment.text}
        for segment in segments
    ]
    return collected, info


def load_with_fallback(model_name: str, device: str, compute_type: str):
    """Load a model, downgrading to CPU when CUDA cannot be used."""
    compute_type = resolve_compute_type(compute_type, device)
    try:
        return get_model(model_name, device, compute_type), device, compute_type
    except Exception as exc:
        if device != "cuda":
            raise
        mark_cuda_unavailable(exc)
        drop_model(model_name, device, compute_type)
        device = "cpu"
        compute_type = resolve_compute_type("auto", device)
        return get_model(model_name, device, compute_type), device, compute_type


def warmup(model_name: str, device: str, compute_type: str) -> None:
    global _warmup_thread
    if _warmup_thread is not None and _warmup_thread.is_alive():
        return

    def run() -> None:
        label = f"{model_name} ({device}/{compute_type})"
        _begin_busy()
        _state["loadingModels"] = [label]
        try:
            model, actual_device, actual_compute = load_with_fallback(
                model_name, device, compute_type
            )

            if actual_device == "cuda":
                # Some Windows machines expose a GPU but miss cuBLAS/cuDNN.
                # That failure only shows up on the first real compute, so probe
                # once with VAD disabled (VAD would skip the encoder entirely).
                try:
                    run_transcription(model, silence_wav(), "en", "", 1, vad=False)
                except Exception as exc:
                    mark_cuda_unavailable(exc)
                    drop_model(model_name, actual_device, actual_compute)
                    actual_device = "cpu"
                    actual_compute = resolve_compute_type("auto", actual_device)
                    model = get_model(model_name, actual_device, actual_compute)
                    run_transcription(model, silence_wav(), "en", "", 1, vad=False)

            _state["lastError"] = None
            loaded = f"{model_name} ({actual_device}/{actual_compute})"
            if loaded not in _state["loadedModels"]:
                _state["loadedModels"].append(loaded)
            log(f"warmup complete: {loaded}")
        except Exception as exc:  # noqa: BLE001 - reported to the UI
            _state["lastError"] = str(exc)
            log(f"warmup failed: {exc}\n{traceback.format_exc()}")
        finally:
            _state["loadingModels"] = []
            _end_busy()

    _warmup_thread = threading.Thread(target=run, name="warmup", daemon=True)
    _warmup_thread.start()


def transcribe(
    wav_bytes: bytes,
    model_name: str,
    language: str,
    device: str,
    compute_type: str,
    initial_prompt: str,
    beam_size: int,
) -> dict:
    _begin_busy()
    try:
        return _transcribe(
            wav_bytes,
            model_name,
            language,
            device,
            compute_type,
            initial_prompt,
            beam_size,
        )
    finally:
        _end_busy()


def _transcribe(
    wav_bytes: bytes,
    model_name: str,
    language: str,
    device: str,
    compute_type: str,
    initial_prompt: str,
    beam_size: int,
) -> dict:
    requested_device = (device or "auto").lower()
    requested_compute = compute_type
    _state["requestedDevice"] = requested_device
    _state["requestedModel"] = model_name

    device = resolve_device(device)

    # The CUDA model was released while idle: use the preloaded CPU model for
    # this dictation, then bring the GPU model back in the background.
    cold_gpu = _state["gpuReleased"] and requested_device == "auto"
    if cold_gpu:
        _state["gpuReleased"] = False
        device = "cpu"
        compute_type = resolve_compute_type("auto", device)
        model = get_model(model_name, device, compute_type)
    else:
        model, device, compute_type = load_with_fallback(
            model_name, device, compute_type
        )

    language_arg = None if language in ("", "auto", "None") else language
    started = time.time()

    try:
        collected, info = run_transcription(
            model, wav_bytes, language_arg, initial_prompt, beam_size
        )
    except Exception as exc:
        if device != "cuda":
            raise
        mark_cuda_unavailable(exc)
        drop_model(model_name, device, compute_type)
        device = "cpu"
        compute_type = resolve_compute_type("auto", device)
        model = get_model(model_name, device, compute_type)
        collected, info = run_transcription(
            model, wav_bytes, language_arg, initial_prompt, beam_size
        )

    text = normalize_text("".join(segment["text"] for segment in collected).strip())
    elapsed = time.time() - started

    if cold_gpu:
        threading.Thread(
            target=warmup,
            args=(
                model_name,
                "cuda",
                resolve_compute_type(requested_compute, "cuda"),
            ),
            name="gpu-rewarm",
            daemon=True,
        ).start()

    return {
        "text": text,
        "segments": collected,
        "language": info.language,
        "languageProbability": info.language_probability,
        "duration": info.duration,
        "elapsed": round(elapsed, 3),
        "device": device,
        "computeType": compute_type,
        "model": model_name,
    }


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "UndertoneSidecar/1.0"

    def log_message(self, fmt: str, *args) -> None:  # noqa: A003
        log(f"{self.address_string()} {fmt % args}")

    # -- helpers ---------------------------------------------------------
    def _send_json(self, status: int, payload: dict) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _query(self) -> dict[str, str]:
        parsed = urlparse(self.path)
        return {k: v[0] for k, v in parse_qs(parsed.query).items()}

    def _path(self) -> str:
        return urlparse(self.path).path.rstrip("/") or "/"

    def _read_body(self) -> bytes:
        length = int(self.headers.get("Content-Length") or 0)
        if length <= 0:
            return b""
        if length > MAX_BODY_BYTES:
            raise ValueError("音訊檔案過大")
        chunks = []
        remaining = length
        while remaining > 0:
            chunk = self.rfile.read(min(remaining, 1024 * 1024))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)

    # -- routes ----------------------------------------------------------
    def do_GET(self) -> None:  # noqa: N802
        path = self._path()
        if path == "/health":
            self._send_json(
                200,
                {
                    "app": "undertone-sidecar",
                    "ok": True,
                    "pid": os.getpid(),
                    "python": sys.version.split()[0],
                    "fasterWhisper": _faster_whisper_version(),
                    "cudaDevices": _cuda_device_count(),
                    "loadedModels": _state["loadedModels"],
                    "loadingModels": _state["loadingModels"],
                    "lastError": _state["lastError"],
                    "cudaInstall": _state["cudaInstall"],
                    "uptime": round(time.time() - _state["startedAt"], 1),
                    "gpuIdleMinutes": _state["gpuIdleMinutes"],
                    "gpuReleased": _state["gpuReleased"],
                    "lastGpuReleaseAt": _state["lastGpuReleaseAt"],
                    "idleSeconds": round(time.time() - _state["lastActivityAt"], 1),
                },
            )
            return
        if path == "/models":
            self._send_json(
                200,
                {
                    "loaded": _state["loadedModels"],
                    "loading": _state["loadingModels"],
                    "lastError": _state["lastError"],
                },
            )
            return
        if path == "/cuda":
            self._send_json(200, cuda_status())
            return
        self._send_json(404, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        path = self._path()
        query = self._query()
        try:
            if path == "/transcribe":
                if "gpu_idle_minutes" in query:
                    configure_gpu_idle(int(query["gpu_idle_minutes"]))
                body = self._read_body()
                if not body:
                    self._send_json(400, {"error": "缺少音訊資料"})
                    return
                result = transcribe(
                    body,
                    model_name=query.get("model", "small"),
                    language=query.get("language", "auto"),
                    device=query.get("device", "auto"),
                    compute_type=query.get("compute_type", "auto"),
                    initial_prompt=query.get("initial_prompt", ""),
                    beam_size=int(query.get("beam_size", "5")),
                )
                self._send_json(200, result)
                return

            if path == "/config":
                if "gpu_idle_minutes" in query:
                    configure_gpu_idle(int(query["gpu_idle_minutes"]))
                self._send_json(
                    200,
                    {
                        "gpuIdleMinutes": _state["gpuIdleMinutes"],
                        "gpuReleased": _state["gpuReleased"],
                    },
                )
                return

            if path == "/cuda/install":
                if os.name != "nt":
                    self._send_json(400, {"error": "GPU 加速僅支援 Windows"})
                    return
                self._send_json(202, start_cuda_install())
                return

            if path == "/warmup":
                # A stale error from an earlier run must not fail a fresh warmup.
                _state["lastError"] = None
                if "gpu_idle_minutes" in query:
                    configure_gpu_idle(int(query["gpu_idle_minutes"]))
                _state["requestedDevice"] = (
                    query.get("device", "auto") or "auto"
                ).lower()
                _state["requestedModel"] = query.get("model", "small")
                device = resolve_device(query.get("device", "auto"))
                compute_type = resolve_compute_type(query.get("compute_type", "auto"), device)
                warmup(query.get("model", "small"), device, compute_type)
                self._send_json(202, {"status": "warming"})
                return

            if path == "/shutdown":
                self._send_json(200, {"status": "bye"})
                threading.Thread(target=self.server.shutdown, daemon=True).start()
                return

            self._send_json(404, {"error": "not found"})
        except ValueError as exc:
            self._send_json(400, {"error": str(exc)})
        except Exception as exc:  # noqa: BLE001 - always report to the app
            _state["lastError"] = str(exc)
            log(f"request failed: {exc}\n{traceback.format_exc()}")
            self._send_json(500, {"error": str(exc)})


def main() -> None:
    parser = argparse.ArgumentParser(description="Undertone faster-whisper sidecar")
    parser.add_argument("--port", type=int, default=8756)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args()

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    server.daemon_threads = True
    ensure_idle_watcher()
    log(f"listening on http://{args.host}:{args.port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        log("stopped")


if __name__ == "__main__":
    main()
