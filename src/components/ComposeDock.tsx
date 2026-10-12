import { useEffect, useRef, useState, type FocusEvent, type ReactNode, type RefObject } from "react";
import { useMail } from "../state/MailProvider";
import type { ComposeDraft } from "../types";
import { IconBold, IconClearFormat, IconItalic, IconLink, IconListBullet, IconListNumber, IconStrike, IconUnderline } from "./Icons";
import { WhenMenu } from "./WhenMenu";

export function ComposeDock() {
  const mail = useMail();
  return (
    <div className="pointer-events-none fixed bottom-0 right-4 z-30 flex items-end gap-3">
      {mail.composes.map((draft) => (
        <ComposeWindow key={draft.localId} draft={draft} />
      ))}
    </div>
  );
}

function ComposeWindow({ draft }: { draft: ComposeDraft }) {
  const mail = useMail();
  const editor = useRef<HTMLDivElement>(null);
  // Only one footer pop-up at a time.
  const [menu, setMenu] = useState<"later" | "templates" | null>(null);
  const menuArea = useRef<HTMLElement>(null);
  const [templateName, setTemplateName] = useState("");
  const [ccOpen, setCcOpen] = useState(draft.showCc || draft.cc.trim() !== "");
  const [bccOpen, setBccOpen] = useState(draft.bcc.trim() !== "");
  const ccInput = useRef<HTMLInputElement>(null);
  const bccInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!menu) return;
    const onDown = (event: MouseEvent) => {
      if (!menuArea.current?.contains(event.target as Node)) setMenu(null);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [menu]);

  // A blank Cc/Bcc folds away as soon as focus lands anywhere else in this window. Reacting
  // to where focus arrives (not to the blur) also covers clicks on a label, and switching
  // to another app never collapses anything.
  const collapseBlankFields = (event: FocusEvent<HTMLElement>) => {
    if (ccOpen && event.target !== ccInput.current && draft.cc.trim() === "") {
      setCcOpen(false);
      mail.updateCompose(draft.localId, { showCc: false });
    }
    if (bccOpen && event.target !== bccInput.current && draft.bcc.trim() === "") setBccOpen(false);
  };
  const account = mail.accounts.find((item) => item.id === draft.accountId) ?? mail.accounts[0];
  const fromOptions = mail.accounts.flatMap((item) => item.sendAs.map((alias) => ({ accountId: item.id, email: alias.email, name: alias.displayName || item.name })));
  return (
    <section
      onFocus={collapseBlankFields}
      className="pointer-events-auto flex w-[480px] max-w-[calc(100vw-2rem)] flex-col overflow-hidden rounded-t-2xl border border-line bg-panel shadow-[var(--shadow)]"
    >
      <header className="flex items-center bg-[#241c14] px-3 py-2 text-sm text-[#f6f1e7]">
        <span className="flex-1">{draft.subject.trim() || "New message"}</span>
        <button type="button" className="px-2" onClick={() => mail.updateCompose(draft.localId, { minimized: !draft.minimized })}>{draft.minimized ? "▢" : "–"}</button>
        <button type="button" className="px-2" aria-label="Close compose" onClick={() => mail.closeCompose(draft.localId)}>×</button>
      </header>
      {!draft.minimized && (
        <>
          <label className="flex items-center gap-2 border-b border-line px-3 py-2 text-sm">
            <span className="w-12 text-muted">From</span>
            <select
              className="w-full bg-transparent outline-none"
              value={`${draft.accountId}|${draft.fromEmail}`}
              onChange={(event) => {
                const [accountId, fromEmail] = event.target.value.split("|");
                mail.updateCompose(draft.localId, { accountId, fromEmail });
              }}
            >
              {(fromOptions.length > 0 ? fromOptions : [{ accountId: account?.id ?? "", email: account?.email ?? "", name: account?.name ?? "" }]).map((option) => (
                <option key={`${option.accountId}|${option.email}`} value={`${option.accountId}|${option.email}`}>
                  {`${option.name} <${option.email}>`}
                </option>
              ))}
            </select>
          </label>
          <Field
            label="To"
            value={draft.to}
            onChange={(to) => mail.updateCompose(draft.localId, { to })}
            trailing={
              <span className="flex shrink-0 gap-1">
                {!ccOpen && (
                  <button type="button" className="rounded-md px-1.5 py-0.5 text-xs font-medium text-muted hover:bg-hover hover:text-ink" onMouseDown={(event) => event.preventDefault()} onClick={() => { setCcOpen(true); requestAnimationFrame(() => ccInput.current?.focus()); }}>
                    Cc
                  </button>
                )}
                {!bccOpen && (
                  <button type="button" className="rounded-md px-1.5 py-0.5 text-xs font-medium text-muted hover:bg-hover hover:text-ink" onMouseDown={(event) => event.preventDefault()} onClick={() => { setBccOpen(true); requestAnimationFrame(() => bccInput.current?.focus()); }}>
                    Bcc
                  </button>
                )}
              </span>
            }
          />
          {ccOpen && (
            <Field
              label="Cc"
              inputRef={ccInput}
              value={draft.cc}
              onChange={(cc) => mail.updateCompose(draft.localId, { cc })}
            />
          )}
          {bccOpen && (
            <Field label="Bcc" inputRef={bccInput} value={draft.bcc} onChange={(bcc) => mail.updateCompose(draft.localId, { bcc })} />
          )}
          <Field label="Subject" value={draft.subject} onChange={(subject) => mail.updateCompose(draft.localId, { subject })} />
          <RichEditor
            editor={editor}
            resetKey={`${draft.localId}:${draft.accountId}:${draft.fromEmail}:${draft.editorRev}`}
            html={draft.html}
            onChange={(html) => mail.updateCompose(draft.localId, { html })}
          />
          {draft.attachments.length > 0 && (
            <div className="flex flex-wrap gap-2 px-3 pb-2 text-xs">
              {draft.attachments.map((file) => (
                <span key={file.filename} className="rounded-full bg-hover px-2 py-1">{file.filename}</span>
              ))}
            </div>
          )}
          <FormatBar editor={editor} />
          <footer ref={menuArea} className="flex items-center gap-2 px-3 pb-3 pt-1">
            <button type="button" className="rounded-full bg-accent px-5 py-2 font-semibold text-white shadow-sm hover:brightness-95 disabled:opacity-60" disabled={draft.sending} onClick={() => { syncHtml(); void mail.sendCompose(draft.localId); }}>
              {draft.sending ? "Sending…" : "Send"}
            </button>
            <div className="relative">
              <button type="button" aria-expanded={menu === "later"} className={`rounded-full px-3 py-2 text-sm hover:bg-hover ${menu === "later" ? "bg-hover" : ""}`} onClick={() => { syncHtml(); setMenu((open) => (open === "later" ? null : "later")); }}>Send later</button>
              {menu === "later" && (
                <WhenMenu
                  title="Send at"
                  upward
                  onPick={(at) => {
                    syncHtml();
                    void mail.scheduleCompose(draft.localId, at);
                  }}
                  onClose={() => setMenu(null)}
                />
              )}
            </div>
            <div className="relative">
              <button type="button" aria-expanded={menu === "templates"} className={`rounded-full px-3 py-2 text-sm hover:bg-hover ${menu === "templates" ? "bg-hover" : ""}`} onClick={() => setMenu((open) => (open === "templates" ? null : "templates"))}>Templates</button>
              {menu === "templates" && (
                <div className="absolute bottom-11 left-0 z-20 w-64 rounded-xl border border-line bg-panel p-2 shadow-[var(--shadow)]">
                  {mail.templates.length === 0 && <p className="px-2 py-1 text-sm text-muted">No templates yet.</p>}
                  {mail.templates.map((template) => (
                    <div key={template.id} className="flex items-center gap-1">
                      <button
                        type="button"
                        className="min-w-0 flex-1 truncate rounded-lg px-2 py-1.5 text-left text-sm hover:bg-hover"
                        onClick={() => {
                          const signature = editor.current?.querySelector("[data-signature]")?.outerHTML ?? "";
                          mail.updateCompose(draft.localId, {
                            subject: draft.subject.trim() ? draft.subject : template.subject,
                            html: `${template.html}${signature ? `<p><br></p>${signature}` : ""}`,
                            editorRev: draft.editorRev + 1,
                          });
                          setMenu(null);
                        }}
                      >
                        {template.name}
                      </button>
                      <button type="button" className="px-2 text-xs text-muted" aria-label={`Delete ${template.name}`} onClick={() => void mail.deleteTemplate(template.id)}>Delete</button>
                    </div>
                  ))}
                  <div className="mt-2 flex gap-1 border-t border-line pt-2">
                    <input className="field" placeholder="Template name" value={templateName} onChange={(event) => setTemplateName(event.target.value)} />
                    <button
                      type="button"
                      className="rounded-lg px-2 text-sm font-medium"
                      onClick={() => {
                        syncHtml();
                        void mail.saveTemplate(templateName, draft.subject, editor.current?.innerHTML ?? draft.html);
                        setTemplateName("");
                      }}
                    >
                      Save
                    </button>
                  </div>
                </div>
              )}
            </div>
            <label className="cursor-pointer rounded-full px-3 py-2 text-sm hover:bg-hover">
              Attach
              <input
                type="file"
                className="hidden"
                multiple
                onChange={(event) => void addFiles(event.target.files, draft, mail.updateCompose)}
              />
            </label>
            <button type="button" className="text-sm text-muted" onClick={() => { syncHtml(); void mail.saveDraftCompose(draft.localId); }}>Save draft</button>
          </footer>
        </>
      )}
    </section>
  );

  function syncHtml() {
    if (editor.current) mail.updateCompose(draft.localId, { html: editor.current.innerHTML });
  }
}

