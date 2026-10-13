import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { api, type ImportItem } from "../api";
import { createDemo, newId, type DemoDb } from "../demo";
import { escapeHtml, parseThreadKey, threadKey } from "../format";
import { applyThreadAction, buildMailbox, threadFrom } from "../folders";
import { applyAppearance, inTauri } from "../platform";
import { cleanFragment } from "../sanitize";
import {
  defaultInboxTabs,
  defaultSettings,
  emptyCounts,
  initialQuery,
  type Account,
  type Category,
  type ComposeDraft,
  type Folder,
  type FolderCounts,
  type Label,
  type Mailbox,
  type MailQuery,
  type MessageDetail,
  type Notice,
  type Settings,
  type Template,
  type ThreadDetail,
  type ThreadSummary,
  type ThreadTarget,
} from "../types";

function normalizeSettings(value: Partial<Settings> | null | undefined): Settings {
  return { ...defaultSettings, ...value, inboxTabs: { ...defaultInboxTabs, ...value?.inboxTabs } };
}

const emptyBox: Mailbox = {
  accounts: [],
  labels: [],
  threads: [],
  hasMore: false,
  total: 0,
  counts: emptyCounts,
};

const REVERSE: Record<string, string> = {
  archive: "unarchive",
  unarchive: "archive",
  trash: "untrash",
  untrash: "trash",
  read: "unread",
  unread: "read",
  star: "unstar",
  unstar: "star",
  spam: "unspam",
  unspam: "spam",
  "add-label": "remove-label",
  "remove-label": "add-label",
};

const ACTION_TEXT: Record<string, string> = {
  archive: "Archived",
  unarchive: "Moved to Inbox",
  trash: "Moved to Trash",
  untrash: "Restored",
  read: "Marked as read",
  unread: "Marked as unread",
  star: "Starred",
  unstar: "Star removed",
  spam: "Reported as spam",
  unspam: "Removed from Spam",
  "add-label": "Label added",
  "remove-label": "Label removed",
};

type Undo = { text: string; targets: ThreadTarget[]; reverse: string; labelId?: string };
type Toast = { id: string; text: string };
type Mode = "none" | "demo" | "live";

export type MailApi = {
  ready: boolean;
  mode: Mode;
  accounts: Account[];
  labels: Label[];
  threads: ThreadSummary[];
  hasMore: boolean;
  /** Conversations matching the query across all pages. */
  total: number;
  counts: FolderCounts;
  query: MailQuery;
  thread: ThreadDetail | null;
  threadLoading: boolean;
  checked: string[];
  cursor: number;
  settings: Settings;
  settingsOpen: boolean;
  sidebarOpen: boolean;
  composes: ComposeDraft[];
  toasts: Toast[];
  notices: Notice[];
  undo: Undo | null;
  syncing: boolean;
  syncLabel: string;
  offline: boolean;
  signInBusy: boolean;
  signInError: string;
  /** Whether this build can sign in to Google (a client ID was set in .env at build time). */
  googleReady: boolean;
  shownImages: string[];
  remoteNote: string;
  openDemo: () => void;
  leaveDemo: () => void;
  signIn: () => Promise<void>;
  cancelSignIn: () => void;
  removeAccount: (id: string, revoke?: boolean) => Promise<void>;
  importSignIns: (path: string, password: string) => Promise<ImportItem[]>;
  /** Call when the import dialog closes: switches to the imported accounts if any arrived. */
  finishImport: () => void;
  selectAccount: (id: string | null) => void;
  setFolder: (folder: Folder, labelId?: string | null) => void;
  setCategory: (category: Category) => void;
  setSearch: (search: string) => void;
  submitSearch: () => Promise<void>;
  /** Moves the list one page forward (1) or back (-1). */
  turnPage: (step: 1 | -1) => void;
  openThread: (thread: ThreadSummary) => Promise<void>;
  closeThread: () => void;
  toggleCheck: (key: string) => void;
  toggleAllChecks: () => void;
  setCursor: (index: number) => void;
  act: (action: string, labelId?: string, targets?: ThreadTarget[]) => Promise<void>;
  undoAct: () => Promise<void>;
  /** Deletes the trashed messages of the checked or open conversations for good. No undo. */
  deleteForever: (targets?: ThreadTarget[]) => Promise<void>;
  /** Deletes everything in Trash for good, for the selected account or all of them. No undo. */
  emptyTrash: () => Promise<void>;
  saveSettings: (settings: Settings) => Promise<void>;
  setSignature: (accountId: string, signature: string) => Promise<void>;
  setSettingsOpen: (open: boolean) => void;
  toggleSidebar: () => void;
  composeNew: (accountId?: string) => void;
  composeReply: (mode: "reply" | "replyAll" | "forward") => Promise<void>;
  closeCompose: (localId: string) => void;
  updateCompose: (localId: string, patch: Partial<ComposeDraft>) => void;
  sendCompose: (localId: string) => Promise<void>;
  saveDraftCompose: (localId: string) => Promise<void>;
  showImages: (messageId: string) => void;
  allowSender: (email: string) => void;
  refresh: () => Promise<void>;
  previewAttachment: (accountId: string, messageId: string, attachmentId: string) => Promise<{ filename: string; mimeType: string; dataBase64: string }>;
  saveAttachment: (accountId: string, messageId: string, attachmentId: string, filename: string) => Promise<void>;
  dismissToast: (id: string) => void;
  dismissNotice: (id: string) => void;
  testNotify: (prefs: Settings) => void;
  openNotice: (notice: Notice) => void;
  templates: Template[];
  snooze: (until: number, targets?: ThreadTarget[]) => Promise<void>;
  unsnooze: (targets?: ThreadTarget[]) => Promise<void>;
  scheduleCompose: (localId: string, sendAt: number) => Promise<void>;
  cancelScheduled: (id: string) => Promise<void>;
  sendScheduled: (id: string) => Promise<void>;
  saveTemplate: (name: string, subject: string, html: string) => Promise<void>;
  deleteTemplate: (id: string) => Promise<void>;
};

