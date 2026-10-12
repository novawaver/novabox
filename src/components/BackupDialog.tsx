import { useState } from "react";
import { api, type ImportItem } from "../api";
import { useMail } from "../state/MailProvider";
import { Overlay } from "./Dialogs";

const EXTENSION = "novabox";
const MIN_PASSWORD = 8;

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Export or import a password-protected backup of the accounts' Google sign-ins. */
export function BackupDialog({ mode, onClose }: { mode: "export" | "import"; onClose: () => void }) {
  const mail = useMail();
  const [path, setPath] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [done, setDone] = useState<string | null>(null);
  const [report, setReport] = useState<ImportItem[] | null>(null);
  const exporting = mode === "export";

  const pickFile = async () => {
    const dialog = await import("@tauri-apps/plugin-dialog");
    const filters = [{ name: "Novabox sign-in backup", extensions: [EXTENSION] }];
    const chosen = exporting
      ? await dialog.save({ defaultPath: `novabox-sign-ins-${new Date().toISOString().slice(0, 10)}.${EXTENSION}`, filters })
      : await dialog.open({ multiple: false, directory: false, filters });
    if (typeof chosen === "string") {
      setPath(chosen);
      setError("");
    }
  };

  const tooShort = exporting && password.length < MIN_PASSWORD;
  const mismatch = exporting && confirm !== password;
  const ready = path !== "" && password !== "" && !tooShort && !mismatch && !busy;

  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      if (exporting) {
        const result = await api.backupExport(path, password);
        const skipped = result.skipped.length > 0 ? ` Skipped (not signed in): ${result.skipped.join(", ")}.` : "";
        setDone(`Saved ${result.exported.length} account${result.exported.length === 1 ? "" : "s"}.${skipped}`);
      } else {
        setReport(await mail.importSignIns(path, password));
      }
      setPassword("");
      setConfirm("");
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setBusy(false);
    }
  };

  const fileName = path.split(/[\\/]/).pop() ?? "";
  const close = () => {
    if (busy) return;
    if (!exporting) mail.finishImport();
    onClose();
  };
  const finished = done !== null || report !== null;

  return (
    <Overlay title={exporting ? "Export sign-ins" : "Import sign-ins"} onClose={close}>
      {finished ? (
        <div className="space-y-3 text-sm">
          {done && <p>{done}</p>}
          {report && (
            <ul className="space-y-2">
              {report.map((item) => (
                <li key={item.email} className="rounded-xl border border-line p-3">
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate font-medium">{item.email}</span>
                    <span className={`shrink-0 text-xs font-semibold ${item.status === "failed" ? "text-danger" : "text-accent"}`}>
                      {item.status === "added" ? "Added" : item.status === "updated" ? "Signed in again" : "Not restored"}
                    </span>
                  </div>
                  {item.message && <p className="mt-1 text-muted">{item.message}</p>}
                </li>
              ))}
            </ul>
          )}
          <div className="flex justify-end">
            <button type="button" className="rounded-full bg-accent px-4 py-2 font-semibold text-white" onClick={close}>Done</button>
          </div>
        </div>
      ) : (
        <form
          className="space-y-3 text-sm"
          onSubmit={(event) => {
            event.preventDefault();
            if (ready) void submit();
          }}
        >
          <p className="leading-6 text-muted">
            {exporting
              ? "Saves every signed-in account to one encrypted file. Import it on another PC to use the accounts there without signing in again. Mail is not included; it downloads again."
              : "Restores the accounts saved in a Novabox backup file. Each sign-in is checked with Google first."}
          </p>
          {exporting && (
            <p className="rounded-xl bg-selected p-3 leading-6">
              Anyone with this file and its password can read and send your mail. Keep it somewhere safe, and use a password you don't use elsewhere.
            </p>
          )}
          <div className="flex items-center gap-2">
            <button type="button" className="shrink-0 rounded-full border border-line px-3 py-1.5 hover:bg-hover" onClick={() => void pickFile()}>
              {exporting ? "Choose where to save…" : "Choose backup file…"}
            </button>
            <span className="min-w-0 truncate text-muted" title={path}>{fileName || "No file chosen"}</span>
          </div>
          <label className="block">
            <span className="mb-1 block text-muted">Password</span>
            <input className="field" type="password" autoComplete={exporting ? "new-password" : "current-password"} value={password} onChange={(event) => setPassword(event.target.value)} />
          </label>
          {exporting && (
            <label className="block">
              <span className="mb-1 block text-muted">Confirm password</span>
              <input className="field" type="password" autoComplete="new-password" value={confirm} onChange={(event) => setConfirm(event.target.value)} />
            </label>
          )}
          {exporting && password !== "" && tooShort && <p className="text-muted">Use at least {MIN_PASSWORD} characters.</p>}
          {exporting && confirm !== "" && mismatch && <p className="text-danger">The passwords don't match.</p>}
          {error && <p className="text-danger">{error}</p>}
          <div className="flex justify-end gap-2 pt-1">
            <button type="button" className="rounded-full px-4 py-2 disabled:opacity-50" disabled={busy} onClick={close}>Cancel</button>
            <button type="submit" className="rounded-full bg-accent px-4 py-2 font-semibold text-white disabled:opacity-50" disabled={!ready}>
              {busy ? (exporting ? "Encrypting…" : "Checking with Google…") : exporting ? "Export" : "Import"}
            </button>
          </div>
        </form>
      )}
    </Overlay>
  );
}
