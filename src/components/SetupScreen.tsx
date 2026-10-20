import { LogoMark } from "./Icons";
import { useMail } from "../state/MailProvider";

/** Shown while a newly added account downloads its first batch of mail, so the inbox is not used half-filled. */
export function SetupScreen() {
  const mail = useMail();
  const setup = mail.setup;
  if (!setup) return null;
  const failed = setup.error !== "";
  return (
    <div className="grid h-full place-items-center bg-bg px-6 py-10 text-ink">
      <div className="w-full max-w-md text-center" role="status" aria-live="polite">
        <div className="relative mx-auto mb-6 h-16 w-16">
          <LogoMark className="h-16 w-16" />
          {!failed && <span className="absolute inset-[-8px] animate-spin rounded-full border-2 border-transparent border-t-accent" aria-hidden />}
        </div>
        <h1 className="text-2xl font-semibold tracking-tight">{failed ? "Could not finish setting up" : "Setting up your inbox"}</h1>
        <p className="mt-2 truncate text-muted">{setup.email}</p>
        {failed ? (
          <>
            <p className="mt-6 text-sm text-danger">{setup.error}</p>
            <div className="mt-6 flex justify-center gap-3">
              <button type="button" className="rounded-full bg-accent px-5 py-2.5 font-semibold text-accent-ink" onClick={mail.retrySetup}>
                Try again
              </button>
              <button type="button" className="rounded-full border border-line px-5 py-2.5 font-medium" onClick={mail.skipSetup}>
                Continue anyway
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="mt-6 text-lg font-medium tabular-nums">
              {setup.fetched > 0 ? `Downloaded ${setup.fetched.toLocaleString()} messages` : "Connecting to Gmail…"}
            </p>
            <p className="mt-2 text-sm text-muted">Your newest mail is downloading. Older mail keeps downloading in the background after this.</p>
          </>
        )}
      </div>
    </div>
  );
}
