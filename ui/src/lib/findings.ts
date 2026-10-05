import type { Finding } from "./types";

/** Problems worth the user's attention: not noise, not handled, and not a
 * purely informational "this is fine" item. */
export function needsAttention(f: Finding): boolean {
  if (f.category === "noise") return false;
  if (f.status !== "open" && f.status !== "fix_failed") return false;
  return !(f.harmful === "no" && f.severity === "info");
}
