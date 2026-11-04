import { api } from "./api";
import { inTauri } from "./platform";
import type { UpdateInfo, UpdateProgress } from "./types";

/** Where releases are published. Shown in Settings > About for anyone who prefers to install by hand. */
export const RELEASES_URL = "https://github.com/novawaver/novabox/releases/latest";

/** Asks the update feed for a release newer than this build. Resolves to null when this build is current. */
export async function checkForUpdates(): Promise<UpdateInfo | null> {
  if (!inTauri()) throw new Error("Updates are checked from the Novabox desktop app.");
  return api.updateCheck();
}

/**
 * Downloads and installs the release from the last check, reporting progress as it goes.
 * Novabox closes and the new version starts when the install finishes, so this only
 * resolves when something went wrong before that point (the error is thrown).
 */
export async function installUpdate(onProgress: (progress: UpdateProgress) => void): Promise<void> {
  if (!inTauri()) throw new Error("Updates are installed from the Novabox desktop app.");
  const { listen } = await import("@tauri-apps/api/event");
  const stop = await listen<UpdateProgress>("update://progress", (event) => onProgress(event.payload));
  try {
    await api.updateInstall();
  } finally {
    stop();
  }
}
