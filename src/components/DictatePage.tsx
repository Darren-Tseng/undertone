import { CopyIcon, FolderIcon, MicIcon, StopIcon } from "./icons";
import { EngineStatus } from "./EngineStatus";
import {
  SIDECAR_STATE_LABEL,
  deviceLabel,
  formatDuration,
  formatTimer,
  formatUptime,
  formatWhen,
  hotkeyParts,
  languageLabel,
} from "../lib/format";
import type {
  DictationState,
  RuntimeState,
  Settings,
  TranscriptEntry,
} from "../lib/api";

interface Props {
  settings: Settings | null;
  runtime: RuntimeState | null;
  state: DictationState;
  message: string | null;
  recent: TranscriptEntry[];
  onToggle: () => void;
  onCancel: () => void;
  onCopy: (entry: TranscriptEntry) => void;
  onOpenHistory: () => void;
  onOpenWorkspace: () => void;
}

const BAR_COUNT = 33;

export function DictatePage({
  settings,
  runtime,
  state,
  message,
  recent,
  onToggle,
  onCancel,
  onCopy,
  onOpenHistory,
  onOpenWorkspace,
}: Props) {
  const recording = Boolean(runtime?.recording) || state === "recording";
  const transcribing = state === "transcribing" || Boolean(runtime?.busy);
  const level = runtime?.level ?? 0;
  const elapsed = runtime?.elapsed ?? 0;
  const parts = hotkeyParts(settings?.hotkey ?? "Ctrl+Alt+Space");
  const engineState = runtime?.sidecar.state ?? "stopped";
  const engineDetails = runtime?.sidecar.details;
  const engineDevice = engineDetails?.device || settings?.device;
  const engineCompute = engineDetails?.computeType || settings?.computeType;

  const bars = Array.from({ length: BAR_COUNT }, (_, index) => {
    if (!recording) {
      const envelope = Math.sin((index / (BAR_COUNT - 1)) * Math.PI);
      return 3 + envelope * 15 + Math.abs(Math.sin(index * 0.85)) * 4;
    }
    const envelope = Math.sin((index / (BAR_COUNT - 1)) * Math.PI);
    const ripple = 0.35 + 0.65 * Math.abs(Math.sin(index * 1.31 + elapsed * 3.2));
    return Math.max(4, 6 + level * 62 * envelope * ripple);
  });

  let status: string;
  if (state === "error") {
    status = message ?? "發生錯誤";
  } else if (transcribing) {
    status = "正在轉錄…";
  } else if (recording) {
    status = "正在聆聽，再按一次快捷鍵結束";
  } else if (state === "done") {
    status = "已轉錄，文字已存到工作區";
  } else {
    status = "按下快捷鍵，開始說話";
  }

  return (
    <div className="page">
      <div className="hero">
        <div className="hero-glow" />
        <button
          type="button"
          className={`recorder ${recording ? "is-recording" : ""} ${
            transcribing ? "is-busy" : ""
          }`}
          onClick={onToggle}
          disabled={transcribing}
          aria-label={recording ? "停止錄音" : "開始錄音"}
        >
          <span className="recorder-ring ring-a" />
          <span className="recorder-ring ring-b" />
          <span className="recorder-core">
            {transcribing ? (
              <span className="core-spinner" />
            ) : recording ? (
              <StopIcon size={26} />
            ) : (
              <MicIcon size={28} />
            )}
          </span>
        </button>

        <div className="hero-copy">
          <p className={`hero-status ${state === "error" ? "is-error" : ""}`}>
            {status}
          </p>
          <div className="hero-meta">
            <span className={`live-dot ${recording ? "on" : ""}`} />
            <span className="timer">{formatTimer(elapsed)}</span>
            <span className="divider">·</span>
            <span>{settings?.model ?? "small"} 模型</span>
            <span className="divider">·</span>
            <span>{languageLabel(settings?.language)}</span>
          </div>
          <div className="hero-engine">
            <span className={`dot ${engineState}`} />
            <span>{SIDECAR_STATE_LABEL[engineState] ?? "引擎狀態未知"}</span>
            <span className="divider">·</span>
            <span>
              {deviceLabel(engineDevice)}
              {engineCompute && engineCompute !== "auto"
                ? ` / ${engineCompute}`
                : ""}
            </span>
            {engineDetails?.uptime != null ? (
              <>
                <span className="divider">·</span>
                <span>已執行 {formatUptime(engineDetails.uptime)}</span>
              </>
            ) : null}
            {engineDetails?.totalTranscriptions ? (
              <>
                <span className="divider">·</span>
                <span>{engineDetails.totalTranscriptions} 次轉錄</span>
              </>
            ) : null}
          </div>
        </div>

        <div className={`level-row ${recording ? "live" : ""}`} aria-hidden="true">
          {bars.map((height, index) => (
            <i key={index} style={{ height: `${height}px` }} />
          ))}
        </div>

        <div className="hero-footer">
          <span className="hotkey-hint">
            {parts.map((part) => (
              <kbd key={part}>{part}</kbd>
            ))}
          </span>
          <span className="hero-hint">
            {recording ? (
              <button type="button" className="link-btn" onClick={onCancel}>
                放棄這段錄音
              </button>
            ) : (
              "說中文、英文都可以，逐字稿會自動存成 Markdown"
            )}
          </span>
        </div>
      </div>

      {state === "error" && message ? (
        <div className="inline-alert" role="alert">
          <strong>無法完成轉錄</strong>
          <span>{message}</span>
        </div>
      ) : null}

      {runtime?.hotkeyError ? (
        <div className="inline-alert" role="alert">
          <strong>全域快捷鍵無法註冊</strong>
          <span>
            {runtime.hotkeyError} — 請到「設定」換一組快捷鍵。
          </span>
        </div>
      ) : null}

      <section className="section-card engine-card">
        <EngineStatus
          status={runtime?.sidecar ?? null}
          settings={settings}
          compact
        />
      </section>

      <div className="section-head">
        <div>
          <h2 className="section-title">最近的逐字稿</h2>
          <p className="section-sub">純文字檔案，直接存在你的工作區</p>
        </div>
        <div className="section-actions">
          <button type="button" className="btn ghost" onClick={onOpenWorkspace}>
            <FolderIcon size={14} />
            開啟資料夾
          </button>
          <button type="button" className="btn ghost" onClick={onOpenHistory}>
            全部紀錄
          </button>
        </div>
      </div>

      {recent.length === 0 ? (
        <div className="empty">
          <p>還沒有任何逐字稿。</p>
          <p className="muted">
            按一下麥克風或使用快捷鍵 {parts.join(" + ")}，開始第一段聽寫。
          </p>
        </div>
      ) : (
        <div className="transcript-list">
          {recent.map((entry) => (
            <article className="tcard" key={entry.path}>
              <div className="tcard-main">
                <p className="tcard-text">{entry.text}</p>
                <div className="tmeta">
                  <span>{formatWhen(entry.created)}</span>
                  <span>{formatDuration(entry.duration)}</span>
                  <span>{entry.model || "whisper"}</span>
                </div>
              </div>
              <button
                type="button"
                className="icon-btn"
                title="複製文字"
                onClick={() => onCopy(entry)}
              >
                <CopyIcon />
              </button>
            </article>
          ))}
        </div>
      )}
    </div>
  );
}