const MailContext = createContext<MailApi | null>(null);

export function useMail(): MailApi {
  const value = useContext(MailContext);
  if (!value) throw new Error("useMail must be used inside MailProvider");
  return value;
}

export function MailProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [mode, setMode] = useState<Mode>("none");
  const [box, setBox] = useState<Mailbox>(emptyBox);
  const [query, setQuery] = useState<MailQuery>(initialQuery);
  const [thread, setThread] = useState<ThreadDetail | null>(null);
  const [threadLoading, setThreadLoading] = useState(false);
  const [checked, setChecked] = useState<string[]>([]);
  const [cursor, setCursor] = useState(0);
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [composes, setComposes] = useState<ComposeDraft[]>([]);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [notices, setNotices] = useState<Notice[]>([]);
  const [undo, setUndo] = useState<Undo | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncLabel, setSyncLabel] = useState("");
  const [offline, setOffline] = useState(false);
  const [signInBusy, setSignInBusy] = useState(false);
  const [signInError, setSignInError] = useState("");
  const [googleReady, setGoogleReady] = useState(true);
  const [shownImages, setShownImages] = useState<string[]>([]);
  const [remoteThreads, setRemoteThreads] = useState<ThreadSummary[] | null>(null);
  const [remoteNote, setRemoteNote] = useState("");
  const [templates, setTemplates] = useState<Template[]>([]);

  const demoRef = useRef<DemoDb | null>(null);
  const queryRef = useRef(query);
  const modeRef = useRef(mode);
  const openRef = useRef<ThreadTarget | null>(null);
  const checkedRef = useRef(checked);
  const cursorRef = useRef(cursor);
  const threadsRef = useRef<ThreadSummary[]>([]);
  const settingsRef = useRef(settings);
  const accountsRef = useRef<Account[]>([]);
  const composesRef = useRef(composes);
  const reloadRef = useRef<() => Promise<void>>(async () => {});
  const toastRef = useRef<(text: string) => void>(() => {});
  const deliverRef = useRef<(payload: Omit<Notice, "id">, prefs: Settings, test?: boolean) => void>(() => {});
  const openNoticeRef = useRef<(notice: Notice) => void>(() => {});

  queryRef.current = query;
  modeRef.current = mode;
  checkedRef.current = checked;
  cursorRef.current = cursor;
  settingsRef.current = settings;
  accountsRef.current = box.accounts;
  composesRef.current = composes;
  const visibleThreads = remoteThreads ?? box.threads;
  threadsRef.current = visibleThreads;

  function toast(text: string) {
    const id = newId("toast");
    setToasts((list) => [...list, { id, text }]);
    window.setTimeout(() => setToasts((list) => list.filter((item) => item.id !== id)), 4200);
  }
  toastRef.current = toast;

  function publishDemo() {
    const db = demoRef.current;
    if (!db) return;
    setBox(buildMailbox(db.messages, db.accounts, db.labels, queryRef.current, db.snoozes, db.scheduled, settingsRef.current.inboxTabs));
    setTemplates(db.templates);
    const open = openRef.current;
    setThread(open ? threadFrom(db.messages, db.accounts, open.accountId, open.threadId) : null);
  }

  async function reloadLive() {
    const next = await api.mailbox(queryRef.current, settingsRef.current.inboxTabs);
    setBox(next);
    try {
      setTemplates(await api.templatesList());
    } catch {
      /* templates stay as they were */
    }
    const open = openRef.current;
    if (!open) return;
    try {
      setThread(await api.threadGet(open.accountId, open.threadId));
    } catch {
      setThread(null);
      openRef.current = null;
    }
  }
  reloadRef.current = async () => {
    if (modeRef.current === "demo") publishDemo();
    else if (modeRef.current === "live") await reloadLive();
  };

  useEffect(() => {
    if (mode === "demo") publishDemo();
    else if (mode === "live") {
      void reloadLive().catch((error: unknown) => toastRef.current(messageOf(error)));
    }
    // publishDemo and reloadLive read the latest query through refs and state setters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, query]);

  // Archiving or deleting the last rows of the last page leaves it empty; step back to a page that has mail.
  useEffect(() => {
    if (remoteThreads || box.threads.length > 0 || query.offset === 0) return;
    const last = Math.max(0, Math.floor((box.total - 1) / query.limit) * query.limit);
    if (last < query.offset) showQuery({ ...queryRef.current, offset: last });
    // showQuery reads the latest query through queryRef.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [box, remoteThreads]);

  useEffect(() => {
    if (!undo) return;
    const timer = window.setTimeout(() => setUndo(null), 5000);
    return () => window.clearTimeout(timer);
  }, [undo]);

  useEffect(() => {
    if (mode !== "demo") return;
    const timer = window.setInterval(() => {
      const db = demoRef.current;
      if (!db) return;
      const now = Date.now();
      const due = db.snoozes.filter((item) => item.untilAt <= now);
      const dueMail = db.scheduled.filter((item) => item.sendAt <= now);
      if (due.length === 0 && dueMail.length === 0) return;
      for (const item of due) applyThreadAction(db.messages, item.accountId, item.threadId, "unarchive");
      db.snoozes = db.snoozes.filter((item) => item.untilAt > now);
      for (const item of dueMail) {
        deliverDemo(db, item.draft, false);
        toastRef.current(`Sent “${item.draft.subject || "message"}”`);
      }
      db.scheduled = db.scheduled.filter((item) => item.sendAt > now);
      if (due.length > 0) toastRef.current(due.length === 1 ? "A snoozed conversation is back in the inbox." : `${due.length} snoozed conversations are back.`);
      publishDemo();
    }, 2000);
    return () => window.clearInterval(timer);
  }, [mode]);

  useEffect(() => {
    if (settings.theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyAppearance(settings.theme, settings.density);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [settings.theme, settings.density]);

  useEffect(() => {
    let stop = false;
    const unlisten: Array<() => void> = [];
    void (async () => {
      applyAppearance(defaultSettings.theme, defaultSettings.density);
      if (!inTauri()) {
        const saved = localStorage.getItem("novabox-settings");
        if (saved) {
          try {
            const parsed = normalizeSettings(JSON.parse(saved) as Settings);
            setSettings(parsed);
            applyAppearance(parsed.theme, parsed.density);
          } catch {
            /* keep defaults */
          }
        }
        if (!stop) setReady(true);
        return;
      }
      try {
        void api
          .appInfo()
          .then((info) => setGoogleReady(info.googleReady))
          .catch(() => {});
        const loaded = normalizeSettings(await api.settingsGet());
        setSettings(loaded);
        applyAppearance(loaded.theme, loaded.density);
        const next = await api.mailbox(initialQuery, loaded.inboxTabs);
        if (stop) return;
        setBox(next);
        try {
          setTemplates(await api.templatesList());
        } catch {
          /* the inbox can open without templates */
        }
        if (next.accounts.length > 0) setMode("live");
        const { listen } = await import("@tauri-apps/api/event");
        unlisten.push(await listen("mail://updated", () => void reloadRef.current()));
        unlisten.push(
          await listen<{ email: string; phase: string }>("sync://progress", (event) => {
            setSyncing(true);
            setSyncLabel(`${event.payload.phase} · ${event.payload.email}`);
          }),
        );
        unlisten.push(
          await listen<{ ok: boolean; message: string; offline: boolean }>("sync://done", (event) => {
            setSyncing(false);
            setSyncLabel("");
            setOffline(Boolean(event.payload.offline));
            if (!event.payload.ok && event.payload.message) toastRef.current(event.payload.message);
          }),
        );
        unlisten.push(
          await listen<Omit<Notice, "id">>("mail://notify", (event) => deliverRef.current(event.payload, settingsRef.current)),
        );
        unlisten.push(await listen<Notice>("popup://open", (event) => openNoticeRef.current(event.payload)));
      } catch (error) {
        toastRef.current(messageOf(error));
      } finally {
        if (!stop) setReady(true);
      }
    })();
    return () => {
      stop = true;
      unlisten.forEach((stopListen) => stopListen());
    };
  }, []);

  function currentTargets(): ThreadTarget[] {
    if (checkedRef.current.length > 0) return checkedRef.current.map(parseThreadKey);
    return openRef.current ? [openRef.current] : [];
  }

  async function modify(targets: ThreadTarget[], action: string, labelId?: string, silent = false) {
    if (targets.length === 0) return;
    if (targets.some((target) => target.threadId.startsWith("sched:"))) {
      toast("Open a scheduled message to send it now or cancel it.");
      return;
    }
    if (modeRef.current === "demo" && demoRef.current) {
      for (const target of targets) applyThreadAction(demoRef.current.messages, target.accountId, target.threadId, action, labelId);
      publishDemo();
    } else if (modeRef.current === "live") {
      await api.threadsModify(targets, action, labelId);
      await reloadLive();
    }
    const reverse = REVERSE[action];
    if (!silent && reverse) setUndo({ text: ACTION_TEXT[action] ?? "Updated", targets, reverse, labelId });
    if (["archive", "trash", "spam"].includes(action)) closeThread();
  }

  async function act(action: string, labelId?: string, targets?: ThreadTarget[]) {
    try {
      await modify(targets && targets.length > 0 ? targets : currentTargets(), action, labelId);
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function undoAct() {
    if (!undo) return;
    const current = undo;
    setUndo(null);
    try {
      if (current.reverse === "unsnooze") await unsnooze(current.targets);
      else await modify(current.targets, current.reverse, current.labelId, true);
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function deleteForever(targets?: ThreadTarget[]) {
    const list = targets && targets.length > 0 ? targets : currentTargets();
    if (list.length === 0) return;
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        const db = demoRef.current;
        db.messages = db.messages.filter(
          (message) => !message.labelIds.includes("TRASH") || !list.some((target) => target.accountId === message.accountId && target.threadId === message.threadId),
        );
        publishDemo();
      } else if (modeRef.current === "live") {
        await api.threadsDeleteForever(list);
        await reloadLive();
      }
      setChecked([]);
      setUndo(null);
      closeThread();
      toast(list.length === 1 ? "Conversation deleted forever" : `${list.length} conversations deleted forever`);
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function emptyTrash() {
    const accountId = queryRef.current.accountId;
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        const db = demoRef.current;
        db.messages = db.messages.filter((message) => !message.labelIds.includes("TRASH") || (accountId !== null && message.accountId !== accountId));
        publishDemo();
      } else if (modeRef.current === "live") {
        await api.trashEmpty(accountId);
        await reloadLive();
      }
      setChecked([]);
      setUndo(null);
      closeThread();
      toast("Trash emptied");
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function snooze(until: number, targets?: ThreadTarget[]) {
    const list = targets && targets.length > 0 ? targets : currentTargets();
    if (list.length === 0 || list.some((target) => target.threadId.startsWith("sched:"))) return;
    if (until <= Date.now()) {
      toast("Choose a time in the future.");
      return;
    }
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        for (const target of list) {
          applyThreadAction(demoRef.current.messages, target.accountId, target.threadId, "archive");
          demoRef.current.snoozes = demoRef.current.snoozes.filter((item) => item.accountId !== target.accountId || item.threadId !== target.threadId);
          demoRef.current.snoozes.push({ accountId: target.accountId, threadId: target.threadId, untilAt: until });
        }
        publishDemo();
      } else if (modeRef.current === "live") {
        await api.threadsSnooze(list, until);
        await reloadLive();
      }
      setUndo({ text: "Snoozed", targets: list, reverse: "unsnooze" });
      closeThread();
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function unsnooze(targets?: ThreadTarget[]) {
    const list = targets && targets.length > 0 ? targets : currentTargets();
    if (list.length === 0) return;
    if (modeRef.current === "demo" && demoRef.current) {
      for (const target of list) {
        applyThreadAction(demoRef.current.messages, target.accountId, target.threadId, "unarchive");
        demoRef.current.snoozes = demoRef.current.snoozes.filter((item) => item.accountId !== target.accountId || item.threadId !== target.threadId);
      }
      publishDemo();
    } else if (modeRef.current === "live") {
      await api.threadsUnsnooze(list);
      await reloadLive();
    }
  }

  async function scheduleCompose(localId: string, sendAt: number) {
    const draft = composesRef.current.find((item) => item.localId === localId);
    if (!draft) return;
    if (!draft.to.trim()) {
      toast("Add a recipient before scheduling.");
      return;
    }
    if (sendAt <= Date.now() + 4000) {
      toast("Choose a time at least a few seconds from now.");
      return;
    }
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        demoRef.current.scheduled.push({ id: newId("sched"), sendAt, draft });
        publishDemo();
        toast("Scheduled in the sample inbox");
      } else {
        await api.mailSchedule(draft, sendAt);
        toast("Scheduled");
        await reloadLive();
      }
      setComposes((list) => list.filter((item) => item.localId !== localId));
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function cancelScheduled(id: string) {
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        demoRef.current.scheduled = demoRef.current.scheduled.filter((item) => item.id !== id);
        publishDemo();
      } else {
        await api.scheduleCancel(id);
        await reloadLive();
      }
      closeThread();
      toast("Schedule cancelled");
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function sendScheduled(id: string) {
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        const item = demoRef.current.scheduled.find((entry) => entry.id === id);
        if (!item) return;
        deliverDemo(demoRef.current, item.draft, false);
        demoRef.current.scheduled = demoRef.current.scheduled.filter((entry) => entry.id !== id);
        publishDemo();
        toast("Sent in the sample inbox");
      } else {
        await api.scheduleSendNow(id);
        toast("Sent");
        await reloadLive();
      }
      closeThread();
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function saveTemplate(name: string, subject: string, html: string) {
    const trimmed = name.trim();
    if (!trimmed) {
      toast("Name the template first.");
      return;
    }
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        const template = { id: newId("tpl"), name: trimmed, subject, html, updatedAt: Date.now() };
        demoRef.current.templates = [template, ...demoRef.current.templates.filter((item) => item.name !== trimmed)];
        setTemplates(demoRef.current.templates);
        toast("Template saved");
        return;
      }
      const saved = await api.templateSave({ id: "", name: trimmed, subject, html, updatedAt: 0 });
      setTemplates((list) => [saved, ...list.filter((item) => item.id !== saved.id)]);
      toast("Template saved");
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function deleteTemplate(id: string) {
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        demoRef.current.templates = demoRef.current.templates.filter((item) => item.id !== id);
        setTemplates(demoRef.current.templates);
        return;
      }
      await api.templateDelete(id);
      setTemplates((list) => list.filter((item) => item.id !== id));
    } catch (error) {
      toast(messageOf(error));
    }
  }

  // Routes one notification to the in-app stack and/or the desktop (Windows toast or
  // Novabox pop-up). A test ignores focus so you can see both while Settings is open.
  function deliver(payload: Omit<Notice, "id">, prefs: Settings, test = false) {
    const account = box.accounts.find((item) => item.id === payload.accountId);
    const notice: Notice = { ...payload, id: newId("notice"), accountEmail: payload.accountEmail ?? account?.email ?? null };
    const inFront = !test && document.visibilityState === "visible" && document.hasFocus();
    if (prefs.notifyInApp && (test || inFront || !prefs.notifyDesktop)) {
      // Keep the stack short; the newest notice goes on top.
      setNotices((list) => [notice, ...list].slice(0, 4));
    }
    if (!prefs.notifyDesktop || (prefs.notifyDesktopBackgroundOnly && inFront)) return;
    if (prefs.notifyStyle === "novabox") {
      const hidden: Record<string, string> = { mail: "You have new mail", sent: "A scheduled message was sent", error: "A scheduled message could not be sent" };
      const shown = prefs.popupPreview || !hidden[notice.kind] ? notice : { ...notice, sender: null, body: hidden[notice.kind] };
      void api.popupPush(shown, prefs.popupCorner || "bottomRight", prefs.popupMax || 3).catch((error: unknown) => console.error(error));
    } else {
      void api.notifyShow(notice.title, notice.body).catch((error: unknown) => console.error(error));
    }
  }
  deliverRef.current = deliver;

  function testNotify(prefs: Settings) {
    const account = box.accounts[0];
    deliver(
      {
        kind: "test",
        title: "Novabox",
        body: "New mail will look like this.",
        sender: "Novabox",
        accountColor: account?.color ?? null,
        accountEmail: account?.email ?? null,
      },
      prefs,
      true,
    );
  }

  function dismissNotice(id: string) {
    setNotices((list) => list.filter((item) => item.id !== id));
  }

  function openNotice(notice: Notice) {
    dismissNotice(notice.id);
    const { accountId, threadId } = notice;
    if (!accountId || !threadId) return;
    setSettingsOpen(false);
    const listed = threadsRef.current.find((item) => item.accountId === accountId && item.threadId === threadId);
    const account = box.accounts.find((item) => item.id === accountId);
    void openThread(
      listed ?? {
        accountId,
        accountEmail: account?.email ?? "",
        accountColor: account?.color ?? notice.accountColor ?? "",
        threadId,
        latestMessageId: "",
        fromName: notice.sender ?? "",
        fromEmail: "",
        subject: notice.body,
        snippet: "",
        date: Date.now(),
        // Not in the current list, so mark it read on open the same way a new arrival would be.
        unread: true,
        starred: false,
        hasAttachments: false,
        messageCount: 1,
        labelIds: [],
      },
    );
  }

  openNoticeRef.current = openNotice;

  async function openThread(summary: ThreadSummary) {
    const index = threadsRef.current.findIndex((item) => item.accountId === summary.accountId && item.threadId === summary.threadId);
    if (index >= 0) setCursor(index);
    if (summary.scheduledId) {
      openRef.current = { accountId: summary.accountId, threadId: summary.threadId };
      setThread(scheduledThread(summary));
      return;
    }
    openRef.current = { accountId: summary.accountId, threadId: summary.threadId };
    setThreadLoading(true);
    try {
      if (summary.unread) {
        try {
          await modify([openRef.current], "read", undefined, true);
        } catch (error) {
          toast(messageOf(error));
          if (modeRef.current === "demo") publishDemo();
          else setThread(await api.threadGet(summary.accountId, summary.threadId));
        }
      } else if (modeRef.current === "demo") publishDemo();
      else setThread(await api.threadGet(summary.accountId, summary.threadId));
    } catch (error) {
      toast(messageOf(error));
    } finally {
      setThreadLoading(false);
    }
  }

  function closeThread() {
    openRef.current = null;
    setThread(null);
  }

  function resetListState() {
    setChecked([]);
    setCursor(0);
    setRemoteThreads(null);
    setRemoteNote("");
    closeThread();
  }

  function showQuery(next: MailQuery) {
    queryRef.current = next;
    setQuery(next);
    if (modeRef.current === "demo") publishDemo();
  }

  function selectAccount(id: string | null) {
    resetListState();
    showQuery({ ...queryRef.current, accountId: id, offset: 0 });
  }

  function setFolder(folder: Folder, labelId: string | null = null) {
    resetListState();
    showQuery({ ...queryRef.current, folder, labelId, category: "primary" as Category, search: "", offset: 0 });
  }

  function setCategory(category: Category) {
    setCursor(0);
    closeThread();
    showQuery({ ...queryRef.current, category, offset: 0 });
  }

  function setSearch(search: string) {
    setRemoteThreads(null);
    setRemoteNote("");
    setCursor(0);
    showQuery({ ...queryRef.current, search, offset: 0 });
  }

  async function submitSearch() {
    const text = queryRef.current.search.trim();
    if (!text) return;
    if (modeRef.current !== "live") return;
    setSyncing(true);
    try {
      const found = await api.remoteSearch(queryRef.current.accountId, text);
      if (hasOperator(text)) {
        setRemoteThreads(found);
        setRemoteNote("Results from Google");
      } else {
        setRemoteNote("Older mail from Google is included");
        await reloadLive();
      }
    } catch (error) {
      toast(messageOf(error));
    } finally {
      setSyncing(false);
    }
  }

  function turnPage(step: 1 | -1) {
    const current = queryRef.current;
    const offset = Math.max(0, current.offset + step * current.limit);
    if (offset === current.offset) return;
    setChecked([]);
    setCursor(0);
    closeThread();
    showQuery({ ...current, offset });
  }

  function toggleCheck(key: string) {
    setChecked((list) => (list.includes(key) ? list.filter((item) => item !== key) : [...list, key]));
  }

  function toggleAllChecks() {
    const keys = visibleThreads.map((item) => threadKey(item.accountId, item.threadId));
    setChecked((list) => (list.length === keys.length ? [] : keys));
  }

  function openDemo() {
    const db = createDemo();
    demoRef.current = db;
    openRef.current = null;
    setThread(null);
    setChecked([]);
    setCursor(0);
    setComposes([]);
    setRemoteThreads(null);
    setRemoteNote("");
    setQuery(initialQuery);
    setBox(buildMailbox(db.messages, db.accounts, db.labels, initialQuery, db.snoozes, db.scheduled, settingsRef.current.inboxTabs));
    setTemplates(db.templates);
    setMode("demo");
  }

  function leaveDemo() {
    demoRef.current = null;
    openRef.current = null;
    setThread(null);
    setComposes([]);
    setMode("none");
    setBox(emptyBox);
  }

  async function signIn() {
    if (!inTauri()) {
      setSignInError("Open the Novabox window to sign in with Google. This browser view can explore the sample inbox.");
      return;
    }
    setSignInBusy(true);
    setSignInError("");
    try {
      // The callback server on 127.0.0.1:47821 has to stay up until Google redirects.
      // Closing it when this window is focused again makes the browser report connection refused.
      await api.signIn();
      setMode("live");
      await reloadLive();
    } catch (error) {
      const text = messageOf(error);
      if (!/sign-in was cancelled/i.test(text)) setSignInError(text);
    } finally {
      setSignInBusy(false);
    }
  }

  function cancelSignIn() {
    if (!inTauri()) {
      setSignInBusy(false);
      return;
    }
    void api.signInCancel();
  }

  async function removeAccount(id: string, revoke = true) {
    const account = box.accounts.find((item) => item.id === id);
    const label = account?.email ?? "this account";
    const question = revoke
      ? `Sign out of ${label}? Mail saved on this PC for that account will be deleted, and Google access will be revoked, which also signs it out on any other PC that uses the same sign-in from a backup.`
      : `Remove ${label} from this PC? Mail saved here for that account will be deleted. Google access is kept, so other PCs and your sign-in backups keep working.`;
    if (!window.confirm(question)) return;
    try {
      if (mode === "demo" && demoRef.current) {
        demoRef.current.accounts = demoRef.current.accounts.filter((item) => item.id !== id);
        demoRef.current.messages = demoRef.current.messages.filter((item) => item.accountId !== id);
        demoRef.current.labels = demoRef.current.labels.filter((item) => item.accountId !== id);
        if (demoRef.current.accounts.length === 0) leaveDemo();
        else publishDemo();
        return;
      }
      await api.accountRemove(id, revoke);
      if (box.accounts.length <= 1) {
        setMode("none");
        setBox(emptyBox);
        closeThread();
      } else await reloadLive();
    } catch (error) {
      toast(messageOf(error));
    }
  }

  // The mode switch waits for finishImport so the dialog (which may live on the welcome
  // screen) stays up long enough to show the per-account report.
  const importedRef = useRef(false);
  async function importSignIns(path: string, password: string): Promise<ImportItem[]> {
    const report = await api.backupImport(path, password);
    if (report.some((item) => item.status !== "failed")) importedRef.current = true;
    return report;
  }

  function finishImport() {
    if (!importedRef.current) return;
    importedRef.current = false;
    setSignInError("");
    if (modeRef.current === "demo") leaveDemo();
    resetListState();
    showQuery({ ...initialQuery });
    setMode("live");
    void reloadLive().catch((error: unknown) => toast(messageOf(error)));
  }

  async function saveSettings(next: Settings) {
    const saved = normalizeSettings(next);
    settingsRef.current = saved;
    setSettings(saved);
    applyAppearance(saved.theme, saved.density);
    const category = queryRef.current.category;
    if (queryRef.current.folder === "inbox" && category !== "primary" && !saved.inboxTabs[category]) {
      const nextQuery = { ...queryRef.current, category: "primary" as const, offset: 0 };
      queryRef.current = nextQuery;
      setQuery(nextQuery);
    }
    try {
      if (inTauri()) {
        await api.settingsSave(saved);
        const autostart = await import("@tauri-apps/plugin-autostart");
        if (saved.startWithWindows) await autostart.enable();
        else await autostart.disable();
      } else {
        localStorage.setItem("novabox-settings", JSON.stringify(saved));
      }
    } catch (error) {
      toast(messageOf(error));
    }
    await reloadRef.current();
  }

  async function setSignature(accountId: string, signature: string) {
    try {
      if (mode === "demo" && demoRef.current) {
        const account = demoRef.current.accounts.find((item) => item.id === accountId);
        if (account) account.signature = signature;
        publishDemo();
        return;
      }
      await api.setSignature(accountId, signature);
      await reloadLive();
    } catch (error) {
      toast(messageOf(error));
    }
  }

  function composeNew(accountId?: string) {
    const account = box.accounts.find((item) => item.id === (accountId ?? query.accountId)) ?? box.accounts[0];
    if (!account) return;
    pushCompose(blankCompose(account));
  }

  async function composeReply(replyMode: "reply" | "replyAll" | "forward") {
    let detail = thread;
    if (!detail) {
      const summary = threadsRef.current[cursorRef.current];
      if (!summary) return;
      if (modeRef.current === "demo" && demoRef.current) {
        detail = threadFrom(demoRef.current.messages, demoRef.current.accounts, summary.accountId, summary.threadId);
      } else if (modeRef.current === "live") {
        try {
          detail = await api.threadGet(summary.accountId, summary.threadId);
        } catch (error) {
          toast(messageOf(error));
          return;
        }
      }
    }
    if (!detail) return;
    const account = box.accounts.find((item) => item.id === detail.accountId);
    const message = detail.messages[detail.messages.length - 1];
    if (!account || !message) return;
    const mine = new Set([account.email, ...account.sendAs.map((alias) => alias.email)].map((email) => email.toLowerCase()));
    const isMine = mine.has(message.fromEmail.toLowerCase());
    let to = "";
    let cc = "";
    let subject = message.subject || "(no subject)";
    if (replyMode === "forward") {
      subject = subject.toLowerCase().startsWith("fwd:") ? subject : `Fwd: ${subject}`;
    } else {
      subject = subject.toLowerCase().startsWith("re:") ? subject : `Re: ${subject}`;
      const others = [...message.to, ...message.cc].filter((address) => !mine.has(address.email.toLowerCase()));
      if (replyMode === "reply") {
        to = isMine ? message.to.map((address) => address.email).join(", ") : message.fromEmail;
      } else {
        const primary = isMine ? [] : [message.fromEmail];
        to = [...primary, ...message.to.filter((address) => !mine.has(address.email.toLowerCase())).map((address) => address.email)].join(", ");
        cc = others.filter((address) => !to.toLowerCase().includes(address.email.toLowerCase())).map((address) => address.email).join(", ");
      }
    }
    const quoted = `<p>On ${escapeHtml(new Date(message.date).toLocaleString())}, ${escapeHtml(message.fromName || message.fromEmail)} wrote:</p><blockquote>${cleanFragment(message.bodyHtml || `<pre>${escapeHtml(message.bodyText)}</pre>`, false)}</blockquote>`;
    let attachments: ComposeDraft["attachments"] = [];
    if (replyMode === "forward" && message.attachments.length > 0) {
      try {
        attachments = await Promise.all(
          message.attachments.map(async (attachment) => {
            const file = await previewAttachment(account.id, message.id, attachment.id);
            return { filename: file.filename, mimeType: file.mimeType, dataBase64: file.dataBase64, size: attachment.size };
          }),
        );
      } catch {
        toast("Some attachments could not be added to the forward.");
      }
    }
    pushCompose({
      ...blankCompose(account),
      to,
      cc,
      showCc: Boolean(cc),
      subject,
      html: `<p><br></p>${signatureBlock(account)}${quoted}`,
      attachments,
      threadId: replyMode === "forward" ? undefined : detail.threadId,
      inReplyTo: replyMode === "forward" ? undefined : message.messageIdHeader,
      references: replyMode === "forward" ? undefined : [message.referencesHeader, message.messageIdHeader].filter(Boolean).join(" "),
    });
  }

  function pushCompose(draft: ComposeDraft) {
    if (composesRef.current.length >= 3) {
      toast("Close a compose window first.");
      return;
    }
    setComposes((list) => [...list, draft]);
  }

  function closeCompose(localId: string) {
    const draft = composesRef.current.find((item) => item.localId === localId);
    const dirty = draft && (draft.to.trim() || draft.subject.trim() || stripHtml(draft.html).trim());
    if (dirty && !window.confirm("Discard this message?")) return;
    setComposes((list) => list.filter((item) => item.localId !== localId));
  }

  function updateCompose(localId: string, patch: Partial<ComposeDraft>) {
    const next = composesRef.current.map((item) => {
      if (item.localId !== localId) return item;
      const updated = { ...item, ...patch };
      if (patch.fromEmail && patch.fromEmail !== item.fromEmail) {
        const account = accountsRef.current.find((candidate) => candidate.id === updated.accountId || candidate.sendAs.some((alias) => alias.email === updated.fromEmail));
        if (account) updated.html = replaceSignature(updated.html, account.signature);
      }
      if (patch.accountId && patch.accountId !== item.accountId) {
        const account = accountsRef.current.find((candidate) => candidate.id === patch.accountId);
        if (account) {
          updated.fromEmail = account.email;
          updated.html = replaceSignature(updated.html, account.signature);
        }
      }
      return updated;
    });
    composesRef.current = next;
    setComposes(next);
  }

  async function sendCompose(localId: string) {
    const draft = composesRef.current.find((item) => item.localId === localId);
    if (!draft) return;
    const total = draft.attachments.reduce((sum, file) => sum + file.size, 0);
    if (total > 24 * 1024 * 1024) {
      toast("Attachments are over Gmail's 25 MB limit.");
      return;
    }
    updateCompose(localId, { sending: true });
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        deliverDemo(demoRef.current, draft, false);
        publishDemo();
        toast("Sent in the sample inbox");
      } else {
        await api.mailSend(draft);
        toast("Sent");
        await reloadLive();
      }
      setComposes((list) => list.filter((item) => item.localId !== localId));
    } catch (error) {
      updateCompose(localId, { sending: false });
      toast(messageOf(error));
    }
  }

  async function saveDraftCompose(localId: string) {
    const draft = composesRef.current.find((item) => item.localId === localId);
    if (!draft) return;
    try {
      if (modeRef.current === "demo" && demoRef.current) {
        const saved = deliverDemo(demoRef.current, draft, true);
        updateCompose(localId, { draftId: saved });
        publishDemo();
        toast("Draft saved in the sample inbox");
      } else {
        const result = await api.draftSave(draft);
        updateCompose(localId, { draftId: result.draftId ?? undefined, threadId: result.threadId });
        toast("Draft saved");
        await reloadLive();
      }
    } catch (error) {
      toast(messageOf(error));
    }
  }

  async function refresh() {
    if (mode === "demo") {
      publishDemo();
      return;
    }
    if (mode !== "live") return;
    setSyncing(true);
    try {
      await api.syncNow();
      await reloadLive();
      setOffline(false);
    } catch (error) {
      const text = messageOf(error);
      setOffline(text.toLowerCase().includes("reach") || text.toLowerCase().includes("network"));
      toast(text);
    } finally {
      setSyncing(false);
    }
  }

  async function previewAttachment(accountId: string, messageId: string, attachmentId: string) {
    if (modeRef.current === "demo" && demoRef.current) {
      const file = demoRef.current.files.find((item) => item.accountId === accountId && item.messageId === messageId && item.attachmentId === attachmentId);
      if (!file) throw new Error("That sample file is missing.");
      return file;
    }
    return api.attachmentGet(accountId, messageId, attachmentId);
  }

  async function saveAttachment(accountId: string, messageId: string, attachmentId: string, filename: string) {
    try {
      if (!inTauri()) {
        const file = await previewAttachment(accountId, messageId, attachmentId);
        downloadBase64(file.filename || filename, file.mimeType, file.dataBase64);
        return;
      }
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ defaultPath: filename });
      if (!path) return;
      await api.attachmentSave(accountId, messageId, attachmentId, path);
      toast("Saved");
    } catch (error) {
      toast(messageOf(error));
    }
  }

  const apiValue: MailApi = {
    ready,
    mode,
    accounts: box.accounts,
    labels: box.labels,
    threads: visibleThreads,
    hasMore: remoteThreads ? false : box.hasMore,
    total: remoteThreads ? remoteThreads.length : box.total,
    counts: box.counts,
    query,
    thread,
    threadLoading,
    checked,
    cursor,
    settings,
    settingsOpen,
    sidebarOpen,
    composes,
    toasts,
    notices,
    undo,
    syncing,
    syncLabel,
    offline,
    signInBusy,
    signInError,
    googleReady,
    shownImages,
    remoteNote,
    openDemo,
    leaveDemo,
    signIn,
    cancelSignIn,
    removeAccount,
    importSignIns,
    finishImport,
    selectAccount,
    setFolder,
    setCategory,
    setSearch,
    submitSearch,
    turnPage,
    openThread,
    closeThread,
    toggleCheck,
    toggleAllChecks,
    setCursor,
    act,
    undoAct,
    deleteForever,
    emptyTrash,
    saveSettings,
    setSignature,
    setSettingsOpen,
    toggleSidebar: () => setSidebarOpen((open) => !open),
    composeNew,
    composeReply,
    closeCompose,
    updateCompose,
    sendCompose,
    saveDraftCompose,
    showImages: (messageId) => setShownImages((list) => (list.includes(messageId) ? list : [...list, messageId])),
    allowSender: (email) => {
      if (settings.imageAllowlist.includes(email)) return;
      void saveSettings({ ...settings, imageAllowlist: [...settings.imageAllowlist, email] });
    },
    refresh,
    previewAttachment,
    saveAttachment,
    dismissToast: (id) => setToasts((list) => list.filter((item) => item.id !== id)),
    dismissNotice,
    openNotice,
    testNotify,
    templates,
    snooze,
    unsnooze,
    scheduleCompose,
    cancelScheduled,
    sendScheduled,
    saveTemplate,
    deleteTemplate,
  };

  return <MailContext.Provider value={apiValue}>{children}</MailContext.Provider>;
}

function scheduledThread(summary: ThreadSummary): ThreadDetail {
  const to = (summary.previewTo ?? "")
    .split(",")
    .map((email) => email.trim())
    .filter(Boolean)
    .map((email) => ({ name: "", email }));
  return {
    accountId: summary.accountId,
    accountEmail: summary.accountEmail,
    accountColor: summary.accountColor,
    threadId: summary.threadId,
    subject: summary.subject,
    scheduledId: summary.scheduledId ?? undefined,
    sendsAt: summary.date,
    messages: [
      {
        accountId: summary.accountId,
        id: summary.scheduledId ?? summary.latestMessageId,
        threadId: summary.threadId,
        fromName: summary.fromName,
        fromEmail: summary.fromEmail,
        to,
        cc: [],
        bcc: [],
        subject: summary.subject,
        snippet: summary.snippet,
        bodyHtml: summary.bodyHtml ?? `<p>${escapeHtml(summary.snippet)}</p>`,
        bodyText: summary.snippet,
        date: summary.date,
        unread: false,
        starred: false,
        labelIds: [],
        attachments: [],
        messageIdHeader: "",
        referencesHeader: "",
      },
    ],
  };
}

function blankCompose(account: Account): ComposeDraft {
  return {
    localId: newId("compose"),
    accountId: account.id,
    fromEmail: account.email,
    to: "",
    cc: "",
    bcc: "",
    showCc: false,
    subject: "",
    html: `<p><br></p>${signatureBlock(account)}`,
    editorRev: 0,
    attachments: [],
    minimized: false,
    sending: false,
  };
}

function signatureBlock(account: Account): string {
  if (!account.signature.trim()) return "";
  return `<p><br></p><div data-signature="1">${escapeHtml(account.signature).replaceAll("\n", "<br>")}</div>`;
}

function replaceSignature(html: string, signature: string): string {
  const block = signature.trim() ? `<div data-signature="1">${escapeHtml(signature).replaceAll("\n", "<br>")}</div>` : "";
  if (html.includes('data-signature="1"')) {
    return html.replace(/<div data-signature="1">[\s\S]*?<\/div>/, block);
  }
  return `${html}${block ? `<p><br></p>${block}` : ""}`;
}

function deliverDemo(db: DemoDb, draft: ComposeDraft, asDraft: boolean): string {
  const account = db.accounts.find((item) => item.id === draft.accountId);
  const id = draft.draftId ? db.messages.find((message) => message.draftId === draft.draftId)?.id ?? newId("m") : newId("m");
  const plain = stripHtml(draft.html);
  const draftId = asDraft ? draft.draftId || newId("draft") : undefined;
  const message: MessageDetail = {
    id,
    accountId: draft.accountId,
    threadId: draft.threadId || id,
    fromName: account?.name ?? "",
    fromEmail: draft.fromEmail,
    to: addressesOf(draft.to),
    cc: addressesOf(draft.cc),
    bcc: addressesOf(draft.bcc),
    subject: draft.subject,
    snippet: plain.slice(0, 160),
    bodyHtml: draft.html,
    bodyText: plain,
    date: Date.now(),
    unread: false,
    starred: false,
    labelIds: asDraft ? ["DRAFT"] : ["SENT"],
    attachments: draft.attachments.map((file) => ({
      id: newId("att"),
      filename: file.filename,
      mimeType: file.mimeType,
      size: file.size,
    })),
    messageIdHeader: `<${id}@novabox.test>`,
    referencesHeader: draft.references ?? "",
    draftId,
  };
  db.files = db.files.filter((file) => !(file.accountId === draft.accountId && file.messageId === id));
  const index = db.messages.findIndex((item) => item.id === id);
  if (index >= 0) db.messages[index] = message;
  else db.messages.push(message);
  draft.attachments.forEach((file, fileIndex) => {
    const attachment = message.attachments[fileIndex];
    db.files.push({
      accountId: draft.accountId,
      messageId: id,
      attachmentId: attachment.id,
      filename: file.filename,
      mimeType: file.mimeType,
      dataBase64: file.dataBase64,
    });
  });
  return draftId ?? "";
}

function addressesOf(value: string): { name: string; email: string }[] {
  return value
    .split(/[,;]/)
    .map((part) => part.trim())
    .filter(Boolean)
    .map((email) => ({ name: "", email }));
}

function stripHtml(html: string): string {
  return html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();
}

function hasOperator(value: string): boolean {
  return /(^|\s)[a-z]+:/i.test(value);
}

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return "Something went wrong.";
}

function downloadBase64(filename: string, mimeType: string, dataBase64: string) {
  const bytes = Uint8Array.from(atob(dataBase64), (char) => char.charCodeAt(0));
  const url = URL.createObjectURL(new Blob([bytes], { type: mimeType }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}
