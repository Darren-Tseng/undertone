import { AlertIcon, RefreshIcon } from "./icons";
import type { Settings, SidecarStatus } from "../lib/api";
import {
  SIDECAR_STATE_LABEL,
  deviceLabel,
  formatDuration,
  formatUptime,
  formatWhen,
  languageLabel,
} from "../lib/format";

interface Props {
  status: SidecarStatus | null;
  settings: Settings | null;
  compact?: boolean;
  onRestart?: () => void;
  restarting?: boolean;
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <span className="stat-label">{label}</span>
      <span className="stat-value">{value}</span>
    </div>
  );
}

export function EngineStatus({
  status,
  settings,
  compact = false,
  onRestart,
  restarting = false,
}: Props) {
  const state = status?.state ?? "stopped";
  const details = status?.details;
  const activeModel = details?.activeModel || settings?.model || "—";
  const device = details?.device || settings?.device || "auto";
  const compute = details?.computeType || settings?.computeType || "auto";
  const loaded = details?.loadedModels ?? [];
  const loading = details?.loadingModels ?? [];
  const last = details?.lastTranscription ?? null;
  const totalTranscriptions = details?.totalTranscriptions ?? 0;
  const totalAudio = details?.totalAudioSeconds ?? 0;
  const totalElapsed = details?.totalTranscribeSeconds ?? 0;
  const averageSpeed = totalElapsed > 0 ? totalAudio / totalElapsed : 0;
  const port = status?.port ?? settings?.sidecarPort;

  const stats: Array<{ label: string; value: string }> = [
    { label: "使用模型", value: activeModel },
    { label: "運算裝置", value: deviceLabel(device) },
    { label: "精度", value: compute },
    { label: "連接埠", value: port ? `127.0.0.1:${port}` : "—" },
    {
      label: "引擎執行時間",
      value: details?.uptime != null ? formatUptime(details.uptime) : "—",
    },
    {
      label: "已載入模型",
      value: loaded.length > 0 ? loaded.join("、") : "尚未載入",
    },
    {
      label: "載入中",
      value: loading.length > 0 ? loading.join("、") : "—",
    },
    {
      label: "累計轉錄",
      value:
        totalTranscriptions > 0
          ? `${totalTranscriptions} 次 · ${formatDuration(totalAudio)} 音訊`
          : "尚無紀錄",
    },
    {
      label: "平均處理速度",
      value: averageSpeed > 0 ? `${averageSpeed.toFixed(1)}× 即時` : "—",
    },
    { label: "PID", value: details?.pid != null ? String(details.pid) : "—" },
    { label: "Python", value: details?.python ?? "—" },
    { label: "faster-whisper", value: details?.fasterWhisper ?? "—" },
    {
      label: "CUDA 裝置",
      value:
        (details?.cudaDevices ?? 0) > 0
          ? `${details?.cudaDevices} 個`
          : "未偵測到",
    },
  ];

  const shown = compact ? stats.slice(0, 6) : stats;

  return (
    <div className={`engine-status ${compact ? "compact" : ""}`}>
      <div className="engine-head">
        <div>
          <h3>本機轉錄引擎</h3>
          <p className="card-sub">
            faster-whisper sidecar · 僅監聽{" "}
            {port ? `127.0.0.1:${port}` : "127.0.0.1"}
          </p>
        </div>
        <div className="engine-actions">
          <span className={`status-pill ${state}`}>
            <span className={`dot ${state}`} />
            {SIDECAR_STATE_LABEL[state] ?? "未知"}
          </span>
          {onRestart ? (
            <button
              type="button"
              className="btn ghost"
              onClick={onRestart}
              disabled={restarting}
            >
              <RefreshIcon size={14} />
              {restarting ? "重新啟動中…" : "重新啟動"}
            </button>
          ) : null}
        </div>
      </div>

      {state === "error" && status?.message ? (
        <div className="inline-alert small">
          <AlertIcon size={14} />
          <span>{status.message}</span>
        </div>
      ) : (
        <p className="hint">{status?.message || "尚未取得引擎狀態。"}</p>
      )}

      {details?.lastError && details.lastError !== status?.message ? (
        <div className="inline-alert small">
          <AlertIcon size={14} />
          <span>{details.lastError}</span>
        </div>
      ) : null}

      <div className="stat-grid">
        {shown.map((stat) => (
          <Stat key={stat.label} label={stat.label} value={stat.value} />
        ))}
      </div>

      {last ? (
        <div className="last-run">
          <div className="last-run-head">
            <span>上次轉錄</span>
            <span>{formatWhen(last.at)}</span>
          </div>
          <div className="last-run-grid">
            <span>
              模型 <strong>{last.model || "—"}</strong>
            </span>
            <span>
              語言 <strong>{languageLabel(last.language)}</strong>
            </span>
            <span>
              音訊 <strong>{formatDuration(last.audioSeconds)}</strong>
            </span>
            <span>
              耗時 <strong>{last.elapsedSeconds.toFixed(2)} 秒</strong>
            </span>
            <span>
              速度 <strong>{last.realtimeFactor.toFixed(1)}×</strong>
            </span>
            <span>
              裝置{" "}
              <strong>
                {deviceLabel(last.device)}
                {last.computeType ? ` / ${last.computeType}` : ""}
              </strong>
            </span>
          </div>
        </div>
      ) : null}

      {!compact ? (
        <details className="logs">
          <summary>引擎日誌</summary>
          <pre>{status?.log?.trim() || "尚無日誌。"}</pre>
        </details>
      ) : null}
    </div>
  );
}
