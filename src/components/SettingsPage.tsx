import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertIcon,
  FolderIcon,
  RefreshIcon,
  SpinnerIcon,
  TrashIcon,
} from "./icons";
import { comboFromEvent, formatBytes, hotkeyParts } from "../lib/format";
import {
  api,
  type ModelInfo,
  type ModelsOverview,
  type RuntimeState,
  type Settings,
} from "../lib/api";
import { EngineStatus } from "./EngineStatus";

interface Props {
  settings: Settings;
  runtime: RuntimeState | null;
  onSave: (next: Settings) => Promise<void>;
  notify: (text: string, kind?: "ok" | "error") => void;
  onOpenWorkspace: () => void;
}

const LANGUAGES: Array<{ value: string; label: string }> = [
  { value: "auto", label: "自動偵測" },
  { value: "zh", label: "中文（繁體輸出）" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "yue", label: "粵語" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
  { value: "es", label: "Español" },
];

const DEVICES = [
  { value: "auto", label: "自動（有 GPU 就用）" },
  { value: "cuda", label: "NVIDIA GPU（CUDA）" },
  { value: "cpu", label: "CPU" },
];

const COMPUTE_TYPES = [
  { value: "auto", label: "自動" },
  { value: "int8", label: "int8（最省資源）" },
  { value: "int8_float16", label: "int8_float16" },
  { value: "float16", label: "float16（GPU 建議）" },
  { value: "float32", label: "float32（最精確、最慢）" },
];

function HotkeyInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const [capturing, setCapturing] = useState(false);
  const parts = hotkeyParts(value);

  return (
    <div
      className={`hotkey-box ${capturing ? "capturing" : ""}`}
      tabIndex={0}
      onFocus={() => setCapturing(true)}
      onBlur={() => setCapturing(false)}
      onKeyDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.key === "Escape") {
          setCapturing(false);
          (event.currentTarget as HTMLElement).blur();
          return;
        }
        const combo = comboFromEvent(event);
        if (combo) {
          onChange(combo);
          setCapturing(false);
          (event.currentTarget as HTMLElement).blur();
        }
      }}
    >
      {capturing ? (
        <span className="hotkey-placeholder">按下想要的組合鍵（需含 Ctrl / Alt / Shift）</span>
      ) : (
        <span className="hotkey-caps">
          {parts.map((part) => (
            <kbd key={part}>{part}</kbd>
          ))}
        </span>
      )}
      <span className="hotkey-edit">{capturing ? "ESC 取消" : "點擊變更"}</span>
    </div>
  );
}

function Switch({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      className="switch"
      data-on={checked}
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    />
  );
}

function Meter({ value, max = 5 }: { value: number; max?: number }) {
  return (
    <span className="meter" aria-label={`${value} / ${max}`}>
      {Array.from({ length: max }, (_, index) => (
        <i key={index} className={index < value ? "on" : ""} />
      ))}
    </span>
  );
}

