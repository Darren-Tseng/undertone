import { useEffect, useState } from "react";
import { api, type Settings } from "../lib/api";
import { hotkeyParts } from "../lib/format";
import { ExternalIcon, WaveMark } from "./icons";

const REPO_URL = "https://github.com/Darren-Tseng/undertone";
const DATA_ROOT = "%APPDATA%\\so.undertone.app";

const LINKS = [
  { label: "GitHub 專案", url: REPO_URL },
  { label: "版本下載（Releases）", url: `${REPO_URL}/releases` },
  { label: "faster-whisper", url: "https://github.com/SYSTRAN/faster-whisper" },
  { label: "Tauri", url: "https://tauri.app/" },
  { label: "Epicenter（靈感來源）", url: "https://epicenter.so/" },
];

interface Props {
  settings: Settings;
  notify: (text: string, kind?: "ok" | "error") => void;
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="about-fact">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

export function AboutPage({ settings, notify }: Props) {
  const [version, setVersion] = useState("");

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const value = await api.appVersion();
        if (alive) setVersion(value);
      } catch {
        /* the version badge simply stays empty */
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const openLink = async (url: string) => {
    try {
      await api.openUrl(url);
    } catch (error) {
      notify(String(error), "error");
    }
  };

  return (
    <div className="page about-page">
      <div className="settings-grid">
        <section className="section-card span-2 about-hero">
          <div className="about-mark">
            <WaveMark size={26} />
          </div>
          <div className="about-hero-copy">
            <div className="about-name-row">
              <h2>Undertone</h2>
              <span className="badge accent">
                {version ? `v${version}` : "版本載入中"}
              </span>
            </div>
            <p>
              Windows 專用的本機語音聽寫工具。按下全域快捷鍵說話，文字會直接出現在你正在使用的程式裡。
            </p>
            <p className="hint">
              沒有帳號、沒有訂閱，音訊不會離開這台電腦；逐字稿是你可以自行備份與版本控制的純文字檔。
            </p>
          </div>
          <div className="about-hero-actions">
            <button
              type="button"
              className="btn ghost"
              onClick={() => void openLink(REPO_URL)}
            >
              <ExternalIcon size={14} />
              GitHub
            </button>
          </div>
        </section>

        <section className="section-card">
          <h3>這個專案在做什麼</h3>
          <p className="card-sub">把「說話」變成任何程式裡都貼得上的文字。</p>
          <ul className="about-list">
            <li>用全域快捷鍵在任何程式裡開始／停止聽寫，不必切換視窗。</li>
            <li>
              錄音只留在本機，由 <code>127.0.0.1</code> 上的 Python 引擎用
              faster-whisper 轉錄。
            </li>
            <li>轉錄完成後自動貼到游標位置，也可以選擇只複製到剪貼簿。</li>
            <li>
              逐字稿存成 Markdown，可用 Obsidian、VS Code、grep 或 Git 管理。
            </li>
            <li>
              支援繁體中文輸出、模型切換與 NVIDIA GPU 加速；GPU
              元件可選擇性下載，不裝也能用 CPU。
            </li>
          </ul>
        </section>

        <section className="section-card">
          <h3>怎麼使用</h3>
          <p className="card-sub">三步驟，之後都靠同一個快捷鍵。</p>
          <ol className="about-steps">
            <li>
              按一下{" "}
              <span className="hotkey-caps about-kbd">
                {hotkeyParts(settings.hotkey).map((part) => (
                  <kbd key={part}>{part}</kbd>
                ))}
              </span>{" "}
              開始錄音。
            </li>
            <li>說完話再按一次，浮動小窗會顯示正在轉錄。</li>
            <li>文字自動貼到游標位置，逐字稿同時存進工作區。</li>
          </ol>
        </section>

        <section className="section-card span-2">
          <h3>資料放在哪裡</h3>
          <p className="card-sub">
            所有資料都在這台電腦上，可以直接備份或刪除。
          </p>
          <dl className="about-facts">
            <Fact label="設定檔" value={`${DATA_ROOT}\\settings.json`} />
            <Fact label="模型快取" value={`${DATA_ROOT}\\models\\`} />
            <Fact label="GPU 元件" value={`${DATA_ROOT}\\cuda\\`} />
            <Fact
              label="逐字稿"
              value={`${settings.workspaceDir}\\transcripts\\`}
            />
            <Fact
              label="轉錄引擎"
              value={`http://127.0.0.1:${settings.sidecarPort}`}
            />
          </dl>
        </section>

        <section className="section-card">
          <h3>技術組成</h3>
          <p className="card-sub">桌面外殼加上本機轉錄引擎。</p>
          <dl className="about-facts">
            <Fact label="介面" value="React 19 + Vite" />
            <Fact label="桌面層" value="Tauri 2 / Rust" />
            <Fact label="語音引擎" value="Python + faster-whisper" />
            <Fact label="錄音／貼上" value="WASAPI (cpal) / enigo" />
            <Fact label="音訊解碼" value="PyAV（不需要 ffmpeg）" />
          </dl>
        </section>

        <section className="section-card">
          <h3>版本與授權</h3>
          <p className="card-sub">開源專案，程式碼採用 MIT 授權。</p>
          <dl className="about-facts">
            <Fact label="版本" value={version ? `v${version}` : "—"} />
            <Fact label="識別碼" value="so.undertone.app" />
            <Fact label="授權" value="MIT" />
            <Fact label="第三方元件" value="THIRD-PARTY-NOTICES.md" />
          </dl>
        </section>

        <section className="section-card span-2">
          <h3>相關連結</h3>
          <p className="card-sub">原始碼、版本下載，以及使用的開源元件。</p>
          <div className="about-links">
            {LINKS.map((link) => (
              <button
                key={link.url}
                type="button"
                className="about-link"
                onClick={() => void openLink(link.url)}
              >
                <ExternalIcon size={14} />
                <span className="about-link-label">{link.label}</span>
                <span className="about-link-url">
                  {link.url.replace(/^https?:\/\//, "")}
                </span>
              </button>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
