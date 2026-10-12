import { IconCloudOff, IconMenu, IconSearch, IconSettings, IconSync, LogoMark } from "./Icons";
import { useMail } from "../state/MailProvider";

export function TopBar() {
  const mail = useMail();
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
