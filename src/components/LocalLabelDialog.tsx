import { useState } from "react";
import { Overlay } from "./Dialogs";
import { useMail } from "../state/MailProvider";
import { LOCAL_LABEL_COLORS, type LocalLabel } from "../types";

/** Creates a Novabox label, or renames, recolours or deletes one. These labels stay on this PC. */
export function LocalLabelDialog({ label, onClose }: { label?: LocalLabel; onClose: () => void }) {
  const mail = useMail();
  const [name, setName] = useState(label?.name ?? "");
  const [color, setColor] = useState(label?.color || LOCAL_LABEL_COLORS[0]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const ready = name.trim() !== "" && !busy;

  const save = async () => {
    setBusy(true);
    setError("");
    const problem = await mail.saveLocalLabel({ id: label?.id ?? "", name: name.trim(), color });
    setBusy(false);
    if (problem) setError(problem);
    else onClose();
  };

  return (
    <Overlay title={label ? "Edit Novabox label" : "New Novabox label"} onClose={() => !busy && onClose()}>
      <form
        className="space-y-4 text-sm"
        onSubmit={(event) => {
          event.preventDefault();
          if (ready) void save();
        }}
      >
        <p className="leading-6 text-muted">
          Novabox labels are kept on this PC only. They work across all your accounts and are never added to Gmail or your mail server.
        </p>
        <label className="block">
          <span className="mb-1 block text-muted">Name</span>
          <input className="field" autoFocus maxLength={40} value={name} onChange={(event) => setName(event.target.value)} />
        </label>
        <div>
          <span className="mb-2 block text-muted">Colour</span>
          <div className="flex flex-wrap gap-2" role="radiogroup" aria-label="Label colour">
            {LOCAL_LABEL_COLORS.map((item) => (
              <button
                key={item}
                type="button"
                role="radio"
                aria-checked={color === item}
                aria-label={item}
                className={`h-7 w-7 rounded-full border-2 ${color === item ? "border-ink" : "border-transparent"}`}
                style={{ background: item }}
                onClick={() => setColor(item)}
              />
            ))}
          </div>
        </div>
        {error && <p className="text-danger" role="alert">{error}</p>}
        <div className="flex items-center justify-between gap-2 pt-1">
          {label ? (
            <button
              type="button"
              className="rounded-full px-3 py-2 text-danger hover:bg-hover disabled:opacity-50"
              disabled={busy}
              onClick={() => {
                onClose();
                mail.deleteLocalLabel(label);
              }}
            >
              Delete label…
            </button>
          ) : <span />}
          <div className="flex gap-2">
            <button type="button" className="rounded-full px-4 py-2 disabled:opacity-50" disabled={busy} onClick={onClose}>Cancel</button>
            <button type="submit" className="rounded-full bg-accent px-4 py-2 font-semibold text-accent-ink disabled:opacity-50" disabled={!ready}>
              {busy ? "Saving…" : label ? "Save" : "Create"}
            </button>
          </div>
        </div>
      </form>
    </Overlay>
  );
}
