export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function applyAppearance(theme: string, density: string): void {
  const systemDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const dark = theme === "dark" || (theme !== "light" && systemDark);
  document.documentElement.classList.toggle("dark", dark);
  document.documentElement.dataset.density = density || "default";
}

export async function openExternal(url: string): Promise<void> {
  if (!inTauri()) {
    window.open(url, "_blank", "noopener");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

/**
 * Hides the webview's own right-click menu (Back, Reload, Print...) so the window feels like an app.
 * Text fields and selected text keep it for cut, copy and paste. Dev builds keep it for Inspect.
 */
export function blockPageMenu(doc: Document): void {
  if (import.meta.env.DEV) return;
  doc.addEventListener("contextmenu", (event) => {
    const target = event.target as HTMLElement | null;
    if (target?.closest("input, textarea, [contenteditable]:not([contenteditable='false'])")) return;
    if (doc.getSelection()?.toString().trim()) return;
    event.preventDefault();
  });
}
