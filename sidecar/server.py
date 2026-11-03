#!/usr/bin/env python3
"""Murmur sidecar.

A tiny local HTTP server that owns the faster-whisper model. The Tauri app
spawns this process, posts raw WAV bytes to /transcribe, and gets plain text
back. Nothing ever leaves the machine except the optional model download.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import sys
import threading
import time
import traceback
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
}

_cache: dict[str, object] = {}

# `os.add_dll_directory` returns a handle that must stay alive, otherwise the
# directory is dropped from the DLL search path once the handle is collected.
_DLL_DIR_HANDLES: list[object] = []


def _enable_bundled_cuda_libs() -> None:
    """Make `pip install nvidia-cublas-cu12 nvidia-cudnn-cu12` work without PATH edits."""
    if os.name != "nt":
        return
    try:
        import nvidia
    except Exception:
        return

    # `nvidia` is a namespace package: its sub-packages have `__file__ == None`,
    # so build the paths from the namespace `__path__` instead.
    roots = list(getattr(nvidia, "__path__", ()))
    extra_paths: list[str] = []
    for root in roots:
        for package in ("cublas", "cudnn", "cuda_runtime", "cuda_nvrtc"):
            base = os.path.join(root, package)
            for sub in ("bin", "lib", os.path.join("lib", "x64")):
                path = os.path.join(base, sub)
                if not os.path.isdir(path):
                    continue
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


_enable_bundled_cuda_libs()


def log(message: str) -> None:
    print(f"[murmur] {message}", file=sys.stderr, flush=True)


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
            download_root=os.environ.get("MURMUR_MODEL_DIR") or None,
        )
        _models[key] = model
        label = f"{model_name} ({device}/{compute_type})"
        if label not in _state["loadedModels"]:
            _state["loadedModels"].append(label)
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
    device = resolve_device(device)
    model, device, compute_type = load_with_fallback(model_name, device, compute_type)

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
    server_version = "MurmurSidecar/1.0"

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
                    "app": "murmur-sidecar",
                    "ok": True,
                    "pid": os.getpid(),
                    "python": sys.version.split()[0],
                    "fasterWhisper": _faster_whisper_version(),
                    "cudaDevices": _cuda_device_count(),
                    "loadedModels": _state["loadedModels"],
                    "loadingModels": _state["loadingModels"],
                    "lastError": _state["lastError"],
                    "uptime": round(time.time() - _state["startedAt"], 1),
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
        self._send_json(404, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        path = self._path()
        query = self._query()
        try:
            if path == "/transcribe":
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

            if path == "/warmup":
                # A stale error from an earlier run must not fail a fresh warmup.
                _state["lastError"] = None
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
    parser = argparse.ArgumentParser(description="Murmur faster-whisper sidecar")
    parser.add_argument("--port", type=int, default=8756)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args()

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    server.daemon_threads = True
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
