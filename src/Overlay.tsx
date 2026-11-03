import { useEffect, useState } from "react";
import {
  api,
  events,
  type DictationState,
  type RuntimeState,
} from "./lib/api";
import { formatTimer } from "./lib/format";
import { AlertIcon, CheckIcon, SpinnerIcon } from "./components/icons";

interface OverlayState {
  state: DictationState;
  message?: string | null;
}

const SHORT: Record<string, { title: string; sub: string }> = {
  idle: { title: "待命", sub: "" },
  recording: { title: "聆聽中", sub: "再按一次快捷鍵結束" },
  transcribing: { title: "轉錄中…", sub: "faster-whisper 正在處理音訊" },
  busy: { title: "轉錄中…", sub: "請稍候" },
  done: { title: "已轉錄", sub: "" },
  error: { title: "轉錄失敗", sub: "" },
};

function truncate(text: string, length: number) {
  const clean = text.replace(/\s+/g, " ").trim();
  return clean.length > length ? `${clean.slice(0, length)}…` : clean;
}

export default function Overlay() {
  const [state, setState] = useState<OverlayState>({ state: "idle" });
  const [runtime, setRuntime] = useState<RuntimeState | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void events
      .state((payload) => {
        if (disposed) return;
        setState({ state: payload.state, message: payload.message ?? null });
      })
      .then((off) => {
        if (disposed) off();
        else unlisten = off;
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let alive = true;
    let timer = 0;

    const tick = async () => {
      let recording = false;
      try {
        const next = await api.runtime();
        if (alive) {
          setRuntime(next);
          recording = next.recording;
        }
      } catch {
        /* ignore */
      }
      if (alive) timer = window.setTimeout(tick, recording ? 90 : 900);
    };

    void tick();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, []);

  const level = runtime?.level ?? 0;
  const elapsed = runtime?.elapsed ?? 0;

  // The overlay can be shown before it receives the "recording" event, so the
  // polling state wins for live phases.
  let phase: "idle" | "recording" | "transcribing" | "done" | "error" =
    state.state === "busy" ? "transcribing" : state.state;
  if (phase !== "done" && phase !== "error") {
    if (runtime?.recording) phase = "recording";
    else if (runtime?.busy) phase = "transcribing";
    else if (phase === "recording" || phase === "transcribing") phase = "idle";
  }

  const base = SHORT[phase] ?? SHORT.idle;

  let sub = base.sub;
  if (phase === "done" && state.message) {
    sub = truncate(state.message, 42);
  } else if (phase === "error" && state.message) {
    sub = truncate(state.message, 46);
  }

  const bars = Array.from({ length: 7 }, (_, index) => {
    const wave = Math.sin(((index + 1) / 8) * Math.PI);
    const pulse = 0.4 + 0.6 * Math.abs(Math.sin(index * 1.9 + elapsed * 4));
    return phase === "recording"
      ? Math.max(5, 5 + level * 46 * wave * pulse)
      : 5 + Math.abs(Math.sin(index * 1.2)) * 6;
  });

  return (
    <div className="overlay-shell">
      <div className={`overlay-pill ${phase}`}>
        <div className="overlay-icon">
          {phase === "recording" ? (
            <span className="rec-dot" />
          ) : phase === "transcribing" ? (
            <SpinnerIcon size={16} />
          ) : phase === "done" ? (
            <CheckIcon size={16} />
          ) : phase === "error" ? (
            <AlertIcon size={16} />
          ) : (
            <span className="rec-dot idle" />
          )}
        </div>

        <div className="overlay-copy">
          <div className="overlay-title">
            {base.title}
            {phase === "recording" ? (
              <span className="overlay-timer">{formatTimer(elapsed)}</span>
            ) : null}
          </div>
          <div className="overlay-sub">{sub}</div>
        </div>

        <div className={`overlay-bars ${phase}`} aria-hidden="true">
          {bars.map((height, index) => (
            <i key={index} style={{ height: `${height}px` }} />
          ))}
        </div>
      </div>
    </div>
  );
}
