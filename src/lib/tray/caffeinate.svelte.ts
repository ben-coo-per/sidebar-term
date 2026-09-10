// Caffeinate: whether this Mac is kept awake. Rust runs `caffeinate` in the background, in no
// Session (src-tauri/src/caffeinate.rs); this mirrors its state for the Tray's button.

import { caffeinateState, onCaffeinate, setCaffeinate } from "../ipc";

export const caffeinate = $state({ on: false });

/** Read the state and follow it turning off on its own; returns the function that stops following. */
export function initCaffeinate(): () => void {
  const listening = onCaffeinate((on) => (caffeinate.on = on));
  void caffeinateState().then((on) => (caffeinate.on = on));
  return () => void listening.then((stop) => stop());
}

export async function toggleCaffeinate(): Promise<void> {
  try {
    caffeinate.on = await setCaffeinate(!caffeinate.on);
  } catch (e) {
    console.error("caffeinate:", e);
  }
}
