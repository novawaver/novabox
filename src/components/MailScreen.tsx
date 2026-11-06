import { AccountRail } from "./AccountRail";
import { ComposeDock } from "./ComposeDock";
import { SettingsDialog } from "./Dialogs";
import { LogoMark } from "./Icons";
import { MessageList } from "./MessageList";
import { Onboarding } from "./Onboarding";
import { SetupScreen } from "./SetupScreen";
import { Sidebar } from "./Sidebar";
import { ThreadView } from "./ThreadView";
import { TopBar } from "./TopBar";
import { NoticeStack } from "./Notices";
import { railWidth } from "../rail";
import { useMail } from "../state/MailProvider";

export function MailScreen() {
  const mail = useMail();
  if (!mail.ready) {
    return (
      <div className="grid h-full place-items-center bg-bg">
        <div className="flex items-center gap-3">
          <LogoMark className="h-10 w-10" />
          <span className="text-lg font-semibold">Novabox</span>
        </div>
      </div>
    );
  }
  if (mail.setup) return <SetupScreen />;
  return (
    <div className="flex h-full flex-col bg-bg text-ink">
      {mail.mode === "demo" && (
        <div className="flex items-center justify-between gap-3 bg-selected px-4 py-2 text-sm">
          <span>Sample inbox. Nothing here is sent to Google.</span>
          <button type="button" className="font-semibold" onClick={mail.leaveDemo}>Leave sample</button>
        </div>
      )}
      {mail.mode === "none" ? <Onboarding /> : (
        <>
          <TopBar />
          <div className="flex min-h-0 flex-1">
            <AccountRail />
            <Sidebar collapsed={!mail.sidebarOpen} />
            <main className={`flex min-w-0 flex-1 ${mail.settings.readingPane === "bottom" ? "flex-col" : ""}`}>
              {mail.settings.readingPane === "none" && (mail.thread || mail.threadLoading) ? (
                <ThreadView />
              ) : mail.settings.readingPane === "right" ? (
                <>
                  <MessageList className="max-w-[460px] border-r border-line" />
                  <ThreadView />
                </>
              ) : mail.settings.readingPane === "bottom" ? (
                <>
                  <MessageList className="max-h-[45%] border-b border-line" />
                  <ThreadView />
                </>
              ) : (
                <MessageList />
              )}
            </main>
          </div>
        </>
      )}
      <ComposeDock />
      <SettingsDialog />
      <NoticeStack />
      {/* Starts just right of the account bar so it never covers the avatars. */}
      <div
        className="pointer-events-none fixed bottom-4 z-50 flex flex-col items-start gap-2"
        style={{ left: railWidth(mail.settings.showAccountEmails) + 16, maxWidth: `calc(100vw - ${railWidth(mail.settings.showAccountEmails) + 32}px)` }}
      >
        {mail.undo && (
          <div className="pointer-events-auto flex items-center gap-3 bg-[#241c14]/85 backdrop-blur-sm px-4 py-2 text-sm text-[#f6f1e7] shadow-[var(--shadow)]">
            <span>{mail.undo.text}</span>
            <button type="button" className="font-semibold text-accent" onClick={() => void mail.undoAct()}>Undo</button>
          </div>
        )}
        {mail.toasts.map((toast) => (
          <button key={toast.id} type="button" className="pointer-events-auto bg-[#241c14]/85 backdrop-blur-sm px-4 py-2 text-left text-sm text-[#f6f1e7]" onClick={() => mail.dismissToast(toast.id)}>
            {toast.text}
          </button>
        ))}
      </div>
    </div>
  );
}
