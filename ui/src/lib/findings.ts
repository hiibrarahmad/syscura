import type { Finding } from "./types";

/** Problems worth the user's attention: not noise, not handled, and not a
 * purely informational "this is fine" item. */
export function needsAttention(f: Finding): boolean {
  if (f.category === "noise") return false;
  if (f.status !== "open" && f.status !== "fix_failed") return false;
  // Marked safe by the person: remembered, never flagged again.
  if (f.harmful === "no" && f.verdict_by === "you") return false;
  return !(f.harmful === "no" && f.severity === "info");
}
