import type { Account, AccountGroup, Settings } from "./types";

/**
 * The order of the account bar. `railOrder` lists what sits at the top level, accounts and
 * groups side by side; the accounts inside a group are listed by that group alone.
 */
export type RailLayout = Pick<Settings, "railOrder" | "accountGroups">;
export type RailEntry = { kind: "account"; account: Account } | { kind: "group"; group: AccountGroup; accounts: Account[] };
/** What is being moved. */
export type RailItem = { kind: "account" | "group"; id: string };
/** The row a dragged item is held over. */
export type RailTarget = { kind: "account"; id: string; groupId: string | null } | { kind: "group"; id: string; collapsed: boolean };
/** A place in the bar: inside a group or at the top level, ahead of `before` or at the end. */
export type RailSlot = { groupId: string | null; before: string | null };

export function railWidth(showEmails: boolean): number {
  return showEmails ? 264 : 72;
}

/** Drops ids that no longer exist and adds accounts and groups the saved order hasn't met yet. */
export function normalizeRail(accounts: Account[], layout: RailLayout): RailLayout {
  const known = new Set(accounts.map((account) => account.id));
  const accountGroups = layout.accountGroups.map((group) => ({ ...group, accountIds: group.accountIds.filter((id) => known.has(id)) }));
  const groupIds = new Set(accountGroups.map((group) => group.id));
  const grouped = new Set(accountGroups.flatMap((group) => group.accountIds));
  const kept = layout.railOrder.filter((id) => groupIds.has(id) || (known.has(id) && !grouped.has(id)));
  const loose = accounts.map((account) => account.id).filter((id) => !grouped.has(id));
  return { railOrder: [...new Set([...kept, ...groupIds, ...loose])], accountGroups };
}

export function railEntries(accounts: Account[], layout: RailLayout): RailEntry[] {
  const { railOrder, accountGroups } = normalizeRail(accounts, layout);
  const byId = new Map(accounts.map((account) => [account.id, account]));
  const entries: RailEntry[] = [];
  for (const id of railOrder) {
    const group = accountGroups.find((item) => item.id === id);
    const account = byId.get(id);
    if (group) entries.push({ kind: "group", group, accounts: group.accountIds.flatMap((member) => byId.get(member) ?? []) });
    else if (account) entries.push({ kind: "account", account });
  }
  return entries;
}

function placed(list: string[], id: string, before: string | null): string[] {
  const rest = list.filter((item) => item !== id);
  const at = before === null ? -1 : rest.indexOf(before);
  return at < 0 ? [...rest, id] : [...rest.slice(0, at), id, ...rest.slice(at)];
}

function after(list: string[], id: string): string | null {
  const at = list.indexOf(id);
  return at < 0 ? null : (list[at + 1] ?? null);
}

export function moveAccount(layout: RailLayout, accountId: string, slot: RailSlot): RailLayout {
  if (slot.before === accountId) return layout;
  return {
    railOrder: slot.groupId ? layout.railOrder.filter((id) => id !== accountId) : placed(layout.railOrder, accountId, slot.before),
    accountGroups: layout.accountGroups.map((group) => ({
      ...group,
      accountIds: group.id === slot.groupId ? placed(group.accountIds, accountId, slot.before) : group.accountIds.filter((id) => id !== accountId),
    })),
  };
}

function moveTopLevel(layout: RailLayout, id: string, before: string | null): RailLayout {
  return before === id ? layout : { ...layout, railOrder: placed(layout.railOrder, id, before) };
}

/** Where dropping `item` on `target` puts it. `lower` is true over the row's bottom half. */
export function dropSlot(layout: RailLayout, item: RailItem, target: RailTarget, lower: boolean): RailSlot {
  if (target.kind === "group" && item.kind === "account" && lower) {
    const members = layout.accountGroups.find((group) => group.id === target.id)?.accountIds ?? [];
    return { groupId: target.id, before: target.collapsed ? null : (members[0] ?? null) };
  }
  const groupId = target.kind === "account" ? target.groupId : null;
  const siblings = groupId ? (layout.accountGroups.find((group) => group.id === groupId)?.accountIds ?? []) : layout.railOrder;
  return { groupId, before: lower ? after(siblings, target.id) : target.id };
}

export function applyDrop(layout: RailLayout, item: RailItem, slot: RailSlot): RailLayout {
  return item.kind === "group" ? moveTopLevel(layout, item.id, slot.before) : moveAccount(layout, item.id, slot);
}

/** Whether Move up (-1) or Move down (1) can do anything for an account or group. */
export function canStep(layout: RailLayout, id: string, step: -1 | 1): boolean {
  // The first or last account of a group steps out of it, so a member can always move.
  if (layout.accountGroups.some((group) => group.accountIds.includes(id))) return true;
  const at = layout.railOrder.indexOf(id);
  return at >= 0 && at + step >= 0 && at + step < layout.railOrder.length;
}

export function moveStep(layout: RailLayout, id: string, step: -1 | 1): RailLayout {
  if (!canStep(layout, id, step)) return layout;
  const group = layout.accountGroups.find((item) => item.accountIds.includes(id));
  const list = group ? group.accountIds : layout.railOrder;
  const to = list.indexOf(id) + step;
  if (to >= 0 && to < list.length) {
    const before = step < 0 ? list[to] : (list[to + 1] ?? null);
    return group ? moveAccount(layout, id, { groupId: group.id, before }) : moveTopLevel(layout, id, before);
  }
  const leaving = group as AccountGroup;
  return moveAccount(layout, id, { groupId: null, before: step < 0 ? leaving.id : after(layout.railOrder, leaving.id) });
}

/** Adds a group where the account sits now, with the account as its first member. */
export function addGroup(layout: RailLayout, group: AccountGroup, accountId: string): RailLayout {
  const owner = layout.accountGroups.find((item) => item.accountIds.includes(accountId));
  const next = {
    railOrder: placed(layout.railOrder, group.id, owner?.id ?? accountId),
    accountGroups: [...layout.accountGroups, group],
  };
  return moveAccount(next, accountId, { groupId: group.id, before: null });
}

/** Removes a group and leaves its accounts where it stood. */
export function ungroup(layout: RailLayout, groupId: string): RailLayout {
  const members = layout.accountGroups.find((group) => group.id === groupId)?.accountIds ?? [];
  return {
    railOrder: layout.railOrder.flatMap((id) => (id === groupId ? members : [id])),
    accountGroups: layout.accountGroups.filter((group) => group.id !== groupId),
  };
}

export function patchGroup(layout: RailLayout, groupId: string, patch: Partial<AccountGroup>): RailLayout {
  return { ...layout, accountGroups: layout.accountGroups.map((group) => (group.id === groupId ? { ...group, ...patch } : group)) };
}
