<script lang="ts">
  // "Ask AI" for one problem or event: explanation, cause, steps for the
  // person, and fixes from Syscura's safe catalog with Apply buttons.
  import { ai, ask, riskOf } from "./ai.svelte";
  import { api } from "./api";
  import { WEB_AIS, askWebAi, searchWeb } from "./web";
  import type { Question } from "./types";

  let {
    qkey,
    question,
    finding = null,
    onapplied,
    search = "",
  }: { qkey: string; question: () => Question; finding?: number | null; onapplied?: () => void; search?: string } = $props();

  let webMsg = $state("");
  async function web(id: string) {
    try { webMsg = await askWebAi(id, question()); } catch (e) { webMsg = String(e); }
  }

  const answer = $derived(ai.answers[qkey]);
  const busy = $derived(!!ai.busy[qkey]);
  const error = $derived(ai.errors[qkey]);
  let msg = $state("");
  let confirm = $state<number | null>(null);

  async function apply(i: number) {
    const p = answer!.actions[i];
    const info = riskOf(p.action);
    if (info && info.risk !== "safe" && confirm !== i) {
      confirm = i;
      return;
    }
    confirm = null;
    try {
      msg = await api.applyAction({
        finding,
        title: question().title,
        action: p.action,
        params: p.params,
        label: `AI: ${info?.label ?? p.action}`,
        automatic: false,
      });
      onapplied?.();
    } catch (e) {
      msg = String(e).replace("agent_offline", "Start protection first: fixes are run by the Syscura agent.");
    }
  }

  const harmText: Record<string, string> = { yes: "Harmful", maybe: "Maybe harmful", no: "Not harmful" };
  const riskText = { safe: "Safe", caution: "Asks first", risky: "Risky" };
</script>

<section class="ai">
  <header>
    <span class="kicker">AI help</span>
    {#if answer}<span class="faint small">{answer.model} with Google Search</span>{/if}
    <span class="grow"></span>
    {#if ai.status.configured}
      <button class="btn btn--sm" disabled={busy} onclick={() => ask(qkey, question())}>
        {busy ? "Searching and thinking…" : answer ? "Ask again" : "Ask AI"}
      </button>
    {/if}
  </header>

  <div class="web">
    <span class="faint small">No key needed:</span>
    <button class="small-btn" onclick={() => searchWeb(search || question().title)}>Search the web</button>
    {#each WEB_AIS as s}<button class="small-btn" onclick={() => web(s.id)}>Ask {s.name}</button>{/each}
  </div>
  {#if webMsg}<p class="faint small">{webMsg}</p>{/if}

  {#if error}<p class="err">{error}</p>{/if}

  {#if answer}
    <p class="summary">{answer.summary}</p>
    <div class="meta">
      <span class="pill pill--{answer.harmful === 'yes' ? 'bad' : answer.harmful === 'no' ? 'ok' : 'warn'}">{harmText[answer.harmful] ?? "Maybe harmful"}</span>
      {#if answer.likely_cause}<span class="muted"><b>Likely cause:</b> {answer.likely_cause}</span>{/if}
    </div>

    {#if answer.actions.length}
      <h5>Fixes Syscura can do</h5>
      <div class="acts">
        {#each answer.actions as p, i}
          {@const info = riskOf(p.action)}
          <div class="act">
            <div class="at">
              <b>{info?.label ?? p.action}</b>
              {#if Object.keys(p.params).length}<span class="mono small">{Object.entries(p.params).map(([k, v]) => `${k}: ${v}`).join(", ")}</span>{/if}
              <span class="muted small">{p.why}</span>
            </div>
            <span class="pill pill--{info?.risk === 'safe' ? 'ok' : info?.risk === 'risky' ? 'bad' : 'warn'}">{info ? riskText[info.risk] : "?"}{info?.undoable ? " · undo" : ""}</span>
            <button class={confirm === i || info?.risk === "safe" ? "btn btn--sm" : "btn btn--ghost btn--sm"} onclick={() => apply(i)}>
              {confirm === i ? "Confirm" : "Apply"}
            </button>
          </div>
          {#if confirm === i}
            <p class="warn small">{info?.description} {info?.risk === "risky" ? "This changes how your PC works." : ""} Press Confirm to run it.</p>
          {/if}
        {/each}
      </div>
    {/if}

    {#if answer.steps.length}
      <h5>Steps for you</h5>
      <ol>{#each answer.steps as s}<li>{s}</li>{/each}</ol>
    {/if}

    {#if answer.sources.length}
      <h5>Sources</h5>
      <ul class="src">
        {#each answer.sources as s}<li><button class="link" onclick={() => api.open(s.url)}>{s.title || s.url}</button></li>{/each}
      </ul>
    {/if}
    {#if msg}<p class="ok small">{msg}</p>{/if}
  {/if}
</section>

<style>
  .ai { background: var(--panel-2); border-radius: var(--r-box); padding: 20px; display: flex; flex-direction: column; gap: 10px; font-size: 14px; }
  header { display: flex; align-items: center; gap: 10px; }
  .grow { flex: 1; }
  .web { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .small { font-size: 13px; }
  .summary { margin: 0; font-size: 15px; }
  .meta { display: flex; gap: 12px; align-items: baseline; flex-wrap: wrap; }
  h5 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); }
  .acts { display: flex; flex-direction: column; gap: 8px; }
  .act { display: flex; gap: 14px; align-items: center; background: var(--panel); border: 1px solid var(--line-2); border-radius: 14px; padding: 12px 16px; }
  .at { flex: 1; display: flex; flex-direction: column; min-width: 0; gap: 2px; }
  .at b { font-weight: 600; }
  .muted.small { color: var(--muted); }
  ol { margin: 0; padding-left: 20px; display: flex; flex-direction: column; gap: 4px; }
  .src { margin: 0; padding-left: 18px; font-size: 13.5px; }
  .link { text-align: left; font-size: 13.5px; }
  .err { color: var(--bad); margin: 0; }
  .warn { color: var(--warn-ink); margin: 0; }
  .ok { color: var(--ok-ink); margin: 0; }
</style>
