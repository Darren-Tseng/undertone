export const SIDECAR_STATE_LABEL: Record<string, string> = {
  ready: "轉錄引擎就緒",
  starting: "引擎啟動中",
  loading: "正在載入模型",
  stopped: "引擎未啟動",
  error: "引擎異常",
};

export function deviceLabel(device?: string | null): string {
  if (!device || device === "auto") return "自動";
  if (device === "cuda") return "GPU";
  return device.toUpperCase();
}

const LANGUAGE_LABEL: Record<string, string> = {
  auto: "自動語言",
  zh: "中文",
  en: "English",
  ja: "日本語",
  ko: "한국어",
  yue: "粵語",
  fr: "Français",
  de: "Deutsch",
  es: "Español",
};

export function languageLabel(code?: string | null): string {
  if (!code) return "自動語言";
  return LANGUAGE_LABEL[code] ?? code;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(
    units.length - 1,
    Math.floor(Math.log(bytes) / Math.log(1024)),
  );
  const value = bytes / 1024 ** index;
  const digits = index === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[index]}`;
}

export function formatUptime(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  if (hours > 0) return `${hours} 小時 ${minutes} 分`;
  if (minutes > 0) return `${minutes} 分 ${rest} 秒`;
  return `${rest} 秒`;
}

export function formatTimer(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(total / 60);
  const rest = total % 60;
  return `${String(minutes).padStart(2, "0")}:${String(rest).padStart(2, "0")}`;
}

export function formatDuration(seconds: number): string {
  if (!seconds || seconds < 1) {
    return `${Math.max(1, Math.round(seconds))} 秒`;
  }
  if (seconds < 60) {
    return `${seconds.toFixed(1)} 秒`;
  }
  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return `${minutes} 分 ${rest} 秒`;
}

function startOfDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

export function formatWhen(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return "";
  }
  const now = new Date();
  const days = Math.round((startOfDay(now) - startOfDay(date)) / 86_400_000);
  const time = date.toLocaleTimeString("zh-TW", {
    hour: "2-digit",
    minute: "2-digit",
  });

  if (days === 0) return `今天 ${time}`;
  if (days === 1) return `昨天 ${time}`;
  if (days < 7) return `${days} 天前 ${time}`;
  return date.toLocaleDateString("zh-TW", {
    month: "long",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function hotkeyParts(hotkey: string): string[] {
  return hotkey
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const key = part.toLowerCase();
      if (key === "control" || key === "ctrl") return "Ctrl";
      if (key === "alt" || key === "option") return "Alt";
      if (key === "shift") return "Shift";
      if (key === "super" || key === "cmd" || key === "command") return "Win";
      return part;
    });
}

const CODE_ALIASES: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Escape: "Escape",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Minus: "Minus",
  Equal: "Equal",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight",
  Backslash: "Backslash",
  Semicolon: "Semicolon",
  Quote: "Quote",
  Comma: "Comma",
  Period: "Period",
  Slash: "Slash",
  Backquote: "Backquote",
};

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
]);

/** Turns a keyboard event into a string the global-shortcut plugin accepts. */
export function comboFromEvent(event: React.KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(event.code)) {
    return null;
  }

  const modifiers: string[] = [];
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Super");

  let key: string | null = null;
  if (/^Key[A-Z]$/.test(event.code)) {
    key = event.code.slice(3);
  } else if (/^Digit[0-9]$/.test(event.code)) {
    key = event.code.slice(5);
  } else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(event.code)) {
    key = event.code;
  } else if (CODE_ALIASES[event.code]) {
    key = CODE_ALIASES[event.code];
  }

  if (!key) {
    return null;
  }
  if (modifiers.length === 0) {
    return null;
  }
  return [...modifiers, key].join("+");
}
