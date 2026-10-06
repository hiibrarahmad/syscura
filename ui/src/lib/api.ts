import { invoke } from "@tauri-apps/api/core";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import type {
  ActionInfo, AiStatus, Analysis, BackupInfo, Finding, HardwareView, ProcessInfo, Question, Sensor, StatusInfo, StoredEvent,
} from "./types";

export const api = {
  agentStatus: () => invoke<StatusInfo | null>("agent_status"),
  events: (limit: number, errorsOnly: boolean) => invoke<StoredEvent[]>("events", { limit, errorsOnly }),
  hardware: (refresh: boolean) => invoke<HardwareView>("hardware", { refresh }),
  liveSensors: () => invoke<Sensor[]>("live_sensors"),
  findings: (includeClosed: boolean) => invoke<Finding[]>("findings", { includeClosed }),
  runFix: (finding: number, fix: number) => invoke<string>("run_fix", { finding, fix }),
  undoFix: (attempt: number) => invoke<string>("undo_fix", { attempt }),
  ignoreFinding: (finding: number, ignore: boolean) => invoke<string>("ignore_finding", { finding, ignore }),
  setVerdict: (finding: number, harmful: string, by: "you" | "ai") => invoke<string>("set_verdict", { finding, harmful, by }),
  processes: () => invoke<ProcessInfo[]>("processes"),
  recycleFile: (path: string) => invoke<string>("recycle_file", { path }),
  reveal: (path: string) => revealItemInDir(path),
  actions: () => invoke<ActionInfo[]>("actions"),
  applyAction: (a: { finding: number | null; title: string; action: string; params: Record<string, string>; label: string; automatic: boolean }) =>
    invoke<string>("apply_action", a),
  aiStatus: () => invoke<AiStatus>("ai_status"),
  aiSaveKey: (key: string) => invoke<string>("ai_save_key", { key }),
  aiForgetKey: () => invoke<void>("ai_forget_key"),
  aiSetAutoFix: (enabled: boolean) => invoke<void>("ai_set_auto_fix", { enabled }),
  aiAsk: (question: Question) => invoke<Analysis>("ai_ask", { question }),
  webPrompt: (question: Question) => invoke<string>("web_prompt", { question }),
  backupInfo: () => invoke<BackupInfo>("backup_info"),
  backupStart: (destination: string, folders: string[]) => invoke<string>("backup_start", { destination, folders }),
  startAgent: () => invoke<string>("start_agent"),
  installService: () => invoke<string>("install_service"),
  open: (url: string) => openUrl(url),
};

export function gb(bytes: number, digits = 0): string {
  const g = bytes / 1024 ** 3;
  return g >= 1000 ? `${(g / 1024).toFixed(1)} TB` : `${g.toFixed(digits)} GB`;
}

// Drive makers count in decimal units; show what the box says.
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
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
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
