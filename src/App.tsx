import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  events,
  type DictationState,
  type RuntimeState,
  type Settings,
  type TranscriptEntry,
} from "./lib/api";
import {
  SIDECAR_STATE_LABEL,
  deviceLabel,
  hotkeyParts,
} from "./lib/format";
import { DictatePage } from "./components/DictatePage";
import { HistoryPage } from "./components/HistoryPage";
import { SettingsPage } from "./components/SettingsPage";
import {
  HistoryIcon,
  MicIcon,
  SettingsIcon,
  WaveMark,
} from "./components/icons";

type Page = "dictate" | "history" | "settings";
type Toast = { id: number; text: string; kind: "ok" | "error" };

const PAGE_META: Record<Page, { title: string; subtitle: string }> = {
  dictate: {
    title: "語音聽寫",
    subtitle: "按一下或使用全域快捷鍵，說完話文字就出現",
  },
  history: {
    title: "逐字稿",
    subtitle: "所有錄音都存成純文字，可搜尋、可用 Git 版本控制",
  },
  settings: {
    title: "設定",
    subtitle: "模型、快捷鍵與資料位置都留在這台電腦上",
  },
};

export default function App() {
  const [page, setPage] = useState<Page>("dictate");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [runtime, setRuntime] = useState<RuntimeState | null>(null);
  const [items, setItems] = useState<TranscriptEntry[]>([]);
  const [search, setSearch] = useState("");
  const [bootError, setBootError] = useState<string | null>(null);
  const [dictation, setDictation] = useState<{
    state: DictationState;
    message: string | null;
  }>({ state: "idle", message: null });
  const [toast, setToast] = useState<Toast | null>(null);

  const toastTimer = useRef<number | null>(null);
  const searchRef = useRef("");
  const recordingRef = useRef(false);

  const notify = useCallback((text: string, kind: "ok" | "error" = "ok") => {
    setToast({ id: Date.now(), text, kind });
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 3400);
  }, []);

  const loadItems = useCallback(
    async (query: string) => {
      try {
        const data = await api.transcripts(query, 200);
        setItems(data);
      } catch (error) {
        notify(String(error), "error");
      }
    },
    [notify],
  );

  useEffect(() => {
    searchRef.current = search;
  }, [search]);

  // Boot: settings + transcript index.
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [loadedSettings, loadedItems] = await Promise.all([
          api.settings(),
          api.transcripts("", 200),
        ]);
        if (!alive) return;
        setSettings(loadedSettings);
        setItems(loadedItems);
      } catch (error) {
        if (alive) setBootError(String(error));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  // Runtime polling: fast while recording so the meter feels alive.
  useEffect(() => {
    let alive = true;
    let timer = 0;
    const tick = async () => {
      try {
        const next = await api.runtime();
        if (!alive) return;
        recordingRef.current = next.recording;
        setRuntime(next);
      } catch {
        /* transient errors are fine, the next tick retries */
      }
      if (alive) {
        timer = window.setTimeout(tick, recordingRef.current ? 90 : 1200);
      }
    };
    void tick();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, []);

  // Engine health refresh every few seconds (cached status is instant).
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const status = await api.sidecarStatus();
        if (!alive) return;
        setRuntime((current) => (current ? { ...current, sidecar: status } : current));
      } catch {
        /* ignore */
      }
    };
    const timer = window.setInterval(tick, 6000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  // Tauri events from the Rust side.
  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    void (async () => {
      const offState = await events.state((payload) => {
        if (disposed) return;
        setDictation({ state: payload.state, message: payload.message ?? null });
        if (payload.state === "done") {
          void loadItems(searchRef.current);
        }
        if (payload.state === "error" && payload.message) {
          notify(payload.message, "error");
        }
      });
      if (disposed) offState();
      else unlisteners.push(offState);

      const offTranscript = await events.transcript((entry) => {
        if (disposed) return;
        setItems((current) => [
          entry,
          ...current.filter((item) => item.path !== entry.path),
        ]);
      });
      if (disposed) offTranscript();
      else unlisteners.push(offTranscript);
    })();

    return () => {
      disposed = true;
      unlisteners.forEach((off) => off());
    };
  }, [loadItems, notify]);

  // Search on the history page (debounced).
  useEffect(() => {
    if (page !== "history") return;
    const timer = window.setTimeout(() => {
      void loadItems(search);
    }, 220);
    return () => window.clearTimeout(timer);
  }, [page, search, loadItems]);

  const toggleDictation = async () => {
    try {
      if (runtime?.recording || dictation.state === "recording") {
        await api.stop();
      } else {
        await api.start();
      }
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const cancelDictation = async () => {
    try {
      await api.cancel();
      setDictation({ state: "idle", message: null });
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const copyEntry = async (entry: TranscriptEntry) => {
    try {
      await api.copy(entry.text);
      notify("已複製到剪貼簿");
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const deleteEntry = async (entry: TranscriptEntry) => {
    if (!window.confirm("刪除這則逐字稿？這個動作無法復原。")) return;
    try {
      await api.deleteTranscript(entry.path);
      setItems((current) => current.filter((item) => item.path !== entry.path));
      notify("已刪除逐字稿");
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const revealEntry = async (entry: TranscriptEntry) => {
    try {
      await api.reveal(entry.path);
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const openWorkspace = async () => {
    try {
      await api.openWorkspace();
    } catch (error) {
      notify(String(error), "error");
    }
  };

  const saveSettings = async (next: Settings) => {
    const saved = await api.saveSettings(next);
    const workspaceChanged = saved.workspaceDir !== settings?.workspaceDir;
    setSettings(saved);
    notify("設定已儲存");
    if (workspaceChanged) {
      void loadItems("");
    }
  };

  const status = runtime?.sidecar.state ?? "stopped";
  const engineDetails = runtime?.sidecar.details;
  const engineModel = engineDetails?.activeModel || settings?.model || "small";
  const engineDevice = engineDetails?.device || settings?.device;
  const engineCompute =
    engineDetails?.computeType || settings?.computeType || "auto";
  const engineSub = `${engineModel} · ${deviceLabel(engineDevice)}${
    engineCompute && engineCompute !== "auto" ? `/${engineCompute}` : ""
  }`;
  const meta = PAGE_META[page];

  if (bootError) {
    return (
      <div className="boot-screen">
        <div className="boot-card">
          <h1>Murmur 無法啟動</h1>
          <p>{bootError}</p>
          <button
            type="button"
            className="btn primary"
            onClick={() => window.location.reload()}
          >
            重試
          </button>
        </div>
      </div>
    );
  }

  if (!settings) {
    return (
      <div className="boot-screen">
        <div className="boot-card">
          <div className="boot-mark">
            <WaveMark size={26} />
          </div>
          <p>正在啟動 Murmur…</p>
        </div>
      </div>
    );
  }

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <WaveMark size={20} />
          </div>
          <div>
            <div className="brand-name">Murmur</div>
            <div className="brand-sub">本機語音聽寫</div>
          </div>
        </div>

        <nav className="nav">
          <button
            type="button"
            className={page === "dictate" ? "active" : ""}
            onClick={() => setPage("dictate")}
          >
            <MicIcon size={17} />
            聽寫
          </button>
          <button
            type="button"
            className={page === "history" ? "active" : ""}
            onClick={() => setPage("history")}
          >
            <HistoryIcon size={17} />
            逐字稿
            {items.length > 0 ? <span className="nav-count">{items.length}</span> : null}
          </button>
          <button
            type="button"
            className={page === "settings" ? "active" : ""}
            onClick={() => setPage("settings")}
          >
            <SettingsIcon size={17} />
            設定
          </button>
        </nav>

        <div className="sidebar-spacer" />

        <div className="sidebar-footer">
          <button
            type="button"
            className={`status-chip ${status}`}
            onClick={() => setPage("settings")}
            title={engineDetails?.lastError || runtime?.sidecar.message}
          >
            <span className={`dot ${status}`} />
            <span className="status-chip-copy">
              <span className="status-chip-main">
                {SIDECAR_STATE_LABEL[status] ?? "引擎狀態未知"}
              </span>
              <span className="status-chip-sub">{engineSub}</span>
            </span>
          </button>
          <p className="privacy-note">
            100% 本機處理
            <br />
            逐字稿是你的純文字檔案
          </p>
        </div>
      </aside>

      <main className="content">
        <header className="topbar">
          <div>
            <h1 className="page-title">{meta.title}</h1>
            <p className="page-sub">{meta.subtitle}</p>
          </div>

          {page === "dictate" ? (
            <div className="topbar-hotkey">
              <span className="topbar-label">快捷鍵</span>
              <span className="hotkey-caps">
                {hotkeyParts(settings.hotkey).map((part) => (
                  <kbd key={part}>{part}</kbd>
                ))}
              </span>
            </div>
          ) : null}
        </header>

        <div className="scroll">
          {page === "dictate" ? (
            <DictatePage
              settings={settings}
              runtime={runtime}
              state={dictation.state}
              message={dictation.message}
              recent={items.slice(0, 4)}
              onToggle={toggleDictation}
              onCancel={cancelDictation}
              onCopy={copyEntry}
              onOpenHistory={() => setPage("history")}
              onOpenWorkspace={openWorkspace}
            />
          ) : null}

          {page === "history" ? (
            <HistoryPage
              items={items}
              loading={false}
              search={search}
              onSearch={setSearch}
              onCopy={copyEntry}
              onDelete={deleteEntry}
              onReveal={revealEntry}
              onOpenWorkspace={openWorkspace}
            />
          ) : null}

          {page === "settings" ? (
            <SettingsPage
              settings={settings}
              runtime={runtime}
              onSave={saveSettings}
              notify={notify}
              onOpenWorkspace={openWorkspace}
            />
          ) : null}
        </div>
      </main>

      {toast ? (
        <div className={`toast ${toast.kind}`} key={toast.id}>
          {toast.text}
        </div>
      ) : null}
    </div>
  );
}
