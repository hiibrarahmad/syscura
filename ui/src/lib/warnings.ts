// Critical problems: shown as a red banner. Windows notifications for them
// come from the tray (src-tauri/src/tray.rs), so they work with the window closed.

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