function Field({
  label,
  value,
  onChange,
  inputRef,
  trailing,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  inputRef?: RefObject<HTMLInputElement | null>;
  trailing?: ReactNode;
}) {
  return (
    <div className="flex items-center gap-2 border-b border-line px-3 py-2 text-sm">
      <label className="flex min-w-0 flex-1 items-center gap-2">
        <span className="w-12 shrink-0 text-muted">{label}</span>
        <input ref={inputRef} className="w-full bg-transparent outline-none" value={value} onChange={(event) => onChange(event.target.value)} />
      </label>
      {trailing}
    </div>
  );
}

function RichEditor({
  editor,
  html,
  resetKey,
  onChange,
}: {
  editor: RefObject<HTMLDivElement | null>;
  html: string;
  resetKey: string;
  onChange: (html: string) => void;
}) {
  useEffect(() => {
    const node = editor.current;
    if (!node) return;
    node.innerHTML = html;
    const start = node.querySelector("p");
    if (!start) return;
    const range = document.createRange();
    range.selectNodeContents(start);
    range.collapse(true);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  }, [editor, resetKey]);
  return (
    <div
      ref={editor}
      className="min-h-40 max-h-72 overflow-y-auto px-3 py-2 outline-none"
      contentEditable
      role="textbox"
      aria-label="Message"
      onInput={() => onChange(editor.current?.innerHTML ?? "")}
    />
  );
}

