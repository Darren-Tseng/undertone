export type ThemeMode = "auto" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const LIGHT_QUERY = "(prefers-color-scheme: light)";
const CACHE_KEY = "undertone.theme";

export function resolveTheme(mode: string | undefined): ResolvedTheme {
  if (mode === "light" || mode === "dark") return mode;
  if (typeof window === "undefined" || !window.matchMedia) return "dark";
  return window.matchMedia(LIGHT_QUERY).matches ? "light" : "dark";
}

/** Applies the last resolved theme before the app renders, to avoid a flash. */
export function applyCachedTheme() {
  try {
    const cached = window.localStorage.getItem(CACHE_KEY);
    if (cached !== "light" && cached !== "dark") return;
    document.documentElement.dataset.theme = cached;
    document.documentElement.style.colorScheme = cached;
  } catch {
    /* localStorage can be unavailable; the theme is applied again later */
  }
}

/**
 * Applies the theme to the document and, when the mode is "auto", keeps it in
 * sync with the operating system. Returns a cleanup function.
 */
export function applyTheme(mode: string | undefined): () => void {
  const root = document.documentElement;

  const paint = () => {
    const resolved = resolveTheme(mode);
    root.dataset.theme = resolved;
    root.style.colorScheme = resolved;
    try {
      window.localStorage.setItem(CACHE_KEY, resolved);
    } catch {
      /* ignore */
    }
  };

  paint();

  if (mode === "light" || mode === "dark") return () => {};
  if (typeof window === "undefined" || !window.matchMedia) return () => {};

  const media = window.matchMedia(LIGHT_QUERY);
  media.addEventListener("change", paint);
  return () => media.removeEventListener("change", paint);
}
