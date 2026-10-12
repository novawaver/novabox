import type { SnoozeRow, ScheduledRow } from "./folders";
import type { Account, Label, MessageDetail, Template } from "./types";

export type DemoFile = {
  accountId: string;
  messageId: string;
  attachmentId: string;
  filename: string;
  mimeType: string;
  dataBase64: string;
};

export type DemoDb = {
  accounts: Account[];
  labels: Label[];
  messages: MessageDetail[];
  files: DemoFile[];
  snoozes: SnoozeRow[];
  scheduled: ScheduledRow[];
  templates: Template[];
};

const hour = 60 * 60 * 1000;

export function createDemo(): DemoDb {
  const now = Date.now();
  const maya: Account = {
    id: "demo-maya",
    email: "maya@northmail.test",
    name: "Maya Chen",
    color: "#C9842A",
    unread: 0,
    signature: "Maya Chen\nPhotographer",
    sendAs: [{ email: "maya@northmail.test", displayName: "Maya Chen", isDefault: true, isPrimary: true }],
  };
  const studio: Account = {
    id: "demo-studio",
    email: "studio@northwind.test",
    name: "Northwind Studio",
    color: "#2F7D6D",
    unread: 0,
    signature: "Northwind Studio\nReplies within one working day",
    sendAs: [{ email: "studio@northwind.test", displayName: "Northwind Studio", isDefault: true, isPrimary: true }],
  };
  const harbor: Account = {
    id: "demo-harbor",
    email: "projects@harbor.test",
    name: "Harbor & Co",
    color: "#3D5A80",
    unread: 0,
    signature: "Projects desk\nHarbor & Co",
    sendAs: [{ email: "projects@harbor.test", displayName: "Harbor & Co", isDefault: true, isPrimary: true }],
  };
  const labels: Label[] = [
    { accountId: maya.id, id: "Label_clients", name: "Clients", kind: "user", color: "#f6d7a8", unread: 0 },
    { accountId: studio.id, id: "Label_invoices", name: "Invoices", kind: "user", color: "#cfe3dc", unread: 0 },
    { accountId: harbor.id, id: "Label_travel", name: "Travel", kind: "user", color: "#d5e2f2", unread: 0 },
  ];
  const proof = `<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360" fill="#f6f1e7"/><rect x="36" y="36" width="568" height="288" fill="#fff" stroke="#e0d8cb"/><text x="56" y="170" font-family="Segoe UI,sans-serif" font-size="28" fill="#1e1a15">Harbor lobby, frame 12</text><text x="56" y="210" font-family="Segoe UI,sans-serif" font-size="16" fill="#746c60">Sample proof from the Tuesday shoot</text></svg>`;
  const messages: MessageDetail[] = [
    msg({
      id: "m-lease",
      accountId: maya.id,
      threadId: "t-lease",
      fromName: "Rae Okonkwo",
      fromEmail: "rae@lindenhomes.test",
      to: [{ name: "Maya Chen", email: maya.email }],
      subject: "Lease renewal for March",
      date: now - 2 * hour,
      labelIds: ["INBOX", "UNREAD", "CATEGORY_PERSONAL"],
      bodyText: "The renewal is ready. Same rent, twelve months, starting March 1. Send it back when you can.",
      bodyHtml: "<p>Hi Maya,</p><p>The renewal is ready. Same rent, twelve months, starting March 1. Send it back when you can.</p><p>Rae<br>Linden Homes</p>",
    }),
    msg({
      id: "m-shoot",
      accountId: maya.id,
      threadId: "t-shoot",
      fromName: "Jonah Pell",
      fromEmail: "jonah@fieldday.test",
      to: [{ name: "Maya Chen", email: maya.email }],
      subject: "Saturday shoot — lobby light",
      date: now - 5 * hour,
      labelIds: ["INBOX", "UNREAD", "CATEGORY_PERSONAL", "Label_clients"],
      bodyText: "Can you be at the Harbor lobby at 8:30? The east windows are only good until 10.",
      bodyHtml: "<p>Maya,</p><p>Can you be at the Harbor lobby at 8:30? The east windows are only good until 10.</p><p>Jonah</p>",
    }),
    msg({
      id: "m-invoice",
      accountId: maya.id,
      threadId: "t-invoice",
      fromName: "Harbor & Co",
      fromEmail: "accounts@harbor.test",
      to: [{ name: "Maya Chen", email: maya.email }],
      subject: "Invoice 1842 paid",
      date: now - 26 * hour,
      labelIds: ["INBOX", "STARRED", "CATEGORY_PERSONAL", "Label_clients"],
      bodyText: "We sent the transfer for invoice 1842 this morning. Thank you for the lobby set.",
      bodyHtml: "<p>Maya,</p><p>We sent the transfer for invoice 1842 this morning. Thank you for the lobby set.</p>",
    }),
    msg({
      id: "m-sent",
      accountId: maya.id,
      threadId: "t-sent",
      fromName: "Maya Chen",
      fromEmail: maya.email,
      to: [{ name: "Jonah Pell", email: "jonah@fieldday.test" }],
      subject: "Re: Saturday shoot — lobby light",
      date: now - 4 * hour,
      labelIds: ["SENT"],
      bodyText: "8:30 works. I'll bring the 35mm and a small fill.",
      bodyHtml: "<p>8:30 works. I'll bring the 35mm and a small fill.</p><p>Maya Chen<br>Photographer</p>",
    }),
    msg({
      id: "m-trash",
      accountId: maya.id,
      threadId: "t-trash",
      fromName: "Paper Club",
      fromEmail: "news@paperclub.test",
      to: [{ name: "Maya Chen", email: maya.email }],
      subject: "Your weekly paper list",
      date: now - 9 * 24 * hour,
      labelIds: ["TRASH", "CATEGORY_PROMOTIONS"],
      bodyText: "Ten articles you already finished.",
      bodyHtml: "<p>Ten articles you already finished.</p>",
    }),
    msg({
      id: "m-brief",
      accountId: studio.id,
      threadId: "t-proof",
      fromName: "Imani Brooks",
      fromEmail: "imani@harbor.test",
      to: [{ name: "Northwind Studio", email: studio.email }],
      subject: "Lobby proof for Thursday",
      date: now - 30 * hour,
      labelIds: ["INBOX", "CATEGORY_PERSONAL"],
      bodyText: "Please send a wide frame of the lobby desk before Thursday's review.",
      bodyHtml: "<p>Hello,</p><p>Please send a wide frame of the lobby desk before Thursday's review.</p><p>Imani Brooks<br>Harbor &amp; Co</p>",
    }),
    msg({
      id: "m-reply",
      accountId: studio.id,
      threadId: "t-proof",
      fromName: "Northwind Studio",
      fromEmail: studio.email,
      to: [{ name: "Imani Brooks", email: "imani@harbor.test" }],
      subject: "Re: Lobby proof for Thursday",
      date: now - 20 * hour,
      labelIds: ["SENT"],
      bodyText: "Attached is frame 12 from Tuesday. Happy to crop tighter if the desk should lead.",
      bodyHtml: "<p>Imani,</p><p>Attached is frame 12 from Tuesday. Happy to crop tighter if the desk should lead.</p>",
    }),
    msg({
      id: "m-proof",
      accountId: studio.id,
      threadId: "t-proof",
      fromName: "Imani Brooks",
      fromEmail: "imani@harbor.test",
      to: [{ name: "Northwind Studio", email: studio.email }],
      cc: [{ name: "Maya Chen", email: maya.email }],
      subject: "Re: Lobby proof for Thursday",
      date: now - 70 * 60 * 1000,
      labelIds: ["INBOX", "UNREAD", "CATEGORY_PERSONAL"],
      bodyText: "Frame 12 is the one. Can you also send the notes from the walk-through?",
      bodyHtml: "<p>Frame 12 is the one. Can you also send the notes from the walk-through?</p><p>Imani</p>",
      attachments: [{ id: "att-proof", filename: "frame-12.svg", mimeType: "image/svg+xml", size: proof.length }],
    }),
    msg({
      id: "m-promo",
      accountId: studio.id,
      threadId: "t-promo",
      fromName: "Fieldnote Supply",
      fromEmail: "hello@fieldnote.test",
      to: [{ name: "Northwind Studio", email: studio.email }],
      subject: "Spring paper sale ends Friday",
      date: now - 8 * hour,
      labelIds: ["INBOX", "UNREAD", "CATEGORY_PROMOTIONS"],
      bodyText: "Twenty percent off cotton paper through Friday. The tracking image below stays blocked until you allow it.",
      bodyHtml:
        "<p>Twenty percent off cotton paper through Friday.</p><p><img src=\"https://example.com/pixel.gif\" width=\"600\" alt=\"Sale banner\"></p><p><a href=\"https://fieldnote.test/sale\">fieldnote.test/sale</a></p>",
    }),
    msg({
      id: "m-draft",
      accountId: studio.id,
      threadId: "t-draft",
      fromName: "Northwind Studio",
      fromEmail: studio.email,
      to: [{ name: "Imani Brooks", email: "imani@harbor.test" }],
      subject: "Thursday review times",
      date: now - 3 * hour,
      labelIds: ["DRAFT"],
      draftId: "draft-1",
      bodyText: "We can do 2pm or 4pm on Thursday. Draft, not sent.",
      bodyHtml: "<p>We can do 2pm or 4pm on Thursday.</p>",
    }),
    msg({
      id: "m-social",
      accountId: harbor.id,
      threadId: "t-social",
      fromName: "Lumen Gallery",
      fromEmail: "desk@lumen.test",
      to: [{ name: "Harbor & Co", email: harbor.email }],
      subject: "Ada commented on the lobby set",
      date: now - 6 * hour,
      labelIds: ["INBOX", "UNREAD", "CATEGORY_SOCIAL"],
      bodyText: "Ada wrote: the desk lamp is the picture. The rest of the room can wait.",
      bodyHtml: "<p><strong>Ada</strong> commented on the lobby set:</p><blockquote>The desk lamp is the picture. The rest of the room can wait.</blockquote>",
    }),
    msg({
      id: "m-ship",
      accountId: harbor.id,
      threadId: "t-ship",
      fromName: "Northline Parcel",
      fromEmail: "track@northline.test",
      to: [{ name: "Harbor & Co", email: harbor.email }],
      subject: "Delivered: sample books",
      date: now - 15 * hour,
      labelIds: ["INBOX", "CATEGORY_UPDATES"],
      bodyText: "The sample books were left at the front desk at 11:40.",
      bodyHtml: "<p>The sample books were left at the front desk at 11:40.</p>",
    }),
    msg({
      id: "m-travel",
      accountId: harbor.id,
      threadId: "t-travel",
      fromName: "Sable Hotel",
      fromEmail: "stay@sable.test",
      to: [{ name: "Harbor & Co", email: harbor.email }],
      subject: "Confirmation for the Portland stay",
      date: now - 12 * hour,
      labelIds: ["INBOX", "UNREAD", "CATEGORY_PERSONAL", "Label_travel"],
      bodyText: "Two nights, arriving Thursday. Late checkout is confirmed.",
      bodyHtml: "<p>Two nights, arriving Thursday. Late checkout is confirmed.</p><p>Sable Hotel</p>",
    }),
    msg({
      id: "m-spam",
      accountId: harbor.id,
      threadId: "t-spam",
      fromName: "Account Service",
      fromEmail: "prize@mailbox-check.test",
      to: [{ name: "Harbor & Co", email: harbor.email }],
      subject: "Your mailbox will close tonight",
      date: now - 1 * hour,
      labelIds: ["SPAM", "UNREAD"],
      bodyText: "This one pretends to be a sign-in page. The link text and the destination do not match.",
      bodyHtml:
        "<p>Verify your mailbox tonight or it will close.</p><p><a href=\"http://evil.example/login\">https://accounts.google.com</a></p><img src=\"https://evil.example/pixel.gif\" width=\"1\" height=\"1\" alt=\"\">",
    }),
  ];
  return {
    accounts: [maya, studio, harbor],
    labels,
    messages,
    files: [
      {
        accountId: studio.id,
        messageId: "m-proof",
        attachmentId: "att-proof",
        filename: "frame-12.svg",
        mimeType: "image/svg+xml",
        dataBase64: btoa(proof),
      },
    ],
    snoozes: [],
    scheduled: [],
    templates: [
      {
        id: "tpl-thanks",
        name: "Thanks",
        subject: "Thank you",
        html: "<p>Thank you. I'll follow up shortly.</p>",
        updatedAt: now,
      },
    ],
  };
}

function msg(partial: Partial<MessageDetail> & Pick<MessageDetail, "id" | "accountId" | "threadId" | "subject" | "date" | "labelIds">): MessageDetail {
  const message: MessageDetail = {
    fromName: "",
    fromEmail: "",
    to: [],
    cc: [],
    bcc: [],
    snippet: "",
    bodyHtml: "",
    bodyText: "",
    unread: false,
    starred: false,
    attachments: [],
    messageIdHeader: `<${partial.id}@novabox.test>`,
    referencesHeader: "",
    ...partial,
  };
  message.unread = message.labelIds.includes("UNREAD");
  message.starred = message.labelIds.includes("STARRED");
  if (!message.snippet) message.snippet = message.bodyText.slice(0, 160);
  return message;
}

export function newId(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 10)}`;
}
