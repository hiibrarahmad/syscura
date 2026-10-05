// Critical problems: shown as a red banner and as a Windows notification
// (once per problem) while Syscura is open.

import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { needsAttention } from "./findings";
import type { Finding } from "./types";

/** Problems where the person's files or PC are at real risk. */
export function isCritical(f: Finding): boolean {
  if (!needsAttention(f)) return false;
  const dangerous = ["disk.errors", "disk.ntfs", "hw.whea_fatal", "sec.threat", "sys.bsod"];
  return dangerous.includes(f.rule_id) || (f.harmful === "yes" && (f.severity === "critical" || f.severity === "error"));
}

/** Whether a backup is the right first step for this problem. */
export function backupFirst(f: Finding): boolean {
  return ["disk.errors", "disk.ntfs", "hw.whea_fatal", "sec.threat", "sys.bsod"].includes(f.rule_id);
}

const KEY = "syscura.notified";

export async function notifyNew(findings: Finding[]) {
  let seen: Record<string, number> = {};
  try { seen = JSON.parse(localStorage.getItem(KEY) ?? "{}"); } catch { /* fresh */ }
  const fresh = findings.filter((f) => (f.harmful === "yes" || f.severity === "critical") && needsAttention(f) && seen[f.id] !== f.count);
  if (!fresh.length) return;
  for (const f of fresh) seen[f.id] = f.count;
  try { localStorage.setItem(KEY, JSON.stringify(seen)); } catch { /* ignore */ }
  try {
    let ok = await isPermissionGranted();
    if (!ok) ok = (await requestPermission()) === "granted";
    if (!ok) return;
    const f = fresh[0];
    sendNotification({
      title: fresh.length > 1 ? `Syscura: ${fresh.length} serious problems` : `Syscura: ${f.title}`,
      body: backupFirst(f) ? "Back up your important files now, then open Syscura for what to do." : "Open Syscura to see what it means and how to fix it.",
    });
  } catch { /* notifications unavailable */ }
}
