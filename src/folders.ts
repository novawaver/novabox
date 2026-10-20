import type {
  Account,
  ComposeDraft,
  FolderCounts,
  InboxTabs,
  Label,
  Mailbox,
  MailQuery,
  MessageDetail,
  ThreadDetail,
  ThreadSummary,
} from "./types";
import { defaultInboxTabs } from "./types";

export type SnoozeRow = { accountId: string; threadId: string; untilAt: number };
export type ScheduledRow = { id: string; sendAt: number; draft: ComposeDraft };
import { emptyCounts } from "./types";

export function hasLabel(labels: string[], id: string): boolean {
  return labels.includes(id);
}

function inInbox(labels: string[]): boolean {
  return hasLabel(labels, "INBOX") && !hasLabel(labels, "TRASH") && !hasLabel(labels, "SPAM");
}

/** Keep this in step with message_in_folder in src-tauri/src/db.rs. */
export function messageInFolder(
  labels: string[],
  folder: string,
  labelId: string | null,
  category: string | null,
  tabs: InboxTabs = defaultInboxTabs,
): boolean {
  const trash = hasLabel(labels, "TRASH");
  const spam = hasLabel(labels, "SPAM");
  switch (folder) {
    case "all":
      return true;
    case "inbox": {
      if (!inInbox(labels)) return false;
      const promotions = tabs.promotions && hasLabel(labels, "CATEGORY_PROMOTIONS");
      const social = tabs.social && (hasLabel(labels, "CATEGORY_SOCIAL") || hasLabel(labels, "CATEGORY_FORUMS"));
      const updates = tabs.updates && hasLabel(labels, "CATEGORY_UPDATES");
      switch (category ?? "primary") {
        case "all":
          return true;
        case "promotions":
          return promotions;
        case "social":
          return social;
        case "updates":
          return updates;
        default:
          return !promotions && !social && !updates;
      }
    }
    case "starred":
      return hasLabel(labels, "STARRED") && !trash;
    case "sent":
      return hasLabel(labels, "SENT") && !trash;
    case "drafts":
      return hasLabel(labels, "DRAFT") && !trash;
    case "spam":
      return spam;
    case "trash":
      return trash;
    case "label":
      return Boolean(labelId && hasLabel(labels, labelId) && !trash);
    default:
      return false;
  }
}

function group(messages: MessageDetail[]): Map<string, MessageDetail[]> {
  const map = new Map<string, MessageDetail[]>();
  for (const message of messages) {
    const key = `${message.accountId}\t${message.threadId}`;
    const list = map.get(key) ?? [];
    list.push(message);
    map.set(key, list);
  }
  return map;
}

function summarize(
  messages: MessageDetail[],
  accounts: Account[],
  folder: string,
  labelId: string | null,
  category: string | null,
  tabs: InboxTabs,
): ThreadSummary[] {
  const byId = new Map(accounts.map((account) => [account.id, account]));
  const threads: ThreadSummary[] = [];
  for (const list of group(messages).values()) {
    const matching = list.filter((message) => messageInFolder(message.labelIds, folder, labelId, category, tabs));
    if (matching.length === 0) continue;
    const latest = matching.reduce((best, message) => {
      if (message.date > best.date || (message.date === best.date && message.id > best.id)) return message;
      return best;
    });
    const account = byId.get(latest.accountId);
    threads.push({
      accountId: latest.accountId,
      accountEmail: account?.email ?? "",
      accountColor: account?.color ?? "#888888",
      threadId: latest.threadId,
      latestMessageId: latest.id,
      fromName: latest.fromName,
      fromEmail: latest.fromEmail,
      subject: latest.subject,
      snippet: latest.snippet,
      date: latest.date,
      unread: matching.some((message) => hasLabel(message.labelIds, "UNREAD")),
      starred: matching.some((message) => hasLabel(message.labelIds, "STARRED")),
      hasAttachments: matching.some((message) => message.attachments.length > 0),
      messageCount: matching.length,
      labelIds: latest.labelIds,
    });
  }
  threads.sort((a, b) => b.date - a.date || b.threadId.localeCompare(a.threadId));
  return threads;
}

