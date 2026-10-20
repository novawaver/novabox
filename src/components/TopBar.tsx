import { IconCloudOff, IconMenu, IconSearch, IconSettings, IconSignal, IconSync, LogoMark } from "./Icons";
import { useMail, type Connection } from "../state/MailProvider";

// Round trips to Gmail's history endpoint; a healthy connection is usually well under 600 ms.
const STRONG_MS = 600;
const FAIR_MS = 1500;

function describeConnection(connection: Connection): { level: number; off: boolean; tone: string; text: string } {
  const { state, latencyMs, message } = connection;
  if (state === "offline") return { level: 0, off: true, tone: "text-danger", text: "Offline. Gmail cannot be reached." };
  if (state === "error") return { level: 0, off: true, tone: "text-danger", text: `Gmail error: ${message}` };
  if (state === "checking") return { level: 0, off: false, tone: "text-muted", text: "Checking connection to Gmail…" };
  if (latencyMs === null) return { level: 3, off: false, tone: "", text: "Connected to Gmail" };
  const timing = `${latencyMs.toLocaleString()} ms`;
  if (latencyMs <= STRONG_MS) return { level: 3, off: false, tone: "", text: `Connected to Gmail · strong (${timing})` };
  if (latencyMs <= FAIR_MS) return { level: 2, off: false, tone: "text-star", text: `Connected to Gmail · fair (${timing})` };
  return { level: 1, off: false, tone: "text-danger", text: `Connected to Gmail · slow (${timing}). New mail may arrive late.` };
}

export function TopBar() {
  const mail = useMail();
  const link = describeConnection(mail.connection);
  const status = mail.syncing ? mail.syncLabel || "Syncing…" : mail.offline ? "Offline. Click to try again." : "Up to date. Click to check for new mail.";
  return (
    <header className="flex h-14 shrink-0 items-center gap-3 border-b border-line bg-panel px-3">
      <button type="button" className="icon-btn" aria-label={mail.sidebarOpen ? "Collapse folders" : "Expand folders"} aria-expanded={mail.sidebarOpen} onClick={mail.toggleSidebar}>
        <IconMenu />
      </button>
      <LogoMark />
      <span className="hidden font-semibold tracking-tight sm:inline">Novabox</span>
      <form
        className="mx-auto flex w-full max-w-2xl items-center gap-2 rounded-full border border-line bg-bg px-3 py-1.5"
        onSubmit={(event) => {
          event.preventDefault();
          void mail.submitSearch();
        }}
      >
        <IconSearch />
        <input
          id="mail-search"
          className="w-full bg-transparent outline-none"
          placeholder="Search mail"
          value={mail.query.search}
          onChange={(event) => mail.setSearch(event.target.value)}
          aria-label="Search mail"
        />
      </form>
      {mail.mode === "live" && (
        <span className={`grid h-10 w-8 shrink-0 place-items-center ${link.tone}`} role="img" title={link.text} aria-label={link.text}>
          <IconSignal level={link.level} off={link.off} className={`h-5 w-5 ${mail.connection.state === "checking" ? "animate-pulse" : ""}`} />
        </span>
      )}
      <button
        type="button"
        className={`icon-btn h-10 w-10 rounded-xl ${mail.offline && !mail.syncing ? "text-danger" : ""}`}
        title={status}
        aria-label={status}
        aria-busy={mail.syncing}
        onClick={() => {
          if (!mail.syncing) void mail.refresh();
        }}
      >
        {mail.offline && !mail.syncing ? <IconCloudOff className="h-[22px] w-[22px]" /> : <IconSync className={`h-[22px] w-[22px] ${mail.syncing ? "animate-spin text-accent" : ""}`} />}
      </button>
      <button type="button" className="icon-btn h-10 w-10 rounded-xl" title="Settings" aria-label="Settings" onClick={() => mail.setSettingsOpen(true)}>
        <IconSettings className="h-[22px] w-[22px]" />
      </button>
    </header>
  );
}
