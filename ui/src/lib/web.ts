// Help that needs no key: a web search, or a free AI website the person
// opens in their own browser with their own account. Syscura only prepares
// the (privacy-masked) question; it never controls those websites.

import { api } from "./api";
import type { Question } from "./types";

export const WEB_AIS = [
  { id: "gemini", name: "Gemini", url: (_q: string) => "https://gemini.google.com/app" },
  { id: "chatgpt", name: "ChatGPT", url: (q: string) => `https://chatgpt.com/?q=${encodeURIComponent(q)}` },
  { id: "claude", name: "Claude", url: (q: string) => `https://claude.ai/new?q=${encodeURIComponent(q)}` },
  { id: "copilot", name: "Copilot", url: (q: string) => `https://copilot.microsoft.com/?q=${encodeURIComponent(q)}` },
  { id: "google", name: "Google AI Mode", url: (q: string) => `https://www.google.com/search?udm=50&q=${encodeURIComponent(q)}` },
] as const;

/** Opens a normal web search for the problem. */
export function searchWeb(query: string) {
  return api.open(`https://www.google.com/search?q=${encodeURIComponent(query)}`);
}

/** Copies the full question, then opens the AI site (pre-filled where the
 * site supports it; otherwise the person pastes with Ctrl+V). */
export async function askWebAi(id: string, q: Question): Promise<string> {
  const site = WEB_AIS.find((s) => s.id === id)!;
  const prompt = await api.webPrompt(q);
  try { await navigator.clipboard.writeText(prompt); } catch { /* clipboard blocked */ }
  // Very long links are cut by some sites; the clipboard always has the full text.
  const forUrl = prompt.length > 1800 ? prompt.slice(0, 1800) : prompt;
  await api.open(site.url(forUrl));
  return id === "gemini"
    ? "Question copied. Gemini opened: paste it with Ctrl+V and press Enter."
    : `${site.name} opened with your question. If it isn't filled in, paste it with Ctrl+V.`;
}
