import { useState } from "react";
import { BackupDialog } from "./BackupDialog";
import { GoogleMark, LogoMark } from "./Icons";
import { useMail } from "../state/MailProvider";
import { inTauri } from "../platform";

export function Onboarding() {
  const mail = useMail();
  const [importing, setImporting] = useState(false);
  return (
    <div className="grid min-h-full place-items-center bg-bg px-6 py-10">
      <div className="w-full max-w-xl">
        <div className="mb-8 flex items-center gap-3">
          <LogoMark className="h-11 w-11" />
          <div>
            <div className="text-lg font-semibold tracking-tight">Novabox</div>
            <div className="text-sm text-muted">Works with Gmail</div>
          </div>
        </div>
        <h1 className="text-4xl font-semibold tracking-tight text-balance">Every Gmail account, one window.</h1>
        <p className="mt-4 max-w-lg text-lg leading-relaxed text-muted">
          Novabox reads and sends mail from this PC directly to Google. Messages stay on this computer. They are not uploaded to a Novawave server.
        </p>
        <div className="mt-8 flex flex-wrap items-center gap-3">
          <button type="button" className="rounded-full bg-accent px-5 py-2.5 font-semibold text-accent-ink" onClick={mail.openDemo}>
            Explore the sample inbox
          </button>
          <button
            type="button"
            className="inline-flex items-center gap-3 rounded-full border border-[#dadce0] bg-white px-4 py-2.5 font-medium text-[#1f1f1f]"
            onClick={() => void mail.signIn()}
            disabled={mail.signInBusy || !mail.googleReady}
          >
            <GoogleMark />
            {mail.signInBusy ? "Waiting for Google…" : "Sign in with Google"}
          </button>
          {mail.signInBusy && (
            <button type="button" className="rounded-full border border-line px-4 py-2.5 font-medium" onClick={mail.cancelSignIn}>
              Cancel
            </button>
          )}
        </div>
        {inTauri() && (
          <p className="mt-4 text-sm text-muted">
            Moving from another PC?{" "}
            <button type="button" className="font-medium text-accent hover:underline" onClick={() => setImporting(true)}>
              Import sign-ins from a backup
            </button>
          </p>
        )}
        {!mail.googleReady && (
          <p className="mt-4 text-sm text-danger">This build of Novabox has no Google client ID, so Google sign-in is unavailable. Set NOVABOX_GOOGLE_CLIENT_ID in .env and rebuild.</p>
        )}
        {mail.signInError && <p className="mt-4 text-sm text-danger">{mail.signInError}</p>}
        {importing && <BackupDialog mode="import" onClose={() => setImporting(false)} />}
      </div>
    </div>
  );
}
