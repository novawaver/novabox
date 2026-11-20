import { useState } from "react";
import { Overlay } from "./Dialogs";
import { GoogleMark, IconBack, IconMail } from "./Icons";
import { useMail } from "../state/MailProvider";
import { PRIVATE_EMAIL } from "../types";

type Step = "choose" | "privateemail";

/** Picks the mail service for a new account. Google signs in in the browser; Private Email asks for the mailbox password here. */
export function AddAccountDialog({ start = "choose", onClose }: { start?: Step; onClose: () => void }) {
  const mail = useMail();
  const [step, setStep] = useState<Step>(start);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const close = () => {
    if (!busy) onClose();
  };
  const ready = email.trim() !== "" && password !== "" && !busy;
  const submit = async () => {
    setBusy(true);
    setError("");
    const problem = await mail.signInPassword(PRIVATE_EMAIL, email, password, name);
    setBusy(false);
    if (problem) setError(problem);
    else onClose();
  };

  if (step === "choose") {
    return (
      <Overlay title="Add account" onClose={close}>
        <p className="text-sm text-muted">Choose where the mailbox is.</p>
        <div className="mt-4 space-y-2">
          <button
            type="button"
            autoFocus
            className="flex w-full items-center gap-3 rounded-xl border border-line p-3 text-left hover:bg-hover disabled:opacity-50 disabled:hover:bg-transparent"
            disabled={!mail.googleReady}
            onClick={() => {
              onClose();
              void mail.signIn();
            }}
          >
            <span className="grid h-9 w-9 shrink-0 place-items-center rounded-full bg-white"><GoogleMark /></span>
            <span className="min-w-0">
              <span className="block font-medium">Gmail</span>
              <span className="block text-sm text-muted">
                {mail.googleReady ? "Opens Google sign-in in your browser. Come back to this window when you're done." : "Unavailable: this build of Novabox has no Google client ID."}
              </span>
            </span>
          </button>
          <button
            type="button"
            className="flex w-full items-center gap-3 rounded-xl border border-line p-3 text-left hover:bg-hover"
            onClick={() => setStep("privateemail")}
          >
            <span className="grid h-9 w-9 shrink-0 place-items-center rounded-full bg-accent/15 text-accent"><IconMail /></span>
            <span className="min-w-0">
              <span className="block font-medium">Private Email</span>
              <span className="block text-sm text-muted">A privateemail.com mailbox. Sign in with its email address and password.</span>
            </span>
          </button>
        </div>
        <div className="mt-5 flex justify-end">
          <button type="button" className="rounded-full px-4 py-2" onClick={close}>Cancel</button>
        </div>
      </Overlay>
    );
  }

  return (
    <Overlay title="Add a Private Email account" onClose={close}>
      <form
        className="space-y-3 text-sm"
        onSubmit={(event) => {
          event.preventDefault();
          if (ready) void submit();
        }}
      >
        <p className="leading-6 text-muted">
          Use the address and password you use at privateemail.com. Novabox signs in from this PC and keeps the password in Windows Credential Manager.
        </p>
        <label className="block">
          <span className="mb-1 block text-muted">Email address</span>
          <input className="field" type="email" autoFocus autoComplete="username" spellCheck={false} placeholder="you@yourdomain.com" value={email} onChange={(event) => setEmail(event.target.value)} />
        </label>
        <label className="block">
          <span className="mb-1 block text-muted">Password</span>
          <input className="field" type="password" autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} />
        </label>
        <label className="block">
          <span className="mb-1 block text-muted">Your name, as it appears on mail you send (optional)</span>
          <input className="field" type="text" autoComplete="name" maxLength={80} value={name} onChange={(event) => setName(event.target.value)} />
        </label>
        {error && <p className="text-danger" role="alert">{error}</p>}
        <div className="flex items-center justify-between gap-2 pt-1">
          {start === "choose" ? (
            <button type="button" className="inline-flex items-center gap-1 rounded-full px-3 py-2 text-muted hover:text-ink disabled:opacity-50" disabled={busy} onClick={() => setStep("choose")}>
              <IconBack /> Back
            </button>
          ) : <span />}
          <div className="flex gap-2">
            <button type="button" className="rounded-full px-4 py-2 disabled:opacity-50" disabled={busy} onClick={close}>Cancel</button>
            <button type="submit" className="rounded-full bg-accent px-4 py-2 font-semibold text-accent-ink disabled:opacity-50" disabled={!ready}>
              {busy ? "Signing in…" : "Add account"}
            </button>
          </div>
        </div>
      </form>
    </Overlay>
  );
}
