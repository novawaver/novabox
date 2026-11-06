import { useEffect, useLayoutEffect, useRef, useState, type DragEvent, type HTMLAttributes, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { newId } from "../demo";
import { initials } from "../format";
import { addGroup, applyDrop, canStep, dropSlot, moveAccount, moveStep, normalizeRail, patchGroup, railEntries, railWidth, type RailItem, type RailSlot, type RailTarget } from "../rail";
import { Overlay } from "./Dialogs";
import { IconBack, IconNext, IconPlus, IconX } from "./Icons";
import { useMail } from "../state/MailProvider";
import type { Account, AccountGroup } from "../types";

const GROUP_COLOR = "#6b5b47";

type MenuPlace = { x: number; y: number; keyboard: boolean };
type MenuState = MenuPlace & ({ kind: "account"; account: Account } | { kind: "group"; group: AccountGroup });
type Naming = { kind: "new"; accountId: string } | { kind: "rename"; group: AccountGroup };
type DropEdge = "before" | "after" | "inside";
type DropState = { rowId: string; edge: DropEdge; slot: RailSlot };

export function AccountRail() {
  const mail = useMail();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [confirmSignIn, setConfirmSignIn] = useState(false);
  const [naming, setNaming] = useState<Naming | null>(null);
  const [drag, setDrag] = useState<RailItem | null>(null);
  const [drop, setDrop] = useState<DropState | null>(null);
  const wide = mail.settings.showAccountEmails;
  const layout = normalizeRail(mail.accounts, mail.settings);
  const entries = railEntries(mail.accounts, layout);
  const total = mail.accounts.reduce((sum, account) => sum + account.unread, 0);

  const endDrag = () => {
    setDrag(null);
    setDrop(null);
  };
  const hover = (rowId: string, edge: DropEdge, slot: RailSlot) => {
    setDrop((current) => (current?.rowId === rowId && current.edge === edge ? current : { rowId, edge, slot }));
  };
  const finishDrop = (event: DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
    if (drag && drop) {
      const { slot } = drop;
      const item = drag;
      mail.updateRail((current) => applyDrop(current, item, slot));
    }
    endDrag();
  };
  // Drop handling for one row. A group can't sit inside a group, so its rows ignore a dragged group.
  const rowDrag = (item: RailItem, target: RailTarget): HTMLAttributes<HTMLDivElement> => ({
    draggable: true,
    onDragStart: (event) => {
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", item.id);
      setDrag(item);
    },
    onDragEnd: endDrag,
    onDragOver: (event) => {
      if (!drag || (drag.kind === "group" && target.kind === "account" && target.groupId)) return;
      event.preventDefault();
      event.stopPropagation();
      event.dataTransfer.dropEffect = "move";
      const box = event.currentTarget.getBoundingClientRect();
      const lower = event.clientY > box.top + box.height / 2;
      const slot = dropSlot(layout, drag, target, lower);
      hover(target.id, target.kind === "group" && slot.groupId === target.id ? "inside" : lower ? "after" : "before", slot);
    },
    onDrop: finishDrop,
  });
  const accountRow = (account: Account, groupId: string | null) => (
    <RailRow
      key={account.id}
      wide={wide}
      dragging={drag?.id === account.id}
      edge={drop?.rowId === account.id ? drop.edge : null}
      {...rowDrag({ kind: "account", id: account.id }, { kind: "account", id: account.id, groupId })}
    >
      <RailButton
        label={account.email}
        wide={wide}
        active={mail.query.accountId === account.id}
        color={account.color}
        text={initials(account.name, account.email)}
        badge={account.unread}
        onClick={() => mail.selectAccount(account.id)}
        onMenu={(x, y, keyboard) => setMenu({ kind: "account", account, x, y, keyboard })}
        onStep={(step) => mail.updateRail((current) => moveStep(current, account.id, step))}
      />
    </RailRow>
  );

  return (
    <aside className="flex shrink-0 flex-col border-r border-line bg-rail py-3 transition-[width] duration-150" style={{ width: railWidth(wide) }}>
      <RailRow wide={wide}>
        <RailButton
          label="All inboxes"
          wide={wide}
          active={mail.query.accountId === null && mail.query.groupId === null}
          color="#241c14"
          text="All"
          badge={total}
          onClick={() => mail.selectAccount(null)}
        />
      </RailRow>
      <div className="mx-auto my-1 h-px w-8 shrink-0 bg-line" />
      <div
        className="flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden [scrollbar-width:none]"
        onDragLeave={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDrop(null);
        }}
      >
        {entries.map((entry) => {
          if (entry.kind === "account") return accountRow(entry.account, null);
          const { group, accounts } = entry;
          return (
            <div key={group.id} className="shrink-0 py-1">
              <div className="mx-2 rounded-3xl border border-line py-0.5">
                <RailRow
                  wide={wide}
                  dragging={drag?.id === group.id}
                  edge={drop?.rowId === group.id ? drop.edge : null}
                  {...rowDrag({ kind: "group", id: group.id }, { kind: "group", id: group.id, collapsed: group.collapsed })}
                >
                  <RailButton
                    label={group.name}
                    wide={wide}
                    square
                    active={mail.query.groupId === group.id}
                    color={GROUP_COLOR}
                    text={initials(group.name, group.name)}
                    badge={accounts.reduce((sum, account) => sum + account.unread, 0)}
                    onClick={() => mail.selectGroup(group.id)}
                    onMenu={(x, y, keyboard) => setMenu({ kind: "group", group, x, y, keyboard })}
                    onStep={(step) => mail.updateRail((current) => moveStep(current, group.id, step))}
                  />
                  <button
                    type="button"
                    title={group.collapsed ? "Show accounts" : "Hide accounts"}
                    aria-label={group.collapsed ? `Show accounts in ${group.name}` : `Hide accounts in ${group.name}`}
                    aria-expanded={!group.collapsed}
                    onClick={() => mail.updateRail((current) => patchGroup(current, group.id, { collapsed: !group.collapsed }))}
                    className={`absolute z-10 grid h-4 w-4 place-items-center rounded-full border border-line bg-panel text-muted hover:text-ink ${wide ? "right-2 top-1/2 -translate-y-1/2" : "bottom-0.5 left-0.5"}`}
                  >
                    <span className={`scale-75 transition-transform ${group.collapsed ? "" : "rotate-90"}`}><IconNext /></span>
                  </button>
                </RailRow>
                {!group.collapsed && accounts.map((account) => accountRow(account, group.id))}
              </div>
            </div>
          );
        })}
        {/* The space below the last row: dropping here puts an item at the end, outside any group. */}
        <div
          className="min-h-6 flex-1"
          onDragOver={(event) => {
            if (!drag) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = "move";
            hover("end", "before", { groupId: null, before: null });
          }}
          onDrop={finishDrop}
        >
          {drop?.rowId === "end" && <div className="mx-2 h-0.5 rounded bg-accent" />}
        </div>
      </div>
      <div className="mt-2 flex shrink-0 flex-col items-center gap-2">
        <button
          type="button"
          className="icon-btn"
          title={wide ? "Hide email addresses" : "Show email addresses"}
          aria-label={wide ? "Hide email addresses" : "Show email addresses"}
          aria-pressed={wide}
          onClick={() => mail.setShowAccountEmails(!wide)}
        >
          {wide ? <IconBack /> : <IconNext />}
        </button>
        <button
          type="button"
          title={mail.signInBusy ? "Cancel Google sign-in" : "Add Gmail account"}
          aria-label={mail.signInBusy ? "Cancel Google sign-in" : "Add Gmail account"}
          onClick={() => (mail.signInBusy ? mail.cancelSignIn() : setConfirmSignIn(true))}
          className={`group relative grid h-11 w-11 shrink-0 place-items-center rounded-full border-2 transition-all duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-rail ${
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
      </div>
      {menu?.kind === "account" && (
        <AccountMenu
          {...menu}
          onClose={() => setMenu(null)}
          onNewGroup={(accountId) => setNaming({ kind: "new", accountId })}
        />
      )}
      {menu?.kind === "group" && (
        <GroupMenu
          {...menu}
          onClose={() => setMenu(null)}
          onRename={(group) => setNaming({ kind: "rename", group })}
        />
      )}
      {naming && (
        <GroupNameDialog
          title={naming.kind === "new" ? "New group" : "Rename group"}
          initial={naming.kind === "new" ? "" : naming.group.name}
          onClose={() => setNaming(null)}
          onSave={(name) => {
            setNaming(null);
            if (naming.kind === "new") {
              const group = { id: newId("group"), name, accountIds: [], collapsed: false };
              mail.updateRail((current) => addGroup(current, group, naming.accountId));
            } else {
              mail.updateRail((current) => patchGroup(current, naming.group.id, { name }));
            }
          }}
        />
      )}
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

function RailRow({
  wide,
  dragging = false,
  edge = null,
  children,
  ...rest
}: {
  wide: boolean;
  dragging?: boolean;
  edge?: DropEdge | null;
  children: ReactNode;
} & HTMLAttributes<HTMLDivElement>) {
  return (
    <div {...rest} className={`relative shrink-0 py-1 ${wide ? "px-3" : "flex justify-center"} ${dragging ? "opacity-40" : ""}`}>
      {edge === "before" && <span className="absolute inset-x-2 top-0 h-0.5 rounded bg-accent" aria-hidden />}
      {edge === "after" && <span className="absolute inset-x-2 bottom-0 h-0.5 rounded bg-accent" aria-hidden />}
      {edge === "inside" && <span className="pointer-events-none absolute inset-0 rounded-3xl ring-2 ring-inset ring-accent" aria-hidden />}
      {children}
    </div>
  );
}

function RailButton({
  label,
  wide,
  square = false,
  active,
  color,
  text,
  badge,
  onClick,
  onMenu,
  onStep,
}: {
  label: string;
  wide: boolean;
  square?: boolean;
  active: boolean;
  color: string;
  text: string;
  badge: number;
  onClick: () => void;
  onMenu?: (x: number, y: number, keyboard: boolean) => void;
  onStep?: (step: -1 | 1) => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-haspopup={onMenu ? "menu" : undefined}
      onClick={onClick}
      onKeyDown={(event: ReactKeyboardEvent<HTMLButtonElement>) => {
        if (!onStep || !event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
        event.preventDefault();
        onStep(event.key === "ArrowUp" ? -1 : 1);
      }}
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
      className={`flex items-center gap-3 rounded-3xl text-left ${wide ? `w-full ${active ? "bg-selected" : "hover:bg-hover"}` : ""}`}
    >
      <span
        className={`relative grid h-11 w-11 shrink-0 place-items-center text-xs font-semibold text-white ${square ? "rounded-2xl" : "rounded-full"} ${active ? "ring-2 ring-accent ring-offset-2 ring-offset-rail" : ""}`}
        style={{ background: color }}
      >
        {text.slice(0, 3)}
        {badge > 0 && (
          <span className="absolute -right-1 -top-1 grid min-w-5 place-items-center rounded-full bg-accent px-1 text-[10px] font-bold text-accent-ink">
            {badge > 99 ? "99+" : badge}
          </span>
        )}
      </span>
      {/* Wraps instead of truncating, so a long address is always shown in full. */}
      {wide && <span className={`min-w-0 break-all py-1 text-xs leading-snug text-ink ${square ? "pr-6 font-semibold" : "pr-2"}`}>{label}</span>}
    </button>
  );
}

function GroupNameDialog({ title, initial, onSave, onClose }: { title: string; initial: string; onSave: (name: string) => void; onClose: () => void }) {
  const [name, setName] = useState(initial);
  const clean = name.trim();
  return (
    <Overlay title={title} onClose={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (clean) onSave(clean);
        }}
      >
        <input className="field" autoFocus maxLength={40} placeholder="Group name" aria-label="Group name" value={name} onChange={(event) => setName(event.target.value)} />
        <div className="mt-5 flex justify-end gap-2">
          <button type="button" className="rounded-full px-4 py-2" onClick={onClose}>Cancel</button>
          <button type="submit" disabled={!clean} className="rounded-full bg-accent px-4 py-2 font-semibold text-accent-ink disabled:opacity-50">Save</button>
        </div>
      </form>
    </Overlay>
  );
}

function AccountMenu({ account, x, y, keyboard, onClose, onNewGroup }: MenuPlace & { account: Account; onClose: () => void; onNewGroup: (accountId: string) => void }) {
  const mail = useMail();
  const layout = normalizeRail(mail.accounts, mail.settings);
  const current = layout.accountGroups.find((group) => group.accountIds.includes(account.id));
  const others = layout.accountGroups.filter((group) => group.id !== current?.id);
  const run = (action: () => void) => () => {
    onClose();
    action();
  };
  return (
    <RailMenu x={x} y={y} keyboard={keyboard} label={`${account.email} options`} onClose={onClose}>
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
      <MenuItem disabled={!canStep(layout, account.id, -1)} onClick={run(() => mail.updateRail((rail) => moveStep(rail, account.id, -1)))}>Move up</MenuItem>
      <MenuItem disabled={!canStep(layout, account.id, 1)} onClick={run(() => mail.updateRail((rail) => moveStep(rail, account.id, 1)))}>Move down</MenuItem>
      {others.map((group) => (
        <MenuItem key={group.id} onClick={run(() => mail.updateRail((rail) => moveAccount(rail, account.id, { groupId: group.id, before: null })))}>{`Move to ${group.name}`}</MenuItem>
      ))}
      {current && (
        <MenuItem onClick={run(() => mail.updateRail((rail) => moveAccount(rail, account.id, { groupId: null, before: null })))}>{`Remove from ${current.name}`}</MenuItem>
      )}
      <MenuItem onClick={run(() => onNewGroup(account.id))}>New group…</MenuItem>
      <div className="my-1 h-px bg-line" />
      <MenuItem onClick={run(() => void mail.removeAccount(account.id, false))}>Remove from this PC…</MenuItem>
      <MenuItem danger onClick={run(() => void mail.removeAccount(account.id, true))}>
        Sign out…
      </MenuItem>
    </RailMenu>
  );
}

function GroupMenu({ group, x, y, keyboard, onClose, onRename }: MenuPlace & { group: AccountGroup; onClose: () => void; onRename: (group: AccountGroup) => void }) {
  const mail = useMail();
  const layout = normalizeRail(mail.accounts, mail.settings);
  const run = (action: () => void) => () => {
    onClose();
    action();
  };
  return (
    <RailMenu x={x} y={y} keyboard={keyboard} label={`${group.name} options`} onClose={onClose}>
      <div className="px-2 py-1">
        <p className="truncate text-sm font-semibold">{group.name}</p>
      </div>
      <div className="my-1 h-px bg-line" />
      <MenuItem onClick={run(() => mail.selectGroup(group.id))}>Open combined inbox</MenuItem>
      <MenuItem onClick={run(() => mail.updateRail((rail) => patchGroup(rail, group.id, { collapsed: !group.collapsed })))}>{group.collapsed ? "Show accounts" : "Hide accounts"}</MenuItem>
      <MenuItem onClick={run(() => onRename(group))}>Rename…</MenuItem>
      <div className="my-1 h-px bg-line" />
      <MenuItem disabled={!canStep(layout, group.id, -1)} onClick={run(() => mail.updateRail((rail) => moveStep(rail, group.id, -1)))}>Move up</MenuItem>
      <MenuItem disabled={!canStep(layout, group.id, 1)} onClick={run(() => mail.updateRail((rail) => moveStep(rail, group.id, 1)))}>Move down</MenuItem>
      <div className="my-1 h-px bg-line" />
      <MenuItem onClick={run(() => mail.removeGroup(group.id))}>Ungroup</MenuItem>
    </RailMenu>
  );
}

function RailMenu({ x, y, keyboard, label, onClose, children }: MenuPlace & { label: string; onClose: () => void; children: ReactNode }) {
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
    if (keyboard) el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
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
      const items = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
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

  return (
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      aria-label={label}
      className="fixed z-50 max-h-[calc(100vh-8px)] w-60 overflow-y-auto rounded-xl border border-line bg-panel p-2 shadow-[var(--shadow)] outline-none"
      style={pos}
      onContextMenu={(event) => event.preventDefault()}
    >
      {children}
    </div>
  );
}

function MenuItem({ children, onClick, danger = false, disabled = false }: { children: string; onClick: () => void; danger?: boolean; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      className={`block w-full truncate rounded-lg px-2 py-1.5 text-left text-sm hover:bg-hover focus:outline-none focus-visible:bg-hover disabled:cursor-default disabled:bg-transparent disabled:text-muted disabled:opacity-60 ${danger ? "text-danger" : ""}`}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
