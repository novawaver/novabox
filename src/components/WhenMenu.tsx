import { useState } from "react";
import { whenChoices } from "../when";

export function WhenMenu({ title, onPick, onClose, upward = false }: { title: string; onPick: (at: number) => void; onClose: () => void; upward?: boolean }) {
  const [custom, setCustom] = useState("");
  return (
    <div className={`absolute left-0 z-20 w-64 rounded-xl border border-line bg-panel p-2 shadow-[var(--shadow)] ${upward ? "bottom-11" : "top-9"}`}>
      <p className="px-2 py-1 text-xs font-semibold uppercase tracking-wide text-muted">{title}</p>
      {whenChoices().map((choice) => (
        <button
          key={choice.label}
          type="button"
          className="block w-full rounded-lg px-2 py-1.5 text-left text-sm hover:bg-hover"
          onClick={() => {
            onPick(choice.at);
            onClose();
          }}
        >
          {choice.label}
        </button>
      ))}
      <label className="mt-1 block px-2 py-1 text-xs text-muted">
        Pick a time
        <input className="field mt-1" type="datetime-local" value={custom} onChange={(event) => setCustom(event.target.value)} />
      </label>
      <button
        type="button"
        className="mt-1 w-full rounded-lg px-2 py-1.5 text-left text-sm hover:bg-hover"
        onClick={() => {
          const at = new Date(custom).getTime();
          if (!custom || Number.isNaN(at)) return;
          onPick(at);
          onClose();
        }}
      >
        Use this time
      </button>
    </div>
  );
}
