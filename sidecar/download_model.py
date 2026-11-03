#!/usr/bin/env python3
"""Pre-download a faster-whisper model so the first dictation is instant."""

from __future__ import annotations

import argparse
import os
import sys
import time


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("model", nargs="?", default="small", help="tiny/base/small/medium/large-v3")
    parser.add_argument("--device", default="auto")
    parser.add_argument("--compute-type", default="auto")
    args = parser.parse_args()

    from faster_whisper import WhisperModel

    device = args.device
    if device == "auto":
        try:
            import ctranslate2

            device = "cuda" if ctranslate2.get_cuda_device_count() > 0 else "cpu"
        except Exception:
            device = "cpu"

    compute_type = args.compute_type
    if compute_type == "auto":
        compute_type = "float16" if device == "cuda" else "int8"

    root = os.environ.get("UNDERTONE_MODEL_DIR") or None
    print(f"Downloading {args.model} for {device}/{compute_type} …")
    start = time.time()
    WhisperModel(args.model, device=device, compute_type=compute_type, download_root=root)
    print(f"Ready in {time.time() - start:.1f}s. Cache: {root or 'default Hugging Face cache'}")


if __name__ == "__main__":
    sys.exit(main())
