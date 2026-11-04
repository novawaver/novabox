import { useEffect, useRef, useState } from "react";
import { inTauri, openExternal } from "../platform";
import { useMail } from "../state/MailProvider";
import type { UpdateInfo, UpdateProgress } from "../types";
import { RELEASES_URL, checkForUpdates, installUpdate } from "../updates";

type Phase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "installing"; info: UpdateInfo; progress: UpdateProgress | null }
  | { kind: "error"; message: string; info: UpdateInfo | null };

function megabytes(bytes: number): string {
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

/** The About tab in Settings: version, and checking for and installing updates. */
export function AboutSection() {
  const mail = useMail();
  const [phase, setPhase] = useState<Phase>(() => (mail.updateAvailable ? { kind: "available", info: mail.updateAvailable } : { kind: "idle" }));
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  // A background check can finish while this tab is open.
  useEffect(() => {
    if (mail.updateAvailable && (phase.kind === "idle" || phase.kind === "current")) setPhase({ kind: "available", info: mail.updateAvailable });
  }, [mail.updateAvailable, phase.kind]);

  async function check() {
    setPhase({ kind: "checking" });
    try {
      const info = await checkForUpdates();
      mail.noteUpdate(info);
      if (!mounted.current) return;
      setPhase(info ? { kind: "available", info } : { kind: "current" });
    } catch (error) {
      if (mounted.current) setPhase({ kind: "error", message: error instanceof Error ? error.message : String(error), info: null });
    }
  }

  async function install(info: UpdateInfo) {
    setPhase({ kind: "installing", info, progress: null });
    try {
      await installUpdate((progress) => {
        if (mounted.current) setPhase({ kind: "installing", info, progress });
      });
    } catch (error) {
      if (mounted.current) setPhase({ kind: "error", message: error instanceof Error ? error.message : String(error), info });
    }
  }

  const desktop = inTauri();
  const busy = phase.kind === "checking" || phase.kind === "installing";
  const offered = phase.kind === "available" ? phase.info : phase.kind === "error" ? phase.info : null;
  return (
    <>
      <section className="space-y-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted">About</h3>
        <p className="text-sm">
          Novabox {desktop ? mail.appVersion || "" : "browser preview"}
        </p>
        <p className="text-sm leading-6 text-muted">A Windows mail app for several Gmail accounts in one window. Mail stays on this PC.</p>
      </section>
      <section className="mt-6 space-y-2">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-muted">Updates</h3>
        <p className="text-sm leading-6 text-muted">
          Novabox looks for a new version shortly after it starts and twice a day while it runs. Installing takes under a minute and restarts the app. Nothing on this PC is sent anywhere during the check.
        </p>
        {phase.kind === "current" && <p className="text-sm">You have the latest version.</p>}
        {phase.kind === "checking" && <p className="text-sm text-muted">Checking…</p>}
        {phase.kind === "error" && (
          <p className="text-sm text-danger" role="alert">
            {phase.message}
          </p>
        )}
        {offered && (
          <div className="rounded-xl border border-line bg-bg p-3 text-sm">
            <p className="font-semibold">Version {offered.version} is ready to install</p>
            <p className="text-muted">
              You have {offered.currentVersion}.
              {offered.dateUnix ? ` Released ${new Date(offered.dateUnix * 1000).toLocaleDateString()}.` : ""}
            </p>
            {offered.notes && <pre className="mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap font-sans text-sm leading-6">{offered.notes}</pre>}
          </div>
        )}
        {phase.kind === "installing" && (
          <div className="space-y-1 text-sm" aria-live="polite">
            <p>
              {phase.progress?.phase === "installing"
                ? "Installing… Novabox will restart in a moment."
                : phase.progress
                  ? `Downloading… ${megabytes(phase.progress.downloaded)}${phase.progress.total ? ` of ${megabytes(phase.progress.total)}` : ""}`
                  : "Starting download…"}
            </p>
            <div className="h-2 overflow-hidden rounded-full bg-hover">
              <div
                className="h-full rounded-full bg-accent transition-[width]"
                style={{ width: phase.progress?.phase === "installing" ? "100%" : phase.progress?.total ? `${Math.min(100, (100 * phase.progress.downloaded) / phase.progress.total)}%` : "15%" }}
              />
            </div>
          </div>
        )}
        <div className="flex flex-wrap gap-2 pt-1">
          {offered ? (
            <button type="button" className="rounded-full bg-accent px-4 py-1.5 text-sm font-semibold text-accent-ink disabled:opacity-50" disabled={busy} onClick={() => void install(offered)}>
              Install and restart
            </button>
          ) : null}
          <button type="button" className="rounded-full border border-line px-3 py-1.5 text-sm hover:bg-hover disabled:opacity-50" disabled={!desktop || busy} onClick={() => void check()}>
            {offered ? "Check again" : "Check for updates"}
          </button>
          <button type="button" className="rounded-full border border-line px-3 py-1.5 text-sm hover:bg-hover" onClick={() => void openExternal(RELEASES_URL)}>
            All releases
          </button>
        </div>
        {!desktop && <p className="text-sm text-muted">The browser preview has no installer, so there is nothing to update here.</p>}
      </section>
    </>
  );
}
