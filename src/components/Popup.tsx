import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type PopupSnapshot } from "../api";
import { initials } from "../format";
import { applyAppearance } from "../platform";
import type { Notice } from "../types";

const LIFETIME_MS = 8000;

/** The Telegram-style corner pop-up. Runs in its own borderless window labelled "popup". */
export function PopupApp() {
  const [notices, setNotices] = useState<Notice[]>([]);
  const [corner, setCorner] = useState("bottomRight");
  const stackRef = useRef<HTMLDivElement>(null);
  const versionRef = useRef(-1);

  useEffect(() => {
    document.documentElement.classList.add("popup-window");
    let stop = false;
    let unlisten: (() => void) | null = null;
    // A list reply and a change event can arrive in either order; keep only the newest.
    const apply = (snapshot: PopupSnapshot) => {
      if (snapshot.version <= versionRef.current) return;
      versionRef.current = snapshot.version;
      setNotices(snapshot.notices);
      setCorner(snapshot.corner || "bottomRight");
    };
    void (async () => {
      try {
        const settings = await api.settingsGet();
        applyAppearance(settings.theme, settings.density);
      } catch {
        /* default look is fine */
      }
      // Listen before reading the list so nothing pushed in between is missed.
      const { listen } = await import("@tauri-apps/api/event");
      const off = await listen<PopupSnapshot>("popup://changed", (event) => apply(event.payload));
      if (stop) {
        off();
        return;
      }
      unlisten = off;
      apply(await api.popupList());
    })();
    return () => {
      stop = true;
      unlisten?.();
    };
  }, []);

  // Size the window to the cards whenever they change.
  useLayoutEffect(() => {
    const el = stackRef.current;
    if (!el) return;
    const report = () => void api.popupLayout(Math.ceil(el.getBoundingClientRect().height)).catch(() => {});
    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, [notices, corner]);

  // Newest sits nearest the corner: at the bottom for bottom corners, on top otherwise.
  const ordered = corner.startsWith("top") ? [...notices].reverse() : notices;
  return (
    <div ref={stackRef} className="flex flex-col gap-2 p-2" role="region" aria-label="Novabox notifications" aria-live="polite">
      {ordered.map((notice) => (
        <PopupCard key={notice.id} notice={notice} />
      ))}
    </div>
  );
}

function PopupCard({ notice }: { notice: Notice }) {
  const [hovered, setHovered] = useState(false);
  const [busy, setBusy] = useState(false);
  const canOpen = Boolean(notice.accountId && notice.threadId);
  const isMail = notice.kind === "mail" && canOpen;
  const heading = notice.kind === "mail" ? (notice.sender ?? notice.accountEmail ?? notice.title) : notice.title;
  const sub = notice.kind === "mail" && notice.sender ? notice.accountEmail : null;

  useEffect(() => {
    if (hovered || busy) return;
    const timer = window.setTimeout(() => void api.popupDismiss(notice.id), LIFETIME_MS);
    return () => window.clearTimeout(timer);
  }, [hovered, busy, notice.id]);

  const run = async (action: string) => {
    if (!notice.accountId || !notice.threadId) return;
    setBusy(true);
    try {
      await api.threadsModify([{ accountId: notice.accountId, threadId: notice.threadId }], action);
    } catch (error) {
      console.error(error);
    }
    await api.popupDismiss(notice.id);
  };

  return (
    <div
      className="relative flex gap-3 rounded-xl border border-line bg-panel p-3 pr-8 text-ink shadow-[0_6px_20px_rgba(0,0,0,0.22)]"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <span className="grid h-10 w-10 shrink-0 place-items-center rounded-full text-xs font-semibold text-white" style={{ background: notice.accountColor || "#241c14" }} aria-hidden>
        {initials(notice.sender ?? "", notice.accountEmail ?? "Novabox").slice(0, 3)}
      </span>
      <div className="min-w-0 flex-1">
        <button
          type="button"
          className="block w-full text-left"
          onClick={() => void (canOpen ? api.popupOpen(notice) : api.popupDismiss(notice.id))}
        >
          <p className="truncate text-sm font-semibold">{heading}</p>
          {sub && <p className="truncate text-xs text-muted">{sub}</p>}
          <p className={`line-clamp-2 text-sm ${notice.kind === "error" ? "text-danger" : "text-muted"}`}>{notice.body}</p>
        </button>
        {isMail && (
          <div className="mt-2 flex gap-1">
            <button type="button" disabled={busy} className="rounded-lg px-2 py-1 text-xs font-medium text-accent hover:bg-hover disabled:opacity-50" onClick={() => void run("read")}>
              Mark as read
            </button>
            <button type="button" disabled={busy} className="rounded-lg px-2 py-1 text-xs font-medium text-muted hover:bg-hover disabled:opacity-50" onClick={() => void run("archive")}>
              Archive
            </button>
          </div>
        )}
      </div>
      <button type="button" className="absolute right-2 top-2 grid h-6 w-6 place-items-center rounded-full text-muted hover:bg-hover" aria-label="Dismiss notification" onClick={() => void api.popupDismiss(notice.id)}>
        ×
      </button>
    </div>
  );
}
