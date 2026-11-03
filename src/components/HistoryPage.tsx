import { useMemo } from "react";
import { CopyIcon, FolderIcon, SearchIcon, TrashIcon } from "./icons";
import { formatDuration, formatWhen } from "../lib/format";
import type { TranscriptEntry } from "../lib/api";

interface Props {
  items: TranscriptEntry[];
  loading: boolean;
  search: string;
  onSearch: (value: string) => void;
  onCopy: (entry: TranscriptEntry) => void;
  onDelete: (entry: TranscriptEntry) => void;
  onReveal: (entry: TranscriptEntry) => void;
  onOpenWorkspace: () => void;
}

function dayKey(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "unknown";
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
}

function dayLabel(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "較早";
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const that = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  const days = Math.round((today.getTime() - that.getTime()) / 86_400_000);
  if (days === 0) return "今天";
  if (days === 1) return "昨天";
  if (days < 7) return `${days} 天前`;
  return date.toLocaleDateString("zh-TW", {
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
    month: "long",
    day: "numeric",
  });
}

export function HistoryPage({
  items,
  loading,
  search,
  onSearch,
  onCopy,
  onDelete,
  onReveal,
  onOpenWorkspace,
}: Props) {
  const groups = useMemo(() => {
    const map = new Map<string, TranscriptEntry[]>();
    for (const item of items) {
      const key = dayKey(item.created);
      const bucket = map.get(key);
      if (bucket) bucket.push(item);
      else map.set(key, [item]);
    }
    return Array.from(map.entries());
  }, [items]);

  return (
    <div className="page">
      <div className="toolbar">
        <div className="search-wrap">
          <SearchIcon size={15} />
          <input
            className="search-input"
            value={search}
            onChange={(event) => onSearch(event.target.value)}
            placeholder="搜尋逐字稿…"
            spellCheck={false}
          />
        </div>
        <button type="button" className="btn ghost" onClick={onOpenWorkspace}>
          <FolderIcon size={14} />
          開啟工作區
        </button>
      </div>

      <div className="section-head compact">
        <div>
          <h2 className="section-title">
            {search ? `搜尋「${search}」` : "所有逐字稿"}
          </h2>
          <p className="section-sub">
            {loading ? "讀取中…" : `${items.length} 則 · 依時間排序`}
          </p>
        </div>
      </div>

      {items.length === 0 && !loading ? (
        <div className="empty">
          <p>{search ? "找不到符合的內容。" : "還沒有逐字稿。"}</p>
          <p className="muted">
            {search
              ? "換個關鍵字試試，或清除搜尋。"
              : "按一下麥克風，開始第一段聽寫。"}
          </p>
        </div>
      ) : (
        groups.map(([key, entries]) => (
          <section key={key} className="day-group">
            <h3 className="day-label">{dayLabel(entries[0].created)}</h3>
            <div className="transcript-list">
              {entries.map((entry) => (
                <article className="tcard" key={entry.path}>
                  <div className="tcard-main">
                    <p className="tcard-text clamp">
                      {entry.text}
                    </p>
                    <div className="tmeta">
                      <span>{formatWhen(entry.created)}</span>
                      <span>{formatDuration(entry.duration)}</span>
                      <span>{entry.model || "whisper"}</span>
                      <span>{entry.language || "auto"}</span>
                    </div>
                  </div>
                  <div className="tcard-actions">
                    <button
                      type="button"
                      className="icon-btn"
                      title="複製文字"
                      onClick={() => onCopy(entry)}
                    >
                      <CopyIcon />
                    </button>
                    <button
                      type="button"
                      className="icon-btn"
                      title="在資料夾中顯示"
                      onClick={() => onReveal(entry)}
                    >
                      <FolderIcon />
                    </button>
                    <button
                      type="button"
                      className="icon-btn danger"
                      title="刪除"
                      onClick={() => onDelete(entry)}
                    >
                      <TrashIcon />
                    </button>
                  </div>
                </article>
              ))}
            </div>
          </section>
        ))
      )}
    </div>
  );
}
