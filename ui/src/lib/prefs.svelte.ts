// Small per-PC preferences kept in the app's own storage.

import type { Panel } from "./types";

const KEY = "syscura.usbPanels";

function load(): Record<string, Panel> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}");
  } catch {
    return {};
  }
}

/** Where the user says each USB port is, keyed by "hub:port" (the port,
 * not the device, so a new device in the same port is placed correctly). */
export const usbPanels = $state<Record<string, Panel>>(load());

export function portKey(hub: number | null, port: number | null): string {
  return `${hub ?? "?"}:${port ?? "?"}`;
}

export function setUsbPanel(key: string, panel: Panel | null) {
  if (panel) usbPanels[key] = panel;
  else delete usbPanels[key];
  try {
    localStorage.setItem(KEY, JSON.stringify(usbPanels));
  } catch {
    /* storage unavailable: keep for this session */
  }
}
