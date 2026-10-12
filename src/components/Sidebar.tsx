import type { ReactNode } from "react";
import { IconBin, IconClock, IconCompose, IconDraft, IconInbox, IconLabel, IconScheduled, IconSend, IconSpam, IconStarOutline } from "./Icons";
import { useMail } from "../state/MailProvider";
import type { Folder } from "../types";

const FOLDERS: { id: Folder; label: string; icon: ReactNode }[] = [
  { id: "inbox", label: "Inbox", icon: <IconInbox /> },
  { id: "starred", label: "Starred", icon: <IconStarOutline /> },
  { id: "sent", label: "Sent", icon: <IconSend /> },
  { id: "drafts", label: "Drafts", icon: <IconDraft /> },
  { id: "snoozed", label: "Snoozed", icon: <IconClock /> },
  { id: "scheduled", label: "Scheduled", icon: <IconScheduled /> },
  { id: "spam", label: "Spam", icon: <IconSpam /> },
  { id: "trash", label: "Trash", icon: <IconBin /> },
];

/** Full folder list, or (when the menu button collapses it) a narrow column of icon buttons. */
export function Sidebar({ collapsed = false }: { collapsed?: boolean }) {
  const mail = useMail();
  const userLabels = mail.labels.filter((label) => label.kind === "user");
  const countFor = (id: Folder) =>
    id === "inbox" ? mail.counts.inboxUnread
    : id === "drafts" ? mail.counts.drafts
    : id === "starred" ? mail.counts.starred
    : id === "snoozed" ? mail.counts.snoozed
    : id === "scheduled" ? mail.counts.scheduled
    : id === "spam" ? mail.counts.spamUnread
    : 0;
  return (
    <nav
      className={`flex shrink-0 flex-col gap-1 overflow-y-auto overflow-x-hidden border-r border-line bg-side py-3 ${collapsed ? "w-16 items-center px-2" : "w-58 px-3"}`}
      aria-label="Folders"
    >
      {collapsed ? (
        <button type="button" className="mb-3 grid h-11 w-11 place-items-center rounded-2xl bg-accent text-accent-ink" title="Compose" aria-label="Compose" onClick={() => mail.composeNew()}>
          <IconCompose />
        </button>
      ) : (
        <button type="button" className="mb-3 inline-flex items-center gap-2 rounded-2xl bg-accent px-4 py-3 font-semibold text-accent-ink" onClick={() => mail.composeNew()}>
          <IconCompose /> Compose
        </button>
      )}
      {FOLDERS.map((folder) => (
        <NavItem
          key={folder.id}
          collapsed={collapsed}
          icon={folder.icon}
          label={folder.label}
          count={countFor(folder.id)}
          active={mail.query.folder === folder.id}
          onClick={() => mail.setFolder(folder.id)}
        />
      ))}
      {userLabels.length > 0 && (collapsed ? <div className="my-2 h-px w-8 bg-line" /> : <div className="mt-4 px-3 text-xs font-semibold uppercase tracking-wide text-muted">Labels</div>)}
      {userLabels.map((label) => {
        const account = mail.accounts.find((item) => item.id === label.accountId);
        return (
          <NavItem
            key={`${label.accountId}-${label.id}`}
            collapsed={collapsed}
            icon={<span style={{ color: label.color || account?.color || "#c9842a" }}><IconLabel /></span>}
            label={label.name}
            count={label.unread}
            active={mail.query.folder === "label" && mail.query.labelId === label.id}
            onClick={() => mail.setFolder("label", label.id)}
          />
        );
      })}
    </nav>
  );
}

function NavItem({ collapsed, icon, label, count, active, onClick }: { collapsed: boolean; icon: ReactNode; label: string; count: number; active: boolean; onClick: () => void }) {
  const tone = active ? "bg-selected font-semibold text-ink" : "text-muted hover:bg-hover hover:text-ink";
  if (collapsed) {
    return (
      <button
        type="button"
        title={count > 0 ? `${label} (${count})` : label}
        aria-label={count > 0 ? `${label}, ${count}` : label}
        aria-current={active ? "page" : undefined}
        className={`relative grid h-10 w-10 shrink-0 place-items-center rounded-xl ${tone}`}
        onClick={onClick}
      >
        {icon}
        {count > 0 && (
          <span className="absolute -right-0.5 -top-0.5 grid h-4 min-w-4 place-items-center rounded-full bg-accent px-1 text-[9px] font-bold leading-none text-accent-ink">
            {count > 99 ? "99+" : count}
          </span>
        )}
      </button>
    );
  }
  return (
    <button type="button" aria-current={active ? "page" : undefined} className={`flex items-center gap-3 rounded-xl px-3 py-2 text-left ${tone}`} onClick={onClick}>
      <span className="shrink-0">{icon}</span>
      <span className={`min-w-0 flex-1 truncate ${active ? "" : "text-ink"}`}>{label}</span>
      {count > 0 && <span className="text-xs">{count}</span>}
    </button>
  );
}