function countsFrom(messages: MessageDetail[], tabs: InboxTabs): FolderCounts {
  const groups = [...group(messages).values()];
  const inbox = groups.filter((list) => list.some((message) => inInbox(message.labelIds)));
  const unreadIn = (category: string | null) =>
    groups.filter((list) =>
      list.some(
        (message) =>
          hasLabel(message.labelIds, "UNREAD") &&
          messageInFolder(message.labelIds, "inbox", null, category, tabs),
      ),
    ).length;
  return {
    inbox: inbox.length,
    inboxUnread: groups.filter((list) =>
      list.some((message) => hasLabel(message.labelIds, "UNREAD") && inInbox(message.labelIds)),
    ).length,
    starred: groups.filter((list) => list.some((message) => messageInFolder(message.labelIds, "starred", null, null, tabs))).length,
    drafts: groups.filter((list) => list.some((message) => messageInFolder(message.labelIds, "drafts", null, null, tabs))).length,
    primaryUnread: unreadIn("primary"),
    promotionsUnread: unreadIn("promotions"),
    socialUnread: unreadIn("social"),
    updatesUnread: unreadIn("updates"),
    spamUnread: groups.filter((list) =>
      list.some((message) => hasLabel(message.labelIds, "UNREAD") && messageInFolder(message.labelIds, "spam", null, null, tabs)),
    ).length,
    snoozed: 0,
    scheduled: 0,
  };
}

function accountUnread(messages: MessageDetail[], accountId: string): number {
  const mine = messages.filter((message) => message.accountId === accountId);
  return [...group(mine).values()].filter((list) =>
    list.some((message) => hasLabel(message.labelIds, "UNREAD") && inInbox(message.labelIds)),
  ).length;
}

export function buildMailbox(
  messages: MessageDetail[],
  accounts: Account[],
  labels: Label[],
  query: MailQuery,
  snoozes: SnoozeRow[] = [],
  scheduled: ScheduledRow[] = [],
  tabs: InboxTabs = defaultInboxTabs,
): Mailbox {
  const scoped = query.accountId ? messages.filter((message) => message.accountId === query.accountId) : messages;
  const snoozeKeys = new Set(snoozes.filter((item) => !query.accountId || item.accountId === query.accountId).map((item) => `${item.accountId}\t${item.threadId}`));
  const waiting = scheduled.filter((item) => !query.accountId || item.draft.accountId === query.accountId);
  const scopedLabels = query.accountId ? labels.filter((label) => label.accountId === query.accountId) : labels;
  const nextAccounts = accounts.map((account) => ({
    ...account,
    unread: accountUnread(messages, account.id),
  }));
  const nextLabels = scopedLabels.map((label) => {
    if (label.kind !== "user") return label;
    const unread = [...group(scoped).values()].filter((list) =>
      list.some(
        (message) =>
          message.accountId === label.accountId &&
          hasLabel(message.labelIds, label.id) &&
          hasLabel(message.labelIds, "UNREAD") &&
          !hasLabel(message.labelIds, "TRASH"),
      ),
    ).length;
    return { ...label, unread };
  });
  const search = query.search.trim().toLowerCase();
  const listMessages = search
    ? scoped.filter((message) =>
        [message.subject, message.snippet, message.bodyText, message.fromEmail, message.fromName]
          .join("\n")
          .toLowerCase()
          .includes(search),
      )
    : scoped;
  const threads = query.folder === "scheduled" && !search
    ? waiting.map((item) => scheduledSummary(item, nextAccounts))
    : query.folder === "snoozed" && !search
      ? summarize(
          scoped.filter((message) => snoozeKeys.has(`${message.accountId}\t${message.threadId}`) && !message.labelIds.includes("TRASH") && !message.labelIds.includes("SPAM")),
          nextAccounts,
          "all",
          null,
          null,
          tabs,
        ).map((thread) => ({
          ...thread,
          snoozeUntil: snoozes.find((item) => item.accountId === thread.accountId && item.threadId === thread.threadId)?.untilAt ?? null,
        }))
      : summarize(
          listMessages,
          nextAccounts,
          search ? "all" : query.folder,
          search ? null : query.labelId,
          search || query.folder !== "inbox" ? null : query.category,
          tabs,
        ).filter((thread) => search || query.folder !== "inbox" || !snoozeKeys.has(`${thread.accountId}\t${thread.threadId}`));
  const offset = Math.max(0, query.offset);
  const limit = Math.min(200, Math.max(1, query.limit));
  return {
    accounts: nextAccounts,
    labels: nextLabels,
    threads: threads.slice(offset, offset + limit),
    hasMore: threads.length > offset + limit,
    total: threads.length,
    counts: {
      ...(scoped.length ? countsFrom(scoped, tabs) : emptyCounts),
      snoozed: new Set(scoped.filter((message) => snoozeKeys.has(`${message.accountId}\t${message.threadId}`)).map((message) => `${message.accountId}\t${message.threadId}`)).size,
      scheduled: waiting.length,
    },
  };
}

