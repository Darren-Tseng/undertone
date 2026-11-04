import { useEffect, useState } from "react";
import { WaveMark } from "./icons";

interface Props {
  onResolve: (action: "minimize" | "quit", remember: boolean) => void;
  onCancel: () => void;
}

/**
 * Confirmation shown when the main window's close button is pressed and the
 * close action is set to "ask". Esc or the backdrop keeps the window open.
 */
export function CloseDialog({ onResolve, onCancel }: Props) {
  const [remember, setRemember] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onCancel}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="close-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="dialog-head">
          <div className="dialog-mark">
            <WaveMark size={18} />
          </div>
          <div>
            <h2 id="close-dialog-title">要關閉 Undertone 嗎？</h2>
            <p>
              最小化後仍可在系統匣使用全域快捷鍵聽寫；直接關閉會結束程式並停止轉錄引擎。
            </p>
          </div>
        </div>

        <label className="dialog-check">
          <input
            type="checkbox"
            checked={remember}
            onChange={(event) => setRemember(event.target.checked)}
          />
          <span>記住我的選擇，下次不再詢問</span>
        </label>

        <div className="dialog-actions">
          <button type="button" className="btn ghost" onClick={onCancel}>
            取消
          </button>
          <button
            type="button"
            className="btn"
            autoFocus
            onClick={() => onResolve("minimize", remember)}
          >
            最小化到系統匣
          </button>
          <button
            type="button"
            className="btn danger"
            onClick={() => onResolve("quit", remember)}
          >
            直接關閉
          </button>
        </div>
      </div>
    </div>
  );
}
