import { formatListDate, personName, threadKey } from "../format";
import { IconArchive, IconBack, IconNext, IconPaperclip, IconRefresh, IconStar, IconTrash } from "./Icons";
import { WhenMenu } from "./WhenMenu";
import { useMail } from "../state/MailProvider";
import { defaultInboxTabs, type Category, type InboxTabs } from "../types";
import { useCallback, useEffect, useRef, useState } from "react";
import { useDismiss } from "../useDismiss";

const TABS: { id: Category; label: string }[] = [
  { id: "primary", label: "Primary" },
  { id: "promotions", label: "Promotions" },
  { id: "social", label: "Social" },
  { id: "updates", label: "Updates" },
];

export function MessageList({ className = "" }: { className?: string }) {
  const mail = useMail();
  // One toolbar pop-up at a time; a click outside the toolbar closes it.
  const [menu, setMenu] = useState<"label" | "snooze" | null>(null);
  const toolbar = useRef<HTMLDivElement>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  useDismiss(toolbar, menu !== null, closeMenu);
  // Actions apply to the checked rows, or to the conversation open in the reading pane.
  const hasTargets = mail.checked.length > 0 || mail.thread !== null;
  useEffect(() => {
    if (!hasTargets) setMenu(null);
  }, [hasTargets]);
  useEffect(() => {
    document.querySelector(`[data-row="${mail.cursor}"]`)?.scrollIntoView({ block: "nearest" });
  }, [mail.cursor]);
  const inboxTabs = mail.settings.inboxTabs ?? defaultInboxTabs;
  const tabs = TABS.filter((tab) => tab.id === "primary" || inboxTabs[tab.id as keyof InboxTabs]);
  useEffect(() => {
    if (mail.query.folder === "inbox" && mail.query.category !== "primary" && !inboxTabs[mail.query.category]) {
      mail.setCategory("primary");
    }
  }, [mail.query.folder, mail.query.category, inboxTabs, mail.setCategory]);
  const counts: Record<Category, number> = {
    primary: mail.counts.primaryUnread,
    promotions: mail.counts.promotionsUnread,
    social: mail.counts.socialUnread,
    updates: mail.counts.updatesUnread,
  };
  const accountIds = new Set(mail.checked.map((key) => key.split("\t")[0]));
  const labelAccount = accountIds.size === 1 ? [...accountIds][0] : mail.query.accountId;
  const labelChoices = mail.labels.filter((label) => label.kind === "user" && (!labelAccount || label.accountId === labelAccount));
  const pageStart = mail.threads.length > 0 ? mail.query.offset + 1 : 0;
  const pageEnd = mail.query.offset + mail.threads.length;
  return (
    <section className={`flex min-w-0 flex-1 flex-col bg-panel ${className}`}>
      {mail.query.folder === "inbox" && !mail.query.search && (
        <div className="flex gap-1 border-b border-line px-3 py-2">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={`rounded-full px-3 py-1.5 text-sm ${mail.query.category === tab.id ? "bg-selected font-semibold" : "text-muted hover:bg-hover"}`}
              onClick={() => mail.setCategory(tab.id)}
            >
              {tab.label}
              {counts[tab.id] > 0 && <span className="ml-2 text-xs">{counts[tab.id]}</span>}
            </button>
          ))}
        </div>
      )}
      <div ref={toolbar} className="flex items-center gap-1 border-b border-line px-2 py-1">
        <input
          type="checkbox"
          className="mx-2"
          aria-label="Select all"
          checked={mail.threads.length > 0 && mail.checked.length === mail.threads.length}
          onChange={mail.toggleAllChecks}
        />
        <button type="button" className="icon-btn" aria-label="Refresh" onClick={() => void mail.refresh()}><IconRefresh /></button>
        <button type="button" className="icon-btn" aria-label="Archive" title="Archive" disabled={!hasTargets} onClick={() => void mail.act("archive")}><IconArchive /></button>
        <button type="button" className="icon-btn" aria-label="Delete" title="Delete" disabled={!hasTargets} onClick={() => void mail.act("trash")}><IconTrash /></button>
        <button type="button" className="tool-btn" aria-label="Mark read" disabled={!hasTargets} onClick={() => void mail.act("read")}>Read</button>
        <button type="button" className="tool-btn" aria-label="Report spam" disabled={!hasTargets} onClick={() => void mail.act("spam")}>Spam</button>
        {mail.query.folder === "snoozed" ? (
          <button type="button" className="tool-btn" disabled={!hasTargets} onClick={() => void mail.unsnooze()}>Unsnooze</button>
        ) : mail.query.folder !== "scheduled" && (
          <div className="relative">
            <button type="button" className={`tool-btn ${menu === "snooze" ? "bg-hover text-ink" : ""}`} aria-label="Snooze" aria-expanded={menu === "snooze"} disabled={!hasTargets} onClick={() => setMenu((open) => (open === "snooze" ? null : "snooze"))}>Snooze</button>
            {menu === "snooze" && <WhenMenu title="Snooze until" onPick={(at) => void mail.snooze(at)} onClose={closeMenu} />}
          </div>
        )}
        <div className="relative">
          <button type="button" className={`tool-btn ${menu === "label" ? "bg-hover text-ink" : ""}`} aria-label="Label" aria-expanded={menu === "label"} disabled={!hasTargets} onClick={() => setMenu((open) => (open === "label" ? null : "label"))}>Label</button>
          {menu === "label" && (
            <div className="absolute left-0 top-9 z-20 w-56 rounded-xl border border-line bg-panel p-2 shadow-[var(--shadow)]">
              {labelChoices.length === 0 && <p className="px-2 py-1 text-sm text-muted">No labels for this selection.</p>}
              {labelChoices.map((label) => (
                <button
                  key={`${label.accountId}-${label.id}`}
                  type="button"
                  className="block w-full rounded-lg px-2 py-1.5 text-left text-sm hover:bg-hover"
                  onClick={() => {
                    setMenu(null);
                    void mail.act("add-label", label.id);
                  }}
                >
                  {label.name}
                </button>
              ))}
            </div>
          )}
        </div>
        {mail.remoteNote && <span className="ml-auto pr-2 text-xs text-muted">{mail.remoteNote}</span>}
        {mail.total > 0 && (
          <div className={`flex items-center gap-1 ${mail.remoteNote ? "" : "ml-auto"}`}>
            <span className="px-2 text-xs text-muted">
              {pageStart.toLocaleString()}–{pageEnd.toLocaleString()} of {mail.total.toLocaleString()}
            </span>
            <button type="button" className="icon-btn" aria-label="Newer" title="Newer" disabled={mail.query.offset === 0} onClick={() => mail.turnPage(-1)}><IconBack /></button>
            <button type="button" className="icon-btn" aria-label="Older" title="Older" disabled={!mail.hasMore} onClick={() => mail.turnPage(1)}><IconNext /></button>
          </div>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {mail.threads.length === 0 && (
          <div className="grid h-full place-items-center px-6 text-center text-muted">
            <div>
              <p className="text-lg text-ink">Nothing in here</p>
              <p className="mt-1 text-sm">{mail.query.search ? "No mail matched that search." : "This folder is empty."}</p>
            </div>
          </div>
        )}
        {mail.threads.map((thread, index) => {
          const key = threadKey(thread.accountId, thread.threadId);
          const active = (mail.thread?.accountId === thread.accountId && mail.thread.threadId === thread.threadId);
          return (
            <div
              key={key}
              data-row={index}
              role="button"
              tabIndex={0}
              aria-label={thread.subject || "(no subject)"}
              className={`mail-row grid cursor-pointer grid-cols-[28px_28px_minmax(120px,180px)_1fr_auto] items-center gap-2 border-b border-line px-2 ${active ? "bg-selected" : "hover:bg-hover"} ${thread.unread ? "font-semibold" : ""}`}
              style={{ boxShadow: mail.query.accountId ? undefined : `inset 3px 0 0 ${thread.accountColor}` }}
              onClick={() => void mail.openThread(thread)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  void mail.openThread(thread);
                }
              }}
            >
              <input
                type="checkbox"
                aria-label={`Select ${thread.subject}`}
                checked={mail.checked.includes(key)}
                onClick={(event) => event.stopPropagation()}
                onChange={() => mail.toggleCheck(key)}
              />
              <button
                type="button"
                className="icon-btn"
                aria-label={thread.starred ? "Unstar" : "Star"}
                onClick={(event) => {
                  event.stopPropagation();
                  void mail.act(thread.starred ? "unstar" : "star", undefined, [{ accountId: thread.accountId, threadId: thread.threadId }]);
                }}
              >
                <IconStar filled={thread.starred} />
              </button>
              <div className="min-w-0 leading-tight">
                <div className="truncate">{personName(thread.fromName, thread.fromEmail)}{thread.messageCount > 1 ? ` (${thread.messageCount})` : ""}</div>
                {thread.fromName.trim() && thread.fromEmail && <div className="truncate text-xs font-normal text-muted">{thread.fromEmail}</div>}
              </div>
              <div className="min-w-0 truncate">
                <span>{thread.subject || "(no subject)"}</span>
                <span className="font-normal text-muted"> — {thread.snippet}</span>
                {thread.hasAttachments && <span className="ml-1 inline-block align-middle text-muted"><IconPaperclip /></span>}
              </div>
              <div className="flex items-center gap-2 pr-2 text-xs text-muted">
                {!mail.query.accountId && (
                  <span className="max-w-45 truncate rounded-full px-2 py-0.5 font-medium text-white" style={{ background: thread.accountColor }} title={thread.accountEmail}>
                    {thread.accountEmail}
                  </span>
                )}
                {thread.snoozeUntil ? `Until ${formatListDate(thread.snoozeUntil)}` : thread.scheduledId ? `Sends ${formatListDate(thread.date)}` : formatListDate(thread.date)}
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
