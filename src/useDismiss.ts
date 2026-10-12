import { useEffect, type RefObject } from "react";

/** Closes a pop-up when the user clicks outside `area` or presses Escape. */
export function useDismiss(area: RefObject<HTMLElement | null>, open: boolean, onClose: () => void): void {
  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      if (!area.current?.contains(event.target as Node)) onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [area, open, onClose]);
}