export function SettingsPage({
  settings,
  runtime,
  onSave,
  notify,
  onOpenWorkspace,
}: Props) {
  const [draft, setDraft] = useState<Settings>(settings);
  const [saving, setSaving] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [devices, setDevices] = useState<string[]>([]);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [devicesLoading, setDevicesLoading] = useState(false);
  const [models, setModels] = useState<ModelsOverview | null>(null);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [busyModel, setBusyModel] = useState<string | null>(null);

  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  const refreshDevices = useCallback(async () => {
    setDevicesLoading(true);
    try {
      const list = await api.devices();
      setDevices(list);
      setDeviceError(null);
    } catch (error) {
      setDeviceError(String(error));
    } finally {
      setDevicesLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshDevices();
  }, [refreshDevices]);

  const refreshModels = useCallback(async () => {
    setModelsLoading(true);
    try {
      const data = await api.listModels();
      setModels(data);
      setModelError(null);
    } catch (error) {
      setModelError(String(error));
    } finally {
      setModelsLoading(false);
    }
  }, []);

  const engineSignature = `${
    runtime?.sidecar.state ?? ""
  }|${runtime?.sidecar.details?.loadedModels?.join(",") ?? ""}|${
    runtime?.sidecar.details?.loadingModels?.join(",") ?? ""
  }`;

  useEffect(() => {
    void refreshModels();
  }, [refreshModels, settings.model, engineSignature]);

  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(settings),
    [draft, settings],
  );

  const patch = <K extends keyof Settings>(key: K, value: Settings[K]) => {
    setDraft((current) => ({ ...current, [key]: value }));
  };

  const save = async () => {
    setSaving(true);
    try {
      await onSave(draft);
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setSaving(false);
    }
  };

  const restart = async () => {
    setRestarting(true);
    try {
      await api.restartSidecar();
      notify("已重新啟動轉錄引擎");
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setRestarting(false);
    }
  };

  const prepareModel = async (id: string) => {
    setBusyModel(id);
    try {
      await api.prepareModel(id);
      notify(`${id} 模型已下載並載入`);
      await refreshModels();
    } catch (error) {
      notify(String(error), "error");
    } finally {
      setBusyModel(null);
    }
  };

  const removeModel = async (model: ModelInfo) => {
    if (
      !window.confirm(
        `刪除 ${model.label} 的模型檔案？下次使用時需要重新下載。`,
      )
    ) {
      return;
    }
    try {
      await api.deleteModel(model.id);
      notify("已刪除模型檔案");
      await refreshModels();
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const openModelsDir = async () => {
    try {
      await api.openModelsDir();
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const selectedModel = models?.models.find(
    (model) => model.id === draft.model,
  );

  return (
    <div className="page settings-page">
      <div className="settings-grid">
        <section className="section-card">
          <h3>聽寫行為</h3>
          <p className="card-sub">快捷鍵在任何程式裡都能使用。</p>

          <div className="field">
            <label>全域快捷鍵</label>
            <HotkeyInput
              value={draft.hotkey}
              onChange={(value) => patch("hotkey", value)}
            />
            <p className="hint">按一次開始錄音，再按一次結束並轉錄。</p>
          </div>

          <div className="row-field">
            <div>
              <label>自動貼上</label>
              <p className="hint">轉錄完成後，在你的游標位置貼上文字。</p>
            </div>
            <Switch
              checked={draft.autoPaste}
              onChange={(value) => patch("autoPaste", value)}
            />
          </div>

          <div className="row-field">
            <div>
              <label>複製到剪貼簿</label>
              <p className="hint">即使不自動貼上，也把文字留在剪貼簿。</p>
            </div>
            <Switch
              checked={draft.copyToClipboard}
              onChange={(value) => patch("copyToClipboard", value)}
            />
          </div>

          <div className="field">
            <label>最長錄音時間</label>
            <div className="inline-field">
              <input
                className="input narrow"
                type="number"
                min={5}
                max={3600}
                value={draft.maxRecordSecs}
                onChange={(event) =>
                  patch("maxRecordSecs", Number(event.target.value) || 300)
                }
              />
              <span className="unit">秒</span>
            </div>
            <p className="hint">超過時間會自動停止並轉錄，避免忘記關閉。</p>
          </div>
        </section>

        <section className="section-card">
          <div className="engine-head">
            <div>
              <h3>麥克風</h3>
              <p className="card-sub">錄音使用系統音訊裝置（WASAPI）。</p>
            </div>
            <div className="engine-actions">
              <button
                type="button"
                className="btn ghost small"
                onClick={() => void refreshDevices()}
                disabled={devicesLoading}
              >
                {devicesLoading ? (
                  <SpinnerIcon size={14} />
                ) : (
                  <RefreshIcon size={14} />
                )}
                {devicesLoading ? "偵測中…" : "重新整理"}
              </button>
            </div>
          </div>

          <div className="field">
            <label>輸入裝置</label>
            <select
              className="input"
              value={draft.inputDevice ?? ""}
              onChange={(event) =>
                patch("inputDevice", event.target.value || null)
              }
            >
              <option value="">系統預設麥克風</option>
              {draft.inputDevice &&
              !devices.includes(draft.inputDevice) ? (
                <option value={draft.inputDevice}>
                  {draft.inputDevice}（目前未偵測到）
                </option>
              ) : null}
              {devices.map((device) => (
                <option key={device} value={device}>
                  {device}
                </option>
              ))}
            </select>
            {deviceError ? (
              <p className="hint error-text">無法列出麥克風：{deviceError}</p>
            ) : devices.length === 0 ? (
              <p className="hint">目前沒有偵測到其他麥克風。</p>
            ) : (
              <p className="hint">{devices.length} 個可用的輸入裝置。</p>
            )}
          </div>
        </section>

        <section className="section-card span-2">
          <div className="engine-head">
            <div>
              <h3>模型管理</h3>
              <p className="card-sub">
                下載、預先載入或移除本機模型；下載完成後即可完全離線使用。
              </p>
            </div>
            <div className="engine-actions">
              <span className="model-cache">
                {formatBytes(models?.totalBytes ?? 0)} ·{" "}
                {models?.downloadedCount ?? 0} 個已下載
              </span>
              <button
                type="button"
                className="btn ghost"
                onClick={() => void refreshModels()}
                disabled={modelsLoading}
              >
                {modelsLoading ? (
                  <SpinnerIcon size={14} />
                ) : (
                  <RefreshIcon size={14} />
                )}
                {modelsLoading ? "掃描中…" : "重新整理"}
              </button>
              <button
                type="button"
                className="btn ghost"
                onClick={() => void openModelsDir()}
              >
                <FolderIcon size={14} />
                模型資料夾
              </button>
            </div>
          </div>

          <p className="hint model-path">
            {models?.cacheDir || "尚未取得模型資料夾"}
          </p>

          {modelError ? (
            <div className="inline-alert small">
              <AlertIcon size={14} />
              <span>{modelError}</span>
            </div>
          ) : null}

          {modelsLoading && !models ? (
            <p className="hint">正在掃描本機模型…</p>
          ) : (
            <div className="model-list">
              {(models?.models ?? []).map((model) => {
                const selected = draft.model === model.id;
                const active = settings.model === model.id;
                const canDelete =
                  model.downloaded &&
                  !active &&
                  !model.loaded &&
                  !model.loading;
                const canPrepare = !model.loaded && !model.loading;
                return (
                  <div
                    className={`model-row ${selected ? "selected" : ""}`}
                    key={model.id}
                  >
                    <button
                      type="button"
                      className="model-select"
                      onClick={() => patch("model", model.id)}
                    >
                      <span
                        className={`model-radio ${selected ? "on" : ""}`}
                      />
                      <span className="model-main">
                        <span className="model-title">
                          <strong>{model.label}</strong>
                          {model.recommended ? (
                            <span className="badge accent">建議</span>
                          ) : null}
                          {!model.multilingual ? (
                            <span className="badge">英文</span>
                          ) : null}
                          {active ? (
                            <span className="badge live">使用中</span>
                          ) : null}
                          {model.loaded ? (
                            <span className="badge">已載入</span>
                          ) : null}
                          {model.loading ? (
                            <span className="badge warn">載入中</span>
                          ) : null}
                        </span>
                        <span className="model-desc">{model.description}</span>
                        <span className="model-meta">
                          <span>
                            {model.downloaded
                              ? `${formatBytes(model.bytes)} 已下載`
                              : `約 ${formatBytes(model.approxBytes)}`}
                          </span>
                          <span className="model-meter">
                            速度 <Meter value={model.speed} />
                          </span>
                          <span className="model-meter">
                            準確度 <Meter value={model.accuracy} />
                          </span>
                        </span>
                      </span>
                    </button>
                    <div className="model-actions">
                      {canPrepare ? (
                        <button
                          type="button"
                          className="btn ghost small"
                          disabled={busyModel !== null}
                          onClick={() => void prepareModel(model.id)}
                        >
                          {busyModel === model.id
                            ? "處理中…"
                            : model.downloaded
                              ? "預先載入"
                              : "下載"}
                        </button>
                      ) : null}
                      {canDelete ? (
                        <button
                          type="button"
                          className="icon-btn danger"
                          title="刪除模型檔案"
                          disabled={busyModel !== null}
                          onClick={() => void removeModel(model)}
                        >
                          <TrashIcon size={15} />
                        </button>
                      ) : null}
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          <p className="hint model-footnote">
            {selectedModel && !selectedModel.downloaded
              ? `所選的 ${selectedModel.label} 尚未下載，儲存設定或按「下載」後會開始下載（約 ${formatBytes(
                  selectedModel.approxBytes,
                )}）。`
              : "儲存設定後，切換模型會自動下載並載入；刪除模型只會移除本機快取，不影響逐字稿。"}
          </p>
        </section>

        <section className="section-card">
          <h3>語言與運算</h3>
          <p className="card-sub">完全在本機執行，音訊不會上傳。</p>

          <div className="field">
            <label>語言</label>
            <select
              className="input"
              value={draft.language}
              onChange={(event) => patch("language", event.target.value)}
            >
              {LANGUAGES.map((language) => (
                <option key={language.value} value={language.value}>
                  {language.label}
                </option>
              ))}
            </select>
            <p className="hint">指定語言通常比自動偵測更快、更準。</p>
          </div>

          <div className="two-col">
            <div className="field">
              <label>運算裝置</label>
              <select
                className="input"
                value={draft.device}
                onChange={(event) => patch("device", event.target.value)}
              >
                {DEVICES.map((device) => (
                  <option key={device.value} value={device.value}>
                    {device.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>精度</label>
              <select
                className="input"
                value={draft.computeType}
                onChange={(event) => patch("computeType", event.target.value)}
              >
                {COMPUTE_TYPES.map((type) => (
                  <option key={type.value} value={type.value}>
                    {type.label}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className="field">
            <label>提示詞（進階）</label>
            <textarea
              className="input textarea"
              rows={2}
              value={draft.initialPrompt}
              onChange={(event) => patch("initialPrompt", event.target.value)}
              placeholder="例如：以下是繁體中文的逐字稿，請使用正確的標點符號。"
            />
            <p className="hint">
              提示詞會影響用字與標點，可留空。中文建議保留繁體提示。
            </p>
          </div>
        </section>

        <section className="section-card">
          <h3>工作區</h3>
          <p className="card-sub">
            逐字稿是純文字 Markdown，可直接用 Obsidian、VS Code 或 grep 搜尋。
          </p>

          <div className="field">
            <label>資料夾</label>
            <div className="inline-field">
              <input
                className="input"
                value={draft.workspaceDir}
                onChange={(event) => patch("workspaceDir", event.target.value)}
                spellCheck={false}
              />
              <button type="button" className="btn ghost" onClick={onOpenWorkspace}>
                <FolderIcon size={14} />
                開啟
              </button>
            </div>
            <p className="hint">
              逐字稿會存到 <code>transcripts/YYYY-MM-DD/HHMMSS.md</code>。
            </p>
          </div>
        </section>

        <section className="section-card span-2">
          <EngineStatus
            status={runtime?.sidecar ?? null}
            settings={draft}
            onRestart={restart}
            restarting={restarting}
          />

          <div className="field engine-port">
            <label>連接埠</label>
            <input
              className="input narrow"
              type="number"
              min={1024}
              max={65535}
              value={draft.sidecarPort}
              onChange={(event) =>
                patch("sidecarPort", Number(event.target.value) || 8756)
              }
            />
            <p className="hint">變更後會自動重啟引擎。</p>
          </div>
        </section>

        <section className="section-card span-2">
          <h3>進階</h3>
          <div className="field">
            <label>Python 執行檔（選填）</label>
            <input
              className="input"
              value={draft.pythonPath}
              onChange={(event) => patch("pythonPath", event.target.value)}
              placeholder="留空自動使用 sidecar\.venv 或系統 Python"
              spellCheck={false}
            />
            <p className="hint">
              只有在自行打包 sidecar 或使用其他 Python 環境時才需要填。
            </p>
          </div>
        </section>
      </div>

      <div className={`settings-actions ${dirty ? "visible" : ""}`}>
        <span className="dirty-hint">{dirty ? "有尚未儲存的變更" : "設定已同步"}</span>
        <button
          type="button"
          className="btn ghost"
          onClick={() => setDraft(settings)}
          disabled={!dirty || saving}
        >
          還原
        </button>
        <button
          type="button"
          className="btn primary"
          onClick={save}
          disabled={!dirty || saving}
        >
          {saving ? "儲存中…" : "儲存設定"}
        </button>
      </div>
    </div>
  );
}
