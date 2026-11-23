import { formatListDate, personName, threadKey } from "../format";
import { IconArchive, IconBack, IconClock, IconLabel, IconNext, IconPaperclip, IconRead, IconRefresh, IconRestore, IconSpam, IconStar, IconTrash, IconTrashForever, IconUnread, IconUnsnooze } from "./Icons";
import { LocalLabelDialog } from "./LocalLabelDialog";
import { WhenMenu } from "./WhenMenu";
import { useMail } from "../state/MailProvider";
import { defaultInboxTabs, isGoogle, LOCAL_LABEL_COLORS, type Category, type InboxTabs } from "../types";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useDismiss } from "../useDismiss";

const TABS: { id: Category; label: string }[] = [
  { id: "all", label: "All" },
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
  const tabs = TABS.filter((tab) => tab.id === "all" || tab.id === "primary" || inboxTabs[tab.id as keyof InboxTabs]);
  useEffect(() => {
    const category = mail.query.category;
    if (mail.query.folder === "inbox" && category !== "all" && category !== "primary" && !inboxTabs[category]) {
      mail.setCategory("primary");
    }
  }, [mail.query.folder, mail.query.category, inboxTabs, mail.setCategory]);
  const counts: Record<Category, number> = {
    all: mail.counts.inboxUnread,
    primary: mail.counts.primaryUnread,
    promotions: mail.counts.promotionsUnread,
    social: mail.counts.socialUnread,
    updates: mail.counts.updatesUnread,
  };
  const accountIds = new Set(mail.checked.map((key) => key.split("\t")[0]));
  const labelAccount = accountIds.size === 1 ? [...accountIds][0] : mail.query.accountId;
  const labelChoices = mail.labels.filter((label) => label.kind === "user" && (!labelAccount || label.accountId === labelAccount));
  const inTrash = mail.query.folder === "trash";
  const [newLabel, setNewLabel] = useState(false);
  const localLabels = mail.mode === "live" ? mail.localLabels : [];
  // The rows the toolbar acts on, to tell which Novabox labels they all carry already.
  const targetRows = mail.threads.filter((item) =>
    mail.checked.length > 0
      ? mail.checked.includes(threadKey(item.accountId, item.threadId))
      : mail.thread?.accountId === item.accountId && mail.thread.threadId === item.threadId,
  );
  const carried = (labelId: string) => targetRows.length > 0 && targetRows.every((item) => item.localLabelIds?.includes(labelId));
  // Only Gmail empties Trash on a schedule Novabox knows; other services set their own.
  const trashExpires = mail.accounts.some((account) => isGoogle(account) && (!mail.query.accountId || account.id === mail.query.accountId));
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
        {inTrash ? (
          <>
            <button type="button" className="icon-btn" aria-label="Restore" title="Restore to the inbox" disabled={!hasTargets} onClick={() => void mail.act("untrash")}><IconRestore /></button>
            <button type="button" className="icon-btn hover:text-danger" aria-label="Delete forever" title="Delete forever. This can't be undone." disabled={!hasTargets} onClick={() => mail.deleteForever()}><IconTrashForever /></button>
          </>
        ) : (
          <button type="button" className="icon-btn" aria-label="Delete" title="Move to Trash" disabled={!hasTargets} onClick={() => void mail.act("trash")}><IconTrash /></button>
        )}
        <button type="button" className="icon-btn" aria-label="Mark as read" title="Mark as read" disabled={!hasTargets} onClick={() => void mail.act("read")}><IconRead /></button>
        <button type="button" className="icon-btn" aria-label="Report spam" title="Report spam" disabled={!hasTargets} onClick={() => void mail.act("spam")}><IconSpam /></button>
        {mail.query.folder === "snoozed" ? (
          <button type="button" className="icon-btn" aria-label="Unsnooze" title="Unsnooze" disabled={!hasTargets} onClick={() => void mail.unsnooze()}><IconUnsnooze /></button>
        ) : mail.query.folder !== "scheduled" && (
          <div className="relative">
            <button type="button" className={`icon-btn ${menu === "snooze" ? "bg-hover text-ink" : ""}`} aria-label="Snooze" title="Snooze" aria-expanded={menu === "snooze"} disabled={!hasTargets} onClick={() => setMenu((open) => (open === "snooze" ? null : "snooze"))}><IconClock /></button>
            {menu === "snooze" && <WhenMenu title="Snooze until" onPick={(at) => void mail.snooze(at)} onClose={closeMenu} />}
          </div>
        )}
        <div className="relative">
          <button type="button" className={`icon-btn ${menu === "label" ? "bg-hover text-ink" : ""}`} aria-label="Label" title="Label" aria-expanded={menu === "label"} disabled={!hasTargets} onClick={() => setMenu((open) => (open === "label" ? null : "label"))}><IconLabel /></button>
          {menu === "label" && (
            <div className="absolute left-0 top-9 z-20 max-h-[70vh] w-60 overflow-y-auto rounded-xl border border-line bg-panel p-2 shadow-[var(--shadow)]">
              {mail.mode === "live" && (
                <>
                  <p className="px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-muted">Novabox labels</p>
                  {localLabels.map((label) => {
                    const on = carried(label.id);
                    return (
                      <button
                        key={label.id}
                        type="button"
                        role="menuitemcheckbox"
                        aria-checked={on}
                        className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm hover:bg-hover"
                        onClick={() => {
                          setMenu(null);
                          void mail.tagThreads(label.id, !on);
                        }}
                      >
                        <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: label.color || LOCAL_LABEL_COLORS[0] }} aria-hidden />
                        <span className="min-w-0 flex-1 truncate">{label.name}</span>
                        {on && <span aria-hidden>✓</span>}
                      </button>
                    );
                  })}
                  <button
                    type="button"
                    className="block w-full rounded-lg px-2 py-1.5 text-left text-sm text-accent hover:bg-hover"
                    onClick={() => {
                      setMenu(null);
                      setNewLabel(true);
                    }}
                  >
                    New Novabox label…
                  </button>
                  <div className="my-2 h-px bg-line" />
                  <p className="px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-muted">Account labels</p>
                </>
              )}
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
      {inTrash && (
        <div className="flex items-center gap-3 border-b border-line px-4 py-1.5 text-xs text-muted">
          <span>{trashExpires ? "Gmail deletes conversations in Trash for good after 30 days." : "Mail in Trash stays until you delete it for good."}</span>
          <button type="button" className="rounded-full px-2 py-1 font-semibold text-danger hover:bg-hover disabled:opacity-40" disabled={mail.total === 0} onClick={mail.emptyTrash}>
            Empty Trash now
          </button>
        </div>
      )}
      {newLabel && <LocalLabelDialog onClose={() => setNewLabel(false)} />}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {mail.listLoading && (
          <div className="grid h-full place-items-center text-muted" role="status" aria-live="polite">
            <div className="flex flex-col items-center gap-3">
              <span className="h-8 w-8 animate-spin rounded-full border-2 border-line border-t-accent" aria-hidden />
              <span className="text-sm">Loading…</span>
            </div>
          </div>
        )}
        {!mail.listLoading && mail.threads.length === 0 && (
          <div className="grid h-full place-items-center px-6 text-center text-muted">
            <div>
              <p className="text-lg text-ink">Nothing in here</p>
              <p className="mt-1 text-sm">{mail.query.search ? "No mail matched that search." : "This folder is empty."}</p>
            </div>
          </div>
        )}
        {!mail.listLoading && mail.threads.map((thread, index) => {
          const key = threadKey(thread.accountId, thread.threadId);
          const active = (mail.thread?.accountId === thread.accountId && mail.thread.threadId === thread.threadId);
          // Buttons that act on this row alone, whatever else is ticked. Shown over the date
          // while the pointer is on the row, or while one of them has keyboard focus.
          const rowAction = (label: string, action: string, icon: ReactNode) => (
            <button
              type="button"
              className="icon-btn hover:bg-line"
              title={label}
              aria-label={label}
              onClick={(event) => {
                event.stopPropagation();
                void mail.act(action, undefined, [{ accountId: thread.accountId, threadId: thread.threadId }]);
              }}
            >
              {icon}
            </button>
          );
          return (
            <div
              key={key}
              data-row={index}
              role="button"
              tabIndex={0}
              aria-label={thread.subject || "(no subject)"}
              className={`mail-row group grid cursor-pointer grid-cols-[28px_28px_minmax(120px,180px)_1fr_auto] items-center gap-2 border-b border-line px-2 ${active ? "bg-selected" : thread.unread ? "hover:bg-hover" : "bg-read hover:bg-hover"} ${thread.unread ? "font-semibold" : ""}`}
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
                {thread.fromName.trim() && thread.fromEmail && <div className="truncate text-xs font-normal text-muted opacity-70">{thread.fromEmail}</div>}
              </div>
              <div className="min-w-0 truncate">
                {localLabels
                  .filter((label) => thread.localLabelIds?.includes(label.id))
                  .map((label) => (
                    <span key={label.id} className="mr-1.5 inline-block max-w-32 truncate rounded px-1.5 align-middle text-xs font-medium leading-5 text-white" style={{ background: label.color || LOCAL_LABEL_COLORS[0] }}>
                      {label.name}
                    </span>
                  ))}
                {thread.remoteScheduled && <span className="mr-1.5 inline-block rounded border border-line px-1.5 align-middle text-xs font-medium leading-5 text-muted">Scheduled in Gmail</span>}
                <span>{thread.subject || "(no subject)"}</span>
                <span className="font-normal text-muted"> — {thread.snippet}</span>
                {thread.hasAttachments && <span className="ml-1 inline-block align-middle text-muted"><IconPaperclip /></span>}
              </div>
              <div className="relative flex h-full items-center gap-2 pr-2 text-xs text-muted">
                {!mail.query.accountId && (
                  <span className="max-w-45 truncate rounded-full px-2 py-0.5 font-medium text-white" style={{ background: thread.accountColor }} title={thread.accountEmail}>
                    {thread.accountEmail}
                  </span>
                )}
                {thread.snoozeUntil ? `Until ${formatListDate(thread.snoozeUntil)}` : thread.scheduledId || thread.remoteScheduled ? `Sends ${formatListDate(thread.date)}` : formatListDate(thread.date)}
                {!thread.scheduledId && !thread.remoteScheduled && (
                  <div
                    className={`absolute inset-y-0 right-0 hidden items-center gap-0.5 pl-3 pr-1 font-normal text-ink group-hover:flex group-focus-within:flex ${active ? "bg-selected" : "bg-hover"}`}
                  >
                    {!inTrash && mail.query.folder !== "spam" && rowAction("Archive", "archive", <IconArchive />)}
                    {inTrash ? rowAction("Restore to the inbox", "untrash", <IconRestore />) : rowAction("Move to Trash", "trash", <IconTrash />)}
                    {inTrash && (
                      <button
                        type="button"
                        className="icon-btn hover:bg-line hover:text-danger"
                        title="Delete forever"
                        aria-label="Delete forever"
                        onClick={(event) => {
                          event.stopPropagation();
                          mail.deleteForever([{ accountId: thread.accountId, threadId: thread.threadId }]);
                        }}
                      >
                        <IconTrashForever />
                      </button>
                    )}
                    {thread.unread ? rowAction("Mark as read", "read", <IconRead />) : rowAction("Mark as unread", "unread", <IconUnread />)}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
