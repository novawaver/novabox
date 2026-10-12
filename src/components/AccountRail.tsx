import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import { initials } from "../format";
import { Overlay } from "./Dialogs";
import { IconPlus, IconX } from "./Icons";
import { useMail } from "../state/MailProvider";
import type { Account } from "../types";

type MenuState = { account: Account; x: number; y: number; keyboard: boolean };

export function AccountRail() {
  const mail = useMail();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [confirmSignIn, setConfirmSignIn] = useState(false);
  const total = mail.accounts.reduce((sum, account) => sum + account.unread, 0);
  return (
    <aside className="flex w-[72px] shrink-0 flex-col items-center gap-2 border-r border-line bg-rail py-3">
      <RailButton
        label="All inboxes"
        active={mail.query.accountId === null}
        color="#241c14"
        text="All"
        badge={total}
        onClick={() => mail.selectAccount(null)}
      />
      <div className="my-1 h-px w-8 bg-line" />
      {mail.accounts.map((account) => (
        <RailButton
          key={account.id}
          label={account.email}
          active={mail.query.accountId === account.id}
          color={account.color}
          text={initials(account.name, account.email)}
          badge={account.unread}
          onClick={() => mail.selectAccount(account.id)}
          onMenu={(x, y, keyboard) => setMenu({ account, x, y, keyboard })}
        />
      ))}
      <button
        type="button"
        title={mail.signInBusy ? "Cancel Google sign-in" : "Add Gmail account"}
        aria-label={mail.signInBusy ? "Cancel Google sign-in" : "Add Gmail account"}
        onClick={() => (mail.signInBusy ? mail.cancelSignIn() : setConfirmSignIn(true))}
        className={`group relative mt-auto grid h-11 w-11 shrink-0 place-items-center rounded-full border-2 transition-all duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-rail ${
          mail.signInBusy
            ? "border-danger/60 bg-panel text-danger hover:bg-danger hover:text-white"
            : "border-dashed border-accent bg-accent/10 text-accent hover:scale-105 hover:border-solid hover:bg-accent hover:text-accent-ink hover:shadow-[0_4px_14px_color-mix(in_srgb,var(--accent)_45%,transparent)]"
        }`}
      >
        {mail.signInBusy ? (
          <>
            <span className="absolute inset-[-4px] animate-spin rounded-full border-2 border-transparent border-t-accent" aria-hidden />
            <IconX className="h-4 w-4" />
          </>
        ) : (
          <IconPlus className="h-5 w-5 transition-transform duration-150 group-hover:rotate-90" />
        )}
      </button>
      {menu && <AccountMenu {...menu} onClose={() => setMenu(null)} />}
      {confirmSignIn && (
        <Overlay title="Add Gmail account" onClose={() => setConfirmSignIn(false)}>
          <p className="text-sm text-muted">Novabox will open Google sign-in in your browser. Come back to this window when you're done.</p>
          <div className="mt-5 flex justify-end gap-2">
            <button type="button" className="rounded-full px-4 py-2" onClick={() => setConfirmSignIn(false)}>Cancel</button>
            <button
              type="button"
              autoFocus
              className="rounded-full bg-accent px-4 py-2 font-semibold text-accent-ink"
              onClick={() => {
                setConfirmSignIn(false);
                void mail.signIn();
              }}
            >
              Continue to browser
            </button>
          </div>
        </Overlay>
      )}
    </aside>
  );
}

function RailButton({
  label,
  active,
  color,
  text,
  badge,
  onClick,
  onMenu,
}: {
  label: string;
  active: boolean;
  color: string;
  text: string;
  badge: number;
  onClick: () => void;
  onMenu?: (x: number, y: number, keyboard: boolean) => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-haspopup={onMenu ? "menu" : undefined}
      onClick={onClick}
      onContextMenu={(event: MouseEvent<HTMLButtonElement>) => {
        event.preventDefault();
        if (!onMenu) return;
        // Keyboard-opened menus (Shift+F10 / Menu key) report 0,0; anchor to the avatar instead.
        if (event.clientX === 0 && event.clientY === 0) {
          const rect = event.currentTarget.getBoundingClientRect();
          onMenu(rect.right + 4, rect.top, true);
        } else {
          onMenu(event.clientX, event.clientY, false);
        }
      }}
      className={`relative grid h-11 w-11 place-items-center rounded-full text-xs font-semibold text-white ${active ? "ring-2 ring-accent ring-offset-2 ring-offset-rail" : ""}`}
      style={{ background: color }}
    >
      {text.slice(0, 3)}
      {badge > 0 && (
        <span className="absolute -right-1 -top-1 grid min-w-5 place-items-center rounded-full bg-accent px-1 text-[10px] font-bold text-accent-ink">
          {badge > 99 ? "99+" : badge}
        </span>
      )}
    </button>
  );
}

function AccountMenu({ account, x, y, keyboard, onClose }: MenuState & { onClose: () => void }) {
  const mail = useMail();
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });

  // Keep the menu inside the window when opened near the bottom edge.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPos({
      left: Math.max(4, Math.min(x, window.innerWidth - width - 4)),
      top: Math.max(4, Math.min(y, window.innerHeight - height - 4)),
    });
    // Only pre-select the first item for keyboard users; mouse users start with nothing highlighted.
    if (keyboard) el.querySelector<HTMLButtonElement>("button")?.focus();
    else el.focus();
  }, [x, y, keyboard]);

  useEffect(() => {
    const onDown = (event: globalThis.MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
        return;
      }
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
      event.preventDefault();
      const items = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>("button") ?? []);
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "ArrowDown" ? index + 1 : index < 0 ? items.length - 1 : index - 1;
      items[(next + items.length) % items.length]?.focus();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onClose);
    window.addEventListener("resize", onClose);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("resize", onClose);
    };
  }, [onClose]);

  const run = (action: () => void) => () => {
    onClose();
    action();
  };

  return (
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      aria-label={`${account.email} options`}
      className="fixed z-50 w-60 rounded-xl border border-line bg-panel p-2 shadow-[var(--shadow)] outline-none"
      style={pos}
      onContextMenu={(event) => event.preventDefault()}
    >
      <div className="px-2 py-1">
        {account.name && <p className="truncate text-sm font-semibold">{account.name}</p>}
        <p className="truncate text-xs text-muted">{account.email}</p>
      </div>
      <div className="my-1 h-px bg-line" />
      <MenuItem onClick={run(() => mail.selectAccount(account.id))}>Open inbox</MenuItem>
      <MenuItem onClick={run(() => mail.composeNew(account.id))}>Compose from this account</MenuItem>
      <MenuItem onClick={run(() => void mail.refresh())}>Sync now</MenuItem>
      <MenuItem onClick={run(() => void navigator.clipboard?.writeText(account.email).catch(() => {}))}>Copy email address</MenuItem>
      <MenuItem onClick={run(() => mail.setSettingsOpen(true))}>Settings</MenuItem>
      <div className="my-1 h-px bg-line" />
      <MenuItem onClick={run(() => void mail.removeAccount(account.id, false))}>Remove from this PC…</MenuItem>
      <MenuItem danger onClick={run(() => void mail.removeAccount(account.id, true))}>
        Sign out…
      </MenuItem>
    </div>
  );
}

function MenuItem({ children, onClick, danger = false }: { children: string; onClick: () => void; danger?: boolean }) {
  return (
    <button
      type="button"
      role="menuitem"
      className={`block w-full rounded-lg px-2 py-1.5 text-left text-sm hover:bg-hover focus:outline-none focus-visible:bg-hover ${danger ? "text-danger" : ""}`}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