function scheduledSummary(item: ScheduledRow, accounts: Account[]): ThreadSummary {
  const account = accounts.find((candidate) => candidate.id === item.draft.accountId);
  const snippet = item.draft.html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim().slice(0, 160);
  return {
    accountId: item.draft.accountId,
    accountEmail: account?.email ?? item.draft.fromEmail,
    accountColor: account?.color ?? "#888888",
    threadId: `sched:${item.id}`,
    latestMessageId: item.id,
    fromName: item.draft.fromEmail,
    fromEmail: item.draft.fromEmail,
    subject: item.draft.subject,
    snippet,
    date: item.sendAt,
    unread: false,
    starred: false,
    hasAttachments: item.draft.attachments.length > 0,
    messageCount: 1,
    labelIds: [],
    scheduledId: item.id,
    bodyHtml: item.draft.html,
    previewTo: item.draft.to,
  };
}

export function threadFrom(
  messages: MessageDetail[],
  accounts: Account[],
  accountId: string,
  threadId: string,
): ThreadDetail | null {
  const account = accounts.find((item) => item.id === accountId);
  const list = messages
    .filter((message) => message.accountId === accountId && message.threadId === threadId)
    .sort((a, b) => a.date - b.date || a.id.localeCompare(b.id));
  if (!account || list.length === 0) return null;
  return {
    accountId,
    accountEmail: account.email,
    accountColor: account.color,
    threadId,
    subject: list.find((message) => message.subject)?.subject ?? "",
    messages: list,
  };
}

export function applyThreadAction(
  messages: MessageDetail[],
  accountId: string,
  threadId: string,
  action: string,
  labelId?: string,
): void {
  const add: string[] = [];
  const remove: string[] = [];
  switch (action) {
    case "archive":
      remove.push("INBOX");
      break;
    case "unarchive":
      add.push("INBOX");
      break;
    case "trash":
      add.push("TRASH");
      remove.push("INBOX");
      break;
    case "untrash":
      add.push("INBOX");
      remove.push("TRASH");
      break;
    case "read":
      remove.push("UNREAD");
      break;
    case "unread":
      add.push("UNREAD");
      break;
    case "star":
      add.push("STARRED");
      break;
    case "unstar":
      remove.push("STARRED");
      break;
    case "spam":
      add.push("SPAM");
      remove.push("INBOX");
      break;
    case "unspam":
      add.push("INBOX");
      remove.push("SPAM");
      break;
    case "add-label":
      if (labelId) add.push(labelId);
      break;
    case "remove-label":
      if (labelId) remove.push(labelId);
      break;
    default:
      return;
  }
  for (const message of messages) {
    if (message.accountId !== accountId || message.threadId !== threadId) continue;
    for (const id of add) {
      if (!message.labelIds.includes(id)) message.labelIds.push(id);
    }
    message.labelIds = message.labelIds.filter((id) => !remove.includes(id));
    message.unread = message.labelIds.includes("UNREAD");
    message.starred = message.labelIds.includes("STARRED");
  }
}
