import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Finding, HardwareView, PartImage, Sensor, StatusInfo, StoredEvent } from "./types";

export const api = {
  agentStatus: () => invoke<StatusInfo | null>("agent_status"),
  events: (limit: number, errorsOnly: boolean) => invoke<StoredEvent[]>("events", { limit, errorsOnly }),
  hardware: (refresh: boolean) => invoke<HardwareView>("hardware", { refresh }),
  liveSensors: () => invoke<Sensor[]>("live_sensors"),
  partImages: () => invoke<PartImage[]>("part_images"),
  partImageData: (key: string) => invoke<string | null>("part_image_data", { key }),
  retryPartImage: (key: string) => invoke<void>("retry_part_image", { key }),
  setPartImageUrl: (key: string, url: string) => invoke<void>("set_part_image_url", { key, url }),
  setPartImageBytes: (key: string, bytes: number[], fileName: string) =>
    invoke<void>("set_part_image_bytes", { key, bytes, fileName }),
  open: (url: string) => openUrl(url),
  findings: (includeClosed: boolean) => invoke<Finding[]>("findings", { includeClosed }),
  runFix: (finding: number, fix: number) => invoke<string>("run_fix", { finding, fix }),
  undoFix: (attempt: number) => invoke<string>("undo_fix", { attempt }),
  ignoreFinding: (finding: number, ignore: boolean) => invoke<string>("ignore_finding", { finding, ignore }),
  startAgent: () => invoke<string>("start_agent"),
  installService: () => invoke<string>("install_service"),
};

// Picture data URLs, cached by key + version so polling stays cheap.
const imageCache = new Map<string, string>();
export async function imageData(img: PartImage): Promise<string | null> {
  const id = `${img.key}@${img.version}`;
  const hit = imageCache.get(id);
  if (hit) return hit;
  const data = await api.partImageData(img.key);
  if (data) imageCache.set(id, data);
  return data;
}

export function gb(bytes: number, digits = 0): string {
  const g = bytes / 1024 ** 3;
  return g >= 1000 ? `${(g / 1024).toFixed(1)} TB` : `${g.toFixed(digits)} GB`;
}

// Drive makers count in decimal gigabytes; show what the box says.
export function driveSize(bytes: number): string {
  const g = bytes / 1e9;
  return g >= 1000 ? `${(g / 1000).toFixed(g >= 10000 ? 0 : 1)} TB` : `${Math.round(g)} GB`;
}

export function mb(bytes: number): string {
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}

export function ago(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

export function duration(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m ${secs % 60}s`;
}

/** Colour for a temperature reading. */
export function tempTone(c: number | null | undefined): "ok" | "warn" | "danger" | "none" {
  if (c == null) return "none";
  if (c >= 85) return "danger";
  if (c >= 70) return "warn";
  return "ok";
}

export function fmtSensor(s: Sensor): string {
  if (s.value == null) return "—";
  const v = s.unit === "V" ? s.value.toFixed(3) : Number.isInteger(s.value) ? s.value.toString() : s.value.toFixed(1);
  return `${v}${s.unit === "%" || s.unit === "°C" ? "" : " "}${s.unit}`;
}
