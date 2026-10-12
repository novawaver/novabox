import { useEffect, useState } from "react";
import { initials } from "../format";
import { useMail } from "../state/MailProvider";
import type { Notice } from "../types";

const LIFETIME_MS = 7000;

function isInFront() {
  return document.visibilityState === "visible" && document.hasFocus();
}

// Whether Novabox is the active, visible window. Notices only count down while it is.
function useInFront() {
  const [inFront, setInFront] = useState(isInFront);
  useEffect(() => {
    const update = () => setInFront(isInFront());
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    document.addEventListener("visibilitychange", update);
    return () => {
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      document.removeEventListener("visibilitychange", update);
    };
  }, []);
  return inFront;
}

export function NoticeStack() {
  const mail = useMail();
  const inFront = useInFront();
  if (mail.notices.length === 0) return null;
  return (
    <div className="pointer-events-none fixed right-4 top-4 z-50 flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2" role="region" aria-label="Notifications" aria-live="polite">
      {mail.notices.map((notice) => (
        <NoticeCard key={notice.id} notice={notice} inFront={inFront} />
      ))}
    </div>
  );
}

function NoticeCard({ notice, inFront }: { notice: Notice; inFront: boolean }) {
  const mail = useMail();
  const [paused, setPaused] = useState(false);
  const account = mail.accounts.find((item) => item.id === notice.accountId);
  const color = account?.color ?? notice.accountColor ?? "#241c14";
  const canOpen = Boolean(notice.accountId && notice.threadId);
  const heading = notice.kind === "mail" ? (notice.sender ?? account?.email ?? notice.title) : notice.title;
  const sub = notice.kind === "mail" && notice.sender ? account?.email : null;

  useEffect(() => {
    if (paused || !inFront) return;
    const timer = window.setTimeout(() => mail.dismissNotice(notice.id), LIFETIME_MS);
    return () => window.clearTimeout(timer);
  }, [paused, inFront, notice.id]);

  return (
    <div
      className="pointer-events-auto relative flex gap-3 rounded-xl border border-line bg-panel p-3 pr-8 shadow-[var(--shadow)]"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
    >
      <span className="grid h-8 w-8 shrink-0 place-items-center rounded-full text-[10px] font-semibold text-white" style={{ background: color }} aria-hidden>
        {account ? initials(account.name, account.email).slice(0, 3) : "N"}
      </span>
      <button
        type="button"
        className={`min-w-0 flex-1 text-left ${canOpen ? "cursor-pointer" : "cursor-default"}`}
        onClick={() => (canOpen ? mail.openNotice(notice) : mail.dismissNotice(notice.id))}
      >
        <p className="truncate text-sm font-semibold">{heading}</p>
        {sub && <p className="truncate text-xs text-muted">{sub}</p>}
        <p className={`line-clamp-2 text-sm ${notice.kind === "error" ? "text-danger" : "text-muted"}`}>{notice.body}</p>
      </button>
      <button type="button" className="absolute right-2 top-2 grid h-6 w-6 place-items-center rounded-full text-muted hover:bg-hover" aria-label="Dismiss notification" onClick={() => mail.dismissNotice(notice.id)}>
        ×
      </button>
    </div>
  );
}