const STATE_COMMANDS = ["bold", "italic", "underline", "strikeThrough", "insertUnorderedList", "insertOrderedList"];

function readActive(): Record<string, boolean> {
  return Object.fromEntries(STATE_COMMANDS.map((command) => [command, document.queryCommandState(command)]));
}

/** Formatting controls under the message body. Buttons light up for the format at the caret. */
function FormatBar({ editor }: { editor: RefObject<HTMLDivElement | null> }) {
  const [active, setActive] = useState<Record<string, boolean>>({});
  useEffect(() => {
    const update = () => {
      const node = editor.current;
      const anchor = window.getSelection()?.anchorNode;
      if (!node || !anchor || !node.contains(anchor)) return;
      setActive(readActive());
    };
    document.addEventListener("selectionchange", update);
    return () => document.removeEventListener("selectionchange", update);
  }, [editor]);

  const run = (command: string, value?: string) => {
    editor.current?.focus();
    document.execCommand(command, false, value);
    setActive(readActive());
    // execCommand fires input on the editor itself, which saves the HTML.
  };

  return (
    <div className="mx-3 mb-2 flex items-center gap-0.5 rounded-xl border border-line bg-bg p-1" role="toolbar" aria-label="Formatting">
      <FormatButton label="Bold" active={active.bold} onClick={() => run("bold")}><IconBold /></FormatButton>
      <FormatButton label="Italic" active={active.italic} onClick={() => run("italic")}><IconItalic /></FormatButton>
      <FormatButton label="Underline" active={active.underline} onClick={() => run("underline")}><IconUnderline /></FormatButton>
      <FormatButton label="Strikethrough" active={active.strikeThrough} onClick={() => run("strikeThrough")}><IconStrike /></FormatButton>
      <span className="mx-1 h-5 w-px bg-line" aria-hidden />
      <FormatButton label="Bulleted list" active={active.insertUnorderedList} onClick={() => run("insertUnorderedList")}><IconListBullet /></FormatButton>
      <FormatButton label="Numbered list" active={active.insertOrderedList} onClick={() => run("insertOrderedList")}><IconListNumber /></FormatButton>
      <span className="mx-1 h-5 w-px bg-line" aria-hidden />
      <FormatButton
        label="Insert link"
        onClick={() => {
          const url = window.prompt("Link address");
          if (url) run("createLink", url);
        }}
      >
        <IconLink />
      </FormatButton>
      <FormatButton label="Clear formatting" onClick={() => run("removeFormat")}><IconClearFormat /></FormatButton>
    </div>
  );
}

function FormatButton({ label, active = false, onClick, children }: { label: string; active?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      className={`grid h-8 w-8 place-items-center rounded-lg transition-colors ${active ? "bg-selected text-ink" : "text-muted hover:bg-hover hover:text-ink"}`}
      // Keep the caret in the message while clicking.
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

async function addFiles(list: FileList | null, draft: ComposeDraft, update: (id: string, patch: Partial<ComposeDraft>) => void) {
  if (!list) return;
  const files = await Promise.all([...list].map(async (file) => ({
    filename: file.name,
    mimeType: file.type || "application/octet-stream",
    dataBase64: await fileToBase64(file),
    size: file.size,
  })));
  update(draft.localId, { attachments: [...draft.attachments, ...files] });
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const value = String(reader.result || "");
      resolve(value.slice(value.indexOf(",") + 1));
    };
    reader.onerror = () => reject(new Error("Could not read that file."));
    reader.readAsDataURL(file);
  });
}
