import { useCallback, useEffect, useRef, useState } from "react";
import { useDismiss } from "../useDismiss";
import { formatBytes, formatFullDate, formatPeople, initials, personName } from "../format";
import { blockPageMenu } from "../platform";
import { cleanEmail } from "../sanitize";
import { IconArchive, IconBack, IconStar, IconTrash } from "./Icons";
import { WhenMenu } from "./WhenMenu";
import { ConfirmDialog } from "./Dialogs";
import { useMail } from "../state/MailProvider";
import type { MessageDetail } from "../types";

export function ThreadView() {
  const mail = useMail();
  const [snoozeOpen, setSnoozeOpen] = useState(false);
  const snoozeArea = useRef<HTMLDivElement>(null);
  const closeSnooze = useCallback(() => setSnoozeOpen(false), []);
  useDismiss(snoozeArea, snoozeOpen, closeSnooze);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const thread = mail.thread;
  if (mail.threadLoading && !thread) return <div className="grid flex-1 place-items-center text-muted">Opening…</div>;
  if (!thread) return <div className="hidden flex-1 place-items-center text-muted lg:grid">Select a conversation</div>;
  const inTrash = thread.messages.some((message) => message.labelIds.includes("TRASH"));
  const labels = mail.labels.filter((label) => label.accountId === thread.accountId && label.kind === "user" && thread.messages.some((message) => message.labelIds.includes(label.id)));
  return (
    <section className="flex min-w-0 flex-1 flex-col bg-panel">
      <div className="flex items-center gap-1 border-b border-line px-2 py-1">
        {mail.settings.readingPane === "none" && (
          <button type="button" className="icon-btn" aria-label="Back to list" onClick={mail.closeThread}><IconBack /></button>
        )}
        {thread.scheduledId ? (
          <>
            <span className="px-2 text-sm text-muted">Sends {formatFullDate(thread.sendsAt ?? thread.messages[0]?.date ?? 0)}</span>
            <button type="button" className="tool-btn" onClick={() => void mail.sendScheduled(thread.scheduledId ?? "")}>Send now</button>
            <button type="button" className="tool-btn" onClick={() => void mail.cancelScheduled(thread.scheduledId ?? "")}>Cancel</button>
          </>
        ) : (
          <>
            <button type="button" className="icon-btn" aria-label="Archive" onClick={() => void mail.act("archive")}><IconArchive /></button>
            {inTrash ? (
              <button type="button" className="tool-btn" onClick={() => setConfirmDelete(true)}>Delete forever</button>
            ) : (
              <button type="button" className="icon-btn" aria-label="Delete" onClick={() => void mail.act("trash")}><IconTrash /></button>
            )}
            <button type="button" className="icon-btn" aria-label="Star" onClick={() => void mail.act(thread.messages.some((message) => message.starred) ? "unstar" : "star")}><IconStar filled={thread.messages.some((message) => message.starred)} /></button>
            <button type="button" className="tool-btn" onClick={() => void mail.act("unread")}>Unread</button>
            <div ref={snoozeArea} className="relative">
              {mail.query.folder === "snoozed" ? (
                <button type="button" className="tool-btn" onClick={() => void mail.unsnooze()}>Unsnooze</button>
              ) : (
                <>
                  <button type="button" className="tool-btn" aria-label="Snooze" onClick={() => setSnoozeOpen((open) => !open)}>Snooze</button>
                  {snoozeOpen && <WhenMenu title="Snooze until" onPick={(at) => void mail.snooze(at)} onClose={closeSnooze} />}
                </>
              )}
            </div>
            <div className="ml-auto flex gap-1 pr-2">
              <button type="button" className="tool-btn" onClick={() => void mail.composeReply("reply")}>Reply</button>
              <button type="button" className="tool-btn" onClick={() => void mail.composeReply("replyAll")}>Reply all</button>
              <button type="button" className="tool-btn" onClick={() => void mail.composeReply("forward")}>Forward</button>
            </div>
          </>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
        <h1 className="text-2xl font-semibold tracking-tight">{thread.subject || "(no subject)"}</h1>
        <div className="mt-2 flex flex-wrap gap-2">
          <span className="rounded-full px-2 py-0.5 text-xs text-white" style={{ background: thread.accountColor }}>{thread.accountEmail}</span>
          {labels.map((label) => (
            <span key={label.id} className="rounded-full px-2 py-0.5 text-xs" style={{ background: label.color || "var(--hover)" }}>{label.name}</span>
          ))}
        </div>
        <div className="mt-6 space-y-4">
          {thread.messages.map((message, index) => (
            <MessageCard key={message.id} message={message} startOpen={index === thread.messages.length - 1} />
          ))}
        </div>
      </div>
      {confirmDelete && (
        <ConfirmDialog
          title="Delete forever"
          body="The messages of this conversation that are in Trash will be deleted from Gmail permanently. This can't be undone."
          confirmLabel="Delete forever"
          onConfirm={() => void mail.deleteForever([{ accountId: thread.accountId, threadId: thread.threadId }])}
          onClose={() => setConfirmDelete(false)}
        />
      )}
    </section>
  );
}

function MessageCard({ message, startOpen }: { message: MessageDetail; startOpen: boolean }) {
  const mail = useMail();
  const [open, setOpen] = useState(startOpen);
  const [preview, setPreview] = useState<{ filename: string; mimeType: string; url: string } | null>(null);
  const allowRemote = mail.shownImages.includes(message.id) || mail.settings.imageAllowlist.includes(message.fromEmail);
  const cleaned = cleanEmail(message.bodyHtml || "", allowRemote);
  return (
    <article className="rounded-2xl border border-line">
      <button type="button" className="flex w-full items-start gap-3 px-4 py-3 text-left" onClick={() => setOpen((value) => !value)}>
        <span className="grid h-9 w-9 shrink-0 place-items-center rounded-full text-xs font-semibold text-white" style={{ background: mail.thread?.accountColor }}>{initials(message.fromName, message.fromEmail)}</span>
        <span className="min-w-0 flex-1">
          <span className="block truncate">
            <span className="font-medium">{personName(message.fromName, message.fromEmail)}</span>
            {open && message.fromName.trim() && <span className="ml-2 text-sm text-muted">{message.fromEmail}</span>}
          </span>
          {!open && <span className="block truncate text-sm text-muted">{message.snippet}</span>}
          {open && <span className="block text-sm text-muted">to {formatPeople(message.to) || "undisclosed"}{message.cc.length > 0 ? `, cc ${formatPeople(message.cc)}` : ""}</span>}
        </span>
        <span className="text-xs text-muted">{formatFullDate(message.date)}</span>
      </button>
      {open && (
        <div className="px-4 pb-4">
          {cleaned.warnings.length > 0 && (
            <div className="mb-3 rounded-xl bg-selected px-3 py-2 text-sm">
              {cleaned.warnings.map((warning) => <p key={warning}>{warning}</p>)}
            </div>
          )}
          {cleaned.blockedImages > 0 && !allowRemote && (
            <button type="button" className="mb-3 text-sm text-accent" onClick={() => mail.showImages(message.id)}>
              Images blocked ({cleaned.blockedImages}). Show images
            </button>
          )}
          {allowRemote && !mail.settings.imageAllowlist.includes(message.fromEmail) && (
            <button type="button" className="mb-3 block text-sm text-muted" onClick={() => mail.allowSender(message.fromEmail)}>
              Always show images from {message.fromEmail}
            </button>
          )}
          {message.bodyHtml ? <EmailFrame html={cleaned.html} /> : <pre className="whitespace-pre-wrap font-sans">{message.bodyText}</pre>}
          {message.attachments.length > 0 && (
            <div className="mt-3 flex flex-wrap gap-2">
              {message.attachments.map((attachment) => (
                <div key={attachment.id} className="rounded-xl border border-line px-3 py-2 text-sm">
                  <div className="font-medium">{attachment.filename}</div>
                  <div className="text-xs text-muted">{formatBytes(attachment.size)}</div>
                  <div className="mt-1 flex gap-3">
                    <button type="button" className="text-accent" onClick={() => void openPreview(mail, message, attachment.id, setPreview)}>Preview</button>
                    <button type="button" className="text-accent" onClick={() => void mail.saveAttachment(message.accountId, message.id, attachment.id, attachment.filename)}>Save</button>
                  </div>
                </div>
              ))}
            </div>
          )}
          {preview && (
            <div className="mt-3 rounded-xl border border-line bg-white p-3">
              <div className="mb-2 flex items-center justify-between text-sm">
                <span>{preview.filename}</span>
                <button type="button" onClick={() => { URL.revokeObjectURL(preview.url); setPreview(null); }}>Close</button>
              </div>
              {preview.mimeType.startsWith("image/") ? <img src={preview.url} alt="" className="max-h-96 max-w-full" /> : preview.mimeType === "application/pdf" ? <iframe title={preview.filename} src={preview.url} className="h-96 w-full" /> : <p className="text-sm text-muted">Preview is not available for this file type. Save it to open it.</p>}
            </div>
          )}
        </div>
      )}
    </article>
  );
}

function EmailFrame({ html }: { html: string }) {
  const [frame, setFrame] = useState<HTMLIFrameElement | null>(null);
  useEffect(() => {
    if (!frame) return;
    const fit = () => {
      const height = frame.contentDocument?.documentElement.scrollHeight ?? 80;
      frame.style.height = `${Math.max(height, 40)}px`;
    };
    // Each srcDoc load makes a fresh document, so the menu blocker is attached on every load.
    const loaded = () => {
      if (frame.contentDocument) blockPageMenu(frame.contentDocument);
      fit();
    };
    frame.addEventListener("load", loaded);
    fit();
    return () => frame.removeEventListener("load", loaded);
  }, [frame, html]);
  return (
    <iframe
      ref={setFrame}
      title="Message"
      sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
      referrerPolicy="no-referrer"
      srcDoc={html}
      className="w-full rounded-xl bg-white"
    />
  );
}

async function openPreview(
  mail: ReturnType<typeof useMail>,
  message: MessageDetail,
  attachmentId: string,
  setPreview: (value: { filename: string; mimeType: string; url: string } | null) => void,
) {
  const file = await mail.previewAttachment(message.accountId, message.id, attachmentId);
  const bytes = Uint8Array.from(atob(file.dataBase64), (char) => char.charCodeAt(0));
  setPreview({ filename: file.filename, mimeType: file.mimeType, url: URL.createObjectURL(new Blob([bytes], { type: file.mimeType })) });
}
