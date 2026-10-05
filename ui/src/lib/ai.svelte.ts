// AI help shared by every screen: cached answers, and the automatic mode
// that asks about new problems and applies the safe fixes it picks.

import { api } from "./api";
import { needsAttention } from "./findings";
import type { ActionInfo, AiStatus, Analysis, Finding, FixAttempt, Question } from "./types";

export const ai = $state({
  status: { configured: false, model: null, auto_fix: false } as AiStatus,
  catalog: [] as ActionInfo[],
  /** Answers by question key. */
  answers: {} as Record<string, Analysis>,
  busy: {} as Record<string, boolean>,
  errors: {} as Record<string, string>,
  /** What automatic mode did, newest first. */
  log: [] as { ts: number; text: string }[],
});

export async function refreshAi() {
  try { ai.status = await api.aiStatus(); } catch { /* keep last */ }
  try { ai.catalog = await api.actions(); } catch { ai.catalog = []; }
}

export function riskOf(action: string) {
  return ai.catalog.find((a) => a.id === action);
}

export function findingQuestion(f: Finding): Question {
  const details: Record<string, string> = {};
  if (f.message) details["Windows message"] = f.message;
  for (const [k, v] of Object.entries(f.evidence)) if (v && v.length < 600) details[k] = v;
  details["How often"] = `${f.count} time(s) since ${new Date(f.first_ts).toLocaleDateString()}`;
  return { title: f.group ? `${f.title} (${f.group})` : f.title, explanation: f.explanation, details, system: "" };
}

/** What a fix reported, in words the AI (or a person) can judge. */
export function attemptResult(a: FixAttempt): string {
  const state = !a.ok
    ? "It failed to run"
    : a.outcome === "nothing_found"
      ? "The tool found nothing wrong"
      : a.outcome === "not_repaired"
        ? "The tool found a problem but could not repair it"
        : a.verified === false
          ? "The problem came back or the check after the fix failed"
          : "It ran";
  return `${state}. Tool output: ${a.message}`;
}

/** The problem plus the fix just tried, so the AI can say if it worked. */
export function fixQuestion(f: Finding, a: FixAttempt): Question {
  const q = findingQuestion(f);
  q.details["Fix that was tried"] = a.label;
  q.details["Result of the fix"] = attemptResult(a);
  return q;
}

export const checkKey = (a: FixAttempt) => `check:${a.id}`;

export async function ask(key: string, q: Question): Promise<Analysis | null> {
  if (ai.busy[key]) return null;
  ai.busy[key] = true;
  delete ai.errors[key];
  try {
    const a = await api.aiAsk(q);
    ai.answers[key] = a;
    return a;
  } catch (e) {
    ai.errors[key] = String(e);
    return null;
  } finally {
    ai.busy[key] = false;
  }
}

// ---- automatic mode -------------------------------------------------------

const DONE_KEY = "syscura.ai.handled";
const DAY_KEY = "syscura.ai.day";
const DAILY_LIMIT = 20; // stays well inside Gemini's free daily quota

function load<T>(k: string, fallback: T): T {
  try { return JSON.parse(localStorage.getItem(k) ?? "") as T; } catch { return fallback; }
}
function store(k: string, v: unknown) {
  try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* ignore */ }
}

let running = false;

/** Uses one of today's free AI questions; false when none are left. */
function spendOne(): boolean {
  const today = new Date().toISOString().slice(0, 10);
  const day = load<{ date: string; n: number }>(DAY_KEY, { date: today, n: 0 });
  if (day.date !== today) { day.date = today; day.n = 0; }
  if (day.n >= DAILY_LIMIT) return false;
  day.n += 1;
  store(DAY_KEY, day);
  return true;
}

const CHECKED_KEY = "syscura.ai.checked";

/** After a fix (SFC, DISM, a scan...) finishes, asks the AI once whether
 * its result means the problem is fixed, and what to try next if not.
 * Works whether the person or Syscura ran the fix. */
export async function autoCheck(findings: Finding[]) {
  if (running || !ai.status.configured) return;
  const checked = load<Record<string, number>>(CHECKED_KEY, {});
  const dayAgo = Date.now() - 24 * 60 * 60 * 1000;
  for (const f of findings) {
    const a = f.attempts[0];
    if (!a || f.status === "fixing" || a.ts < dayAgo || checked[a.id] || /restore point/i.test(a.label)) continue;
    if (!spendOne()) return;
    checked[a.id] = Date.now();
    store(CHECKED_KEY, checked);
    running = true;
    try {
      const r = await ask(checkKey(a), fixQuestion(f, a));
      if (r) {
        const verdict = r.fixed === "yes" ? "fixed" : r.fixed === "no" ? "not fixed" : "unclear";
        ai.log.unshift({ ts: Date.now(), text: `AI checked "${a.label}" for "${f.title}": ${verdict}.` });
        ai.log.splice(30);
      }
    } finally {
      running = false;
    }
    return; // one at a time: free limits are small
  }
}

/** Looks at one new problem per call: asks the AI, then runs the first
 * safe fix it picked. Riskier fixes stay as suggestions for the person. */
export async function autoStep(findings: Finding[]) {
  if (running || !ai.status.configured || !ai.status.auto_fix) return;
  const handled = load<Record<string, number>>(DONE_KEY, {});
  const today = new Date().toISOString().slice(0, 10);
  const day = load<{ date: string; n: number }>(DAY_KEY, { date: today, n: 0 });
  if (day.date !== today) { day.date = today; day.n = 0; }
  if (day.n >= DAILY_LIMIT) return;

  // Most serious first; skip problems a fix is already handling.
  const order = { critical: 0, error: 1, warning: 2, info: 3, verbose: 4 } as const;
  const next = findings
    .filter((f) => needsAttention(f) && f.attempts.length === 0 && handled[f.id] !== f.count)
    .sort((a, b) => (a.harmful === "yes" ? 0 : 1) - (b.harmful === "yes" ? 0 : 1) || order[a.severity] - order[b.severity])[0];
  if (!next) return;
  running = true;
  try {
    handled[next.id] = next.count;
    store(DONE_KEY, handled);
    day.n += 1;
    store(DAY_KEY, day);
    const a = await ask(`finding:${next.id}`, findingQuestion(next));
    if (!a) {
      ai.log.unshift({ ts: Date.now(), text: `Could not get AI help for "${next.title}".` });
      return;
    }
    const pick = a.actions.find((p) => riskOf(p.action)?.risk === "safe");
    if (pick) {
      const label = riskOf(pick.action)?.label ?? pick.action;
      try {
        await api.applyAction({ finding: next.id, title: next.title, action: pick.action, params: pick.params, label: `AI: ${label}`, automatic: true });
        ai.log.unshift({ ts: Date.now(), text: `Fixed automatically: "${next.title}" → ${label}.` });
      } catch (e) {
        ai.log.unshift({ ts: Date.now(), text: `AI picked "${label}" for "${next.title}", but it could not run: ${e}` });
      }
    } else {
      ai.log.unshift({ ts: Date.now(), text: `Explained "${next.title}". ${a.actions.length ? "Its fixes need your approval." : "It needs steps only you can do."}` });
    }
    ai.log.splice(30);
  } finally {
    running = false;
  }
}
