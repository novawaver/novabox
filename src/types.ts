export type SendAs = {
  email: string;
  displayName: string;
  isDefault: boolean;
  isPrimary: boolean;
};

export type Account = {
  id: string;
  email: string;
  name: string;
  picture?: string | null;
  color: string;
  unread: number;
  signature: string;
  sendAs: SendAs[];
};

export type Label = {
  accountId: string;
  id: string;
  name: string;
  kind: string;
  color?: string | null;
  unread: number;
};

export type Address = {
  name: string;
  email: string;
};

export type AttachmentMeta = {
  id: string;
  filename: string;
  mimeType: string;
  size: number;
};

export type ThreadSummary = {
  accountId: string;
  accountEmail: string;
  accountColor: string;
  threadId: string;
  latestMessageId: string;
  fromName: string;
  fromEmail: string;
  subject: string;
  snippet: string;
  date: number;
  unread: boolean;
  starred: boolean;
  hasAttachments: boolean;
  messageCount: number;
  labelIds: string[];
  snoozeUntil?: number | null;
  scheduledId?: string | null;
  bodyHtml?: string | null;
  previewTo?: string | null;
};

export type Template = {
  id: string;
  name: string;
  subject: string;
  html: string;
  updatedAt: number;
};

export type MessageDetail = {
  accountId: string;
  id: string;
  threadId: string;
  fromName: string;
  fromEmail: string;
  to: Address[];
  cc: Address[];
  bcc: Address[];
  subject: string;
  snippet: string;
  bodyHtml: string;
  bodyText: string;
  date: number;
  unread: boolean;
  starred: boolean;
  labelIds: string[];
  attachments: AttachmentMeta[];
  messageIdHeader: string;
  referencesHeader: string;
  draftId?: string | null;
};

export type ThreadDetail = {
  accountId: string;
  accountEmail: string;
  accountColor: string;
  threadId: string;
  subject: string;
  messages: MessageDetail[];
  scheduledId?: string;
  sendsAt?: number;
};

export type InboxTabs = {
  promotions: boolean;
  social: boolean;
  updates: boolean;
};

export type Settings = {
  theme: string;
  density: string;
  readingPane: string;
  startWithWindows: boolean;
  imageAllowlist: string[];
  inboxTabs: InboxTabs;
  notifyInApp: boolean;
  notifyDesktop: boolean;
  notifyDesktopBackgroundOnly: boolean;
  /** "windows" for native toasts, "novabox" for Novabox's own corner pop-ups. */
  notifyStyle: string;
  popupCorner: string;
  popupMax: number;
  popupPreview: boolean;
};

export type FolderCounts = {
  inbox: number;
  inboxUnread: number;
  starred: number;
  drafts: number;
  primaryUnread: number;
  promotionsUnread: number;
  socialUnread: number;
  updatesUnread: number;
  spamUnread: number;
  snoozed: number;
  scheduled: number;
};

export type Mailbox = {
  accounts: Account[];
  labels: Label[];
  threads: ThreadSummary[];
  hasMore: boolean;
  total: number;
  counts: FolderCounts;
};

export type Folder = "inbox" | "starred" | "sent" | "drafts" | "snoozed" | "scheduled" | "spam" | "trash" | "label";
export type Category = "all" | "primary" | "promotions" | "social" | "updates";

export type MailQuery = {
  accountId: string | null;
  folder: Folder;
  labelId: string | null;
  category: Category;
  search: string;
  limit: number;
  offset: number;
};

export type ThreadTarget = {
  accountId: string;
  threadId: string;
};

export type ComposeFile = {
  filename: string;
  mimeType: string;
  dataBase64: string;
  size: number;
};

export type ComposeDraft = {
  localId: string;
  accountId: string;
  fromEmail: string;
  to: string;
  cc: string;
  bcc: string;
  showCc: boolean;
  subject: string;
  html: string;
  editorRev: number;
  attachments: ComposeFile[];
  threadId?: string;
  inReplyTo?: string;
  references?: string;
  draftId?: string;
  minimized: boolean;
  sending: boolean;
};

export type SendResult = {
  messageId: string;
  threadId: string;
  draftId?: string | null;
};

export type AppInfo = {
  version: string;
  dataDir: string;
  redirectUri: string;
  /** False when this build has no Google client ID baked in. */
  googleReady: boolean;
};

export const defaultInboxTabs: InboxTabs = {
  promotions: true,
  social: true,
  updates: false,
};

export const defaultSettings: Settings = {
  theme: "system",
  density: "default",
  readingPane: "none",
  startWithWindows: false,
  imageAllowlist: [],
  inboxTabs: defaultInboxTabs,
  notifyInApp: true,
  notifyDesktop: true,
  notifyDesktopBackgroundOnly: true,
  notifyStyle: "windows",
  popupCorner: "bottomRight",
  popupMax: 3,
  popupPreview: true,
};

export const emptyCounts: FolderCounts = {
  inbox: 0,
  inboxUnread: 0,
  starred: 0,
  drafts: 0,
  primaryUnread: 0,
  promotionsUnread: 0,
  socialUnread: 0,
  updatesUnread: 0,
  spamUnread: 0,
  snoozed: 0,
  scheduled: 0,
};

export const initialQuery: MailQuery = {
  accountId: null,
  folder: "inbox",
  labelId: null,
  category: "primary",
  search: "",
  limit: 50,
  offset: 0,
};

export type Notice = {
  id: string;
  kind: string;
  title: string;
  body: string;
  accountId?: string | null;
  accountColor?: string | null;
  accountEmail?: string | null;
  threadId?: string | null;
  sender?: string | null;
};
