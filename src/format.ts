export function formatListDate(ms: number): string {
  const date = new Date(ms);
  const now = new Date();
  if (date.toDateString() === now.toDateString()) {
    return date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  }
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString([], { month: "short", day: "numeric" });
  }
  return date.toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" });
}

export function formatFullDate(ms: number): string {
  return new Date(ms).toLocaleString([], {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

export function personName(name: string, email: string): string {
  return name.trim() || email;
}

export function initials(name: string, email: string): string {
  const source = (name.trim() || email).replace(/@.*/, "");
  const parts = source.split(/[\s._-]+/).filter(Boolean).slice(0, 2);
  const letters = parts.map((part) => part[0]?.toUpperCase() ?? "").join("");
  return letters || "?";
}

export function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

export function threadKey(accountId: string, threadId: string): string {
  return `${accountId}\t${threadId}`;
}

export function parseThreadKey(key: string): { accountId: string; threadId: string } {
  const [accountId, threadId] = key.split("\t");
  return { accountId, threadId };
}

export function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

export function formatPeople(addresses: { name: string; email: string }[]): string {
  return addresses.map((address) => personName(address.name, address.email)).join(", ");
}
