import { useEffect, useRef, useState, type ReactNode } from "react";
import { useMail } from "../state/MailProvider";
import { defaultInboxTabs, type Settings } from "../types";
import { BackupDialog } from "./BackupDialog";

const SETTINGS_TABS = [
  { id: "appearance", label: "Appearance" },
  { id: "inbox", label: "Inbox" },
  { id: "notifications", label: "Notifications" },
  { id: "accounts", label: "Accounts" },
  { id: "privacy", label: "Privacy" },
] as const;
type SettingsTab = (typeof SETTINGS_TABS)[number]["id"];

export function SettingsDialog() {
  const mail = useMail();
  // Kept while the app runs, so Settings reopens on the tab you last used.
  const [tab, setTab] = useState<SettingsTab>("appearance");
  const [draft, setDraft] = useState<Settings>(mail.settings);
  const [signatures, setSignatures] = useState<Record<string, string>>({});
  const [backup, setBackup] = useState<"export" | "import" | null>(null);
  const latest = useRef({ settings: mail.settings, accounts: mail.accounts });
  latest.current = { settings: mail.settings, accounts: mail.accounts };
  useEffect(() => {
    if (!mail.settingsOpen) return;
    const { settings, accounts } = latest.current;
    setDraft({ ...settings, inboxTabs: { ...defaultInboxTabs, ...settings.inboxTabs } });
    setSignatures(Object.fromEntries(accounts.map((account) => [account.id, account.signature])));
  }, [mail.settingsOpen]);
  useEffect(() => {
    if (!mail.settingsOpen) return;
    setSignatures((current) => {
      const missing = mail.accounts.filter((account) => !(account.id in current));
      return missing.length === 0 ? current : { ...current, ...Object.fromEntries(missing.map((account) => [account.id, account.signature])) };
    });
  }, [mail.settingsOpen, mail.accounts]);
  if (!mail.settingsOpen) return null;
  return (
    <Overlay title="Settings" wide onClose={() => mail.setSettingsOpen(false)}>
      <div className="flex min-h-0 flex-1 gap-5">
        <div role="tablist" aria-label="Settings sections" aria-orientation="vertical" className="flex w-40 shrink-0 flex-col gap-1 border-r border-line pr-4">
          {SETTINGS_TABS.map((item) => (
            <button
              key={item.id}
              type="button"
              role="tab"
              id={`settings-tab-${item.id}`}
              aria-selected={tab === item.id}
              aria-controls="settings-panel"
              className={`rounded-xl px-3 py-2 text-left text-sm ${tab === item.id ? "bg-selected font-semibold text-ink" : "text-muted hover:bg-hover hover:text-ink"}`}
              onClick={() => setTab(item.id)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <div role="tabpanel" id="settings-panel" aria-labelledby={`settings-tab-${tab}`} className="min-h-0 flex-1 overflow-y-auto pr-1">
          {tab === "appearance" && (
            <Section title="Appearance">
              <Select label="Theme" value={draft.theme} options={[["system", "Match Windows"], ["light", "Light"], ["dark", "Dark"]]} onChange={(theme) => setDraft({ ...draft, theme })} />
              <Select label="Density" value={draft.density} options={[["comfortable", "Comfortable"], ["default", "Default"], ["compact", "Compact"]]} onChange={(density) => setDraft({ ...draft, density })} />
              <Select label="Reading pane" value={draft.readingPane} options={[["none", "Full page"], ["right", "Right"], ["bottom", "Below"]]} onChange={(readingPane) => setDraft({ ...draft, readingPane })} />
              <label className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={draft.startWithWindows} onChange={(event) => setDraft({ ...draft, startWithWindows: event.target.checked })} />
                Start Novabox when Windows starts
              </label>
            </Section>
          )}
          {tab === "notifications" && (
            <Section title="Notifications">
              <TabCheck label="Show notifications inside Novabox" checked={draft.notifyInApp} onChange={(notifyInApp) => setDraft({ ...draft, notifyInApp })} />
              <TabCheck label="Show desktop notifications" checked={draft.notifyDesktop} onChange={(notifyDesktop) => setDraft({ ...draft, notifyDesktop })} />
              {draft.notifyDesktop && (
                <div className="ml-6 space-y-2">
                  <Select
                    label="Style"
                    value={draft.notifyStyle || "windows"}
                    options={[["windows", "Windows notifications"], ["novabox", "Novabox pop-up"]]}
                    onChange={(notifyStyle) => setDraft({ ...draft, notifyStyle })}
                  />
                  {draft.notifyStyle === "novabox" && (
                    <>
                      <Select
                        label="Screen corner"
                        value={draft.popupCorner || "bottomRight"}
                        options={[["bottomRight", "Bottom right"], ["bottomLeft", "Bottom left"], ["topRight", "Top right"], ["topLeft", "Top left"]]}
                        onChange={(popupCorner) => setDraft({ ...draft, popupCorner })}
                      />
                      <Select
                        label="Show at most"
                        value={String(draft.popupMax || 3)}
                        options={[["1", "1"], ["2", "2"], ["3", "3"], ["4", "4"], ["5", "5"]]}
                        onChange={(popupMax) => setDraft({ ...draft, popupMax: Number(popupMax) })}
                      />
                      <TabCheck label="Show sender and subject" checked={draft.popupPreview} onChange={(popupPreview) => setDraft({ ...draft, popupPreview })} />
                    </>
                  )}
                </div>
              )}
              <label className={`ml-6 flex items-center gap-2 text-sm ${draft.notifyDesktop ? "" : "text-muted"}`}>
                <input
                  type="checkbox"
                  disabled={!draft.notifyDesktop}
                  checked={draft.notifyDesktopBackgroundOnly}
                  onChange={(event) => setDraft({ ...draft, notifyDesktopBackgroundOnly: event.target.checked })}
                />
                Only when Novabox isn't the active window
              </label>
              <p className="text-sm leading-6 text-muted">
                {draft.notifyInApp && draft.notifyDesktop && draft.notifyDesktopBackgroundOnly
                  ? "While you're in Novabox, new mail shows inside the app. When it's minimized, in the tray, or behind another window, a desktop notification shows instead."
                  : !draft.notifyInApp && !draft.notifyDesktop
                    ? "New mail won't notify you. The unread badge still updates."
                    : !draft.notifyInApp && draft.notifyDesktopBackgroundOnly
                      ? "While you're in Novabox, new mail won't notify you. Desktop notifications show only when Novabox is in the background."
                      : "Clicking an in-app notification opens the message."}
              </p>
              <button
                type="button"
                className="rounded-full border border-line px-3 py-1.5 text-sm hover:bg-hover disabled:opacity-50"
                disabled={!draft.notifyInApp && !draft.notifyDesktop}
                onClick={() => mail.testNotify(draft)}
              >
                Send test notification
              </button>
            </Section>
          )}
          {tab === "inbox" && (
            <Section title="Inbox tabs">
              <p className="text-sm leading-6 text-muted">
                Gmail keeps a category in Primary until that tab is turned on. Promotions and Social start on. Updates stays in Primary until you turn it on here.
              </p>
              <TabCheck label="Promotions" checked={draft.inboxTabs.promotions} onChange={(promotions) => setDraft({ ...draft, inboxTabs: { ...draft.inboxTabs, promotions } })} />
              <TabCheck label="Social" checked={draft.inboxTabs.social} onChange={(social) => setDraft({ ...draft, inboxTabs: { ...draft.inboxTabs, social } })} />
              <TabCheck label="Updates" checked={draft.inboxTabs.updates} onChange={(updates) => setDraft({ ...draft, inboxTabs: { ...draft.inboxTabs, updates } })} />
            </Section>
          )}
          {tab === "accounts" && (
            <>
              <Section title="Accounts">
                <p className="text-sm leading-6 text-muted">
                  Back up your Google sign-ins to an encrypted file, then import it on another PC to use the same accounts there without signing in again.
                </p>
                <div className="flex flex-wrap gap-2">
                  <button type="button" className="rounded-full border border-line px-3 py-1.5 text-sm hover:bg-hover disabled:opacity-50" disabled={mail.accounts.length === 0 || mail.mode !== "live"} onClick={() => setBackup("export")}>
                    Export sign-ins…
                  </button>
                  <button type="button" className="rounded-full border border-line px-3 py-1.5 text-sm hover:bg-hover" onClick={() => setBackup("import")}>
                    Import sign-ins…
                  </button>
                </div>
              </Section>
              {mail.accounts.length > 0 && (
                <Section title="Signatures">
                  {mail.accounts.map((account) => (
                    <label key={account.id} className="block text-sm">
                      <span className="mb-1 block text-muted">{account.email}</span>
                      <textarea className="field min-h-20" value={signatures[account.id] ?? ""} onChange={(event) => setSignatures({ ...signatures, [account.id]: event.target.value })} />
                    </label>
                  ))}
                </Section>
              )}
            </>
          )}
          {tab === "privacy" && (
            <Section title="Privacy">
              <p className="text-sm leading-6 text-muted">
                Novabox uses Gmail data only to show and send your mail. It does not sell it, advertise with it, or use it to train models. Remote images in messages stay blocked until you show them. Closing the window leaves Novabox in the tray so new mail can arrive. Quit from the tray icon.
              </p>
            </Section>
          )}
        </div>
      </div>
      {backup && <BackupDialog mode={backup} onClose={() => setBackup(null)} />}
      <div className="mt-4 flex shrink-0 justify-end gap-2 border-t border-line pt-4">
        <button type="button" className="rounded-full px-4 py-2" onClick={() => mail.setSettingsOpen(false)}>Cancel</button>
        <button
          type="button"
          className="rounded-full bg-accent px-4 py-2 font-semibold text-accent-ink"
          onClick={() => {
            void mail.saveSettings(draft);
            for (const [accountId, signature] of Object.entries(signatures)) {
              const current = mail.accounts.find((account) => account.id === accountId)?.signature ?? "";
              if (signature !== current) void mail.setSignature(accountId, signature);
            }
            mail.setSettingsOpen(false);
          }}
        >
          Save
        </button>
      </div>
    </Overlay>
  );
}

const openOverlays: symbol[] = [];

export function Overlay({ title, onClose, children, wide = false }: { title: string; onClose: () => void; children: ReactNode; wide?: boolean }) {
  // Escape closes the dialog, as with any modal; only the topmost one when they stack.
  const [layer] = useState(() => Symbol("overlay"));
  useEffect(() => {
    openOverlays.push(layer);
    return () => {
      openOverlays.splice(openOverlays.indexOf(layer), 1);
    };
  }, [layer]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && openOverlays[openOverlays.length - 1] === layer) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [layer, onClose]);
  return (
    <div className="fixed inset-0 z-40 grid place-items-center bg-black/40 p-4" onMouseDown={onClose}>
      <div
        className={`w-full rounded-2xl bg-panel p-5 shadow-[var(--shadow)] ${wide ? "flex h-[min(620px,85vh)] max-w-3xl flex-col" : "max-h-[85vh] max-w-lg overflow-y-auto"}`}
        role="dialog"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="mb-4 flex shrink-0 items-center justify-between">
          <h2 className="text-xl font-semibold">{title}</h2>
          <button type="button" className="icon-btn" onClick={onClose} aria-label="Close">×</button>
        </div>
        {children}
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mt-6 space-y-2 first:mt-0">
      <h3 className="text-sm font-semibold uppercase tracking-wide text-muted">{title}</h3>
      {children}
    </section>
  );
}

function TabCheck({ label, checked, onChange }: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <label className="flex items-center gap-2 text-sm">
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      {label}
    </label>
  );
}

/** A row of option buttons (a radio group) in place of a dropdown. Arrow keys move the choice. */
function Select({ label, value, options, onChange }: { label: string; value: string; options: string[][]; onChange: (value: string) => void }) {
  const move = (step: number) => {
    const index = options.findIndex(([id]) => id === value);
    const next = options[(index + step + options.length) % options.length];
    if (next) onChange(next[0]);
  };
  return (
    <div className="space-y-1.5 text-sm">
      <span className="block font-medium">{label}</span>
      <div
        role="radiogroup"
        aria-label={label}
        className="inline-flex max-w-full flex-wrap gap-1 rounded-xl border border-line bg-bg p-1"
        onKeyDown={(event) => {
          if (event.key === "ArrowRight" || event.key === "ArrowDown") {
            event.preventDefault();
            move(1);
          } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
            event.preventDefault();
            move(-1);
          }
        }}
      >
        {options.map(([id, name]) => {
          const selected = id === value;
          return (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={selected}
              // Only the chosen option is a tab stop, like native radio buttons.
              tabIndex={selected ? 0 : -1}
              ref={(node) => {
                if (selected && node && node.parentElement?.contains(document.activeElement)) node.focus();
              }}
              className={`rounded-lg px-3 py-1.5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                selected ? "bg-panel font-semibold text-ink shadow-sm ring-1 ring-line" : "text-muted hover:bg-hover hover:text-ink"
              }`}
              onClick={() => onChange(id)}
            >
              {name}
            </button>
          );
        })}
      </div>
    </div>
  );
}
