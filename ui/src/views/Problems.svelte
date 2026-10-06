<script lang="ts">
  import { onMount } from "svelte";
  import { ago, api } from "../lib/api";
  import { needsAttention } from "../lib/findings";
  import AiPanel from "../lib/AiPanel.svelte";
  import { ai, ask, checkKey, findingQuestion, fixQuestion } from "../lib/ai.svelte";
  import { nav } from "../lib/nav.svelte";
  import { askWebAi, searchWeb } from "../lib/web";
  import type { Finding, FixOption } from "../lib/types";

  let findings = $state<Finding[]>([]);
  let offline = $state(false);
  let loading = $state(true);
  let showClosed = $state(false);
  let filter = $state("all");
  let message = $state<{ id: number; text: string; bad?: boolean } | null>(null);
  let confirming = $state<{ finding: Finding; fix: FixOption } | null>(null);

  async function load() {
    try {
      findings = await api.findings(showClosed);
      offline = false;
    } catch (e) {
      offline = String(e).includes("agent_offline");
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    load().then(() => requestAnimationFrame(() => window.scrollTo({ top: 0 })));
    const t = setInterval(load, 4000);
    return () => clearInterval(t);
  });

  const SEV: Record<string, number> = { critical: 0, error: 1, warning: 2, info: 3, verbose: 4 };
  const CATS = ["all", "security", "software", "hardware", "storage", "network"];
  const order = (a: Finding, b: Finding) =>
    (a.harmful === "yes" ? 0 : 1) - (b.harmful === "yes" ? 0 : 1) || SEV[a.severity] - SEV[b.severity] || b.last_ts - a.last_ts;
  const shown = $derived(findings.filter((f) => filter === "all" || f.category === filter).sort(order));
  const groups = $derived([
    { id: "needs", title: "Needs you", items: shown.filter(needsAttention) },
    { id: "handling", title: "Syscura is handling", items: shown.filter((f) => f.status === "fixing") },
    { id: "fixed", title: "Fixed", items: shown.filter((f) => f.status === "fixed") },
    { id: "quiet", title: "Safe to ignore", items: shown.filter((f) => f.status === "open" && !needsAttention(f)) },
    { id: "ignored", title: "Ignored by you", items: shown.filter((f) => f.status === "ignored") },
  ].filter((g) => g.items.length));

  // The selected problem; falls back to the most important one.
  const selected = $derived(findings.find((f) => f.id === nav.problem) ?? groups[0]?.items[0] ?? null);

  async function act(id: number, fn: () => Promise<string>) {
    try {
      message = { id, text: await fn() };
    } catch (e) {
      message = { id, text: String(e).replace("agent_offline", "The Syscura agent is not running."), bad: true };
    }
    load();
  }

  function runFix(f: Finding, fix: FixOption) {
    if (fix.risk === "safe") act(f.id, () => api.runFix(f.id, fix.index));
    else confirming = { finding: f, fix };
  }

  /** The AI's verdict wins over the rule's default once it has answered. */
  function harm(f: Finding): { value: string; byAi: boolean } {
    if (f.verdict_by) return { value: f.harmful, byAi: f.verdict_by === "ai" };
    const a = ai.answers[`finding:${f.id}`];
    return a ? { value: a.harmful, byAi: true } : { value: f.harmful, byAi: false };
  }

  function verdict(f: Finding): { text: string; tone: string } {
    const h = harm(f).value;
    if (f.verdict_by === "you" && f.harmful === "no" && f.status !== "fixing") return { text: "Marked safe by you", tone: "ok" };
    if (f.status === "open" && workedBefore(f) && f.category !== "security") return { text: "Known problem · fixed before", tone: "brand" };
    if (f.status === "fixing") return { text: "Fixing now", tone: "brand" };
    if (f.status === "fixed") return { text: "Fixed", tone: "ok" };
    if (f.status === "ignored") return { text: "Ignored", tone: "quiet" };
    if (f.status === "fix_failed") return { text: h === "yes" ? "Harmful · fix did not work" : "Fix did not work", tone: "bad" };
    if (h === "yes") return { text: "Harmful · needs you", tone: "bad" };
    if (h === "no") return { text: "Not harmful", tone: needsAttention(f) ? "quiet" : "ok" };
    return { text: needsAttention(f) ? "Maybe harmful · needs a look" : "Maybe harmful", tone: "warn" };
  }

  function ring(f: Finding): string {
    if (f.status === "fixing") return "watch";
    if (f.status === "fixed") return "ok";
    if (needsAttention(f)) return "bad";
    return "quiet";
  }

  function when(ms: number): string {
    const d = new Date(ms);
    return d.toDateString() === new Date().toDateString()
      ? d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
      : d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }

  const harmAnswer: Record<string, string> = {
    yes: "Yes.",
    maybe: "Maybe.",
    no: "No.",
  };
  const harmLine: Record<string, string> = {
    yes: "Act on it soon. Follow the steps and fixes below.",
    maybe: "It depends on the cause. The steps below tell you how to check.",
    no: "Windows logs this on many healthy PCs. You can leave it.",
  };
  const riskText = { safe: "Safe, runs right away", caution: "Asks first", risky: "Changes how your PC works" };

  /** Files a problem is about: Defender's detection paths
   * ("containerfile:_D:\x.iso; file:_D:\x.iso->(Rar)...") or the program
   * Syscura flagged. Only full paths that look like files. */
  function affectedFiles(f: Finding): string[] {
    const raw = f.evidence["Path"] ?? "";
    const out = new Set<string>();
    for (let part of raw.split(";")) {
      part = part.trim().replace(/^[a-z]+:_/i, "").split("->")[0].trim();
      if (/^[A-Za-z]:\\/.test(part) && !part.endsWith("\\")) out.add(part);
    }
    return [...out].slice(0, 8);
  }

  let deleting = $state<{ finding: Finding; path: string } | null>(null);
  let fileMsg = $state<{ path: string; text: string; bad?: boolean } | null>(null);

  async function reveal(path: string) {
    try { await api.reveal(path); } catch (e) { fileMsg = { path, text: `Could not open the folder: ${e}`, bad: true }; }
  }
  async function recycle(path: string) {
    deleting = null;
    try { fileMsg = { path, text: await api.recycleFile(path) }; } catch (e) { fileMsg = { path, text: String(e), bad: true }; }
  }

  /** The fix that solved this problem before, if it came back. */
  function workedBefore(f: Finding) {
    const a = f.attempts.find((x) => x.verified === true && x.fix_index < f.fixes.length);
    return a ? f.fixes[a.fix_index] : null;
  }

  function attemptState(a: Finding["attempts"][number]): string {
    if (!a.ok) return "failed";
    if (a.outcome === "nothing_found") return "found nothing wrong";
    if (a.outcome === "not_repaired") return "could not repair it";
    if (a.outcome === "unclear") return "result unclear";
    if (a.verified === true) return "worked";
    if (a.verified === false) return "did not hold";
    return "checking it stays fixed";
  }
</script>

<div class="row-chips">
  {#each CATS as c}
    <button class="chip" aria-pressed={filter === c} onclick={() => (filter = c)}>{c === "all" ? "Everything" : c[0].toUpperCase() + c.slice(1)}</button>
  {/each}
  <button class="chip" style="margin-left: auto" aria-pressed={showClosed} onclick={() => { showClosed = !showClosed; load(); }}>Include old fixes and ignored</button>
</div>

{#if loading}
  <p class="muted">Loading…</p>
{:else if offline}
  <section class="panel">
    <h1 class="title-l">Background protection is off.</h1>
    <p class="muted">The Syscura agent watches Windows and runs fixes. Press <b>Start</b> on the Overview page.</p>
  </section>
{:else if !selected}
  <section class="panel">
    <h1 class="title-l">Nothing to worry about.</h1>
    <p class="muted">No problems in this view.</p>
  </section>
{:else}
  <div class="split">
    <section class="panel list" style="padding: 28px 24px; gap: 22px">
      {#each groups as g (g.id)}
        <div class="group">
          <div class="group__head"><span>{g.title}</span><span>{g.items.length}</span></div>
          {#each g.items as f (f.id)}
            <button class="issue" aria-current={selected.id === f.id ? "true" : undefined} onclick={() => (nav.problem = f.id)}>
              <span class="ring ring--{ring(f)}">
                {#if ring(f) === "watch"}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 6v6l4 2" /></svg>{/if}
                {#if ring(f) === "ok"}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5L19 7" /></svg>{/if}
              </span>
              <span class="it">
                <b>{f.title}</b>
                <small>{f.group && !f.title.includes(f.group) ? f.group : f.explanation}</small>
              </span>
              <small class="t">{f.count > 1 ? `${f.count}× · ` : ""}{when(f.last_ts)}</small>
            </button>
          {/each}
        </div>
      {/each}
    </section>

    {#key selected.id}
    {@const f = selected}
    {@const v = verdict(f)}
    {@const h = harm(f)}
    <section class="panel detail" style="gap: 26px">
      <div style="display: flex; flex-direction: column; gap: 12px">
        <span class="pill pill--{v.tone}" style="align-self: flex-start; font-weight: 600; font-size: 13px; padding: 5px 14px">{v.text}{h.byAi && f.status === "open" ? " · checked by AI" : ""}</span>
        <h1 class="title-l">{f.title}</h1>
        <span class="muted" style="font-size: 14px">
          {f.count === 1 ? `Seen ${ago(f.last_ts)}` : `Seen ${f.count} times since ${new Date(f.first_ts).toLocaleDateString()} · last ${ago(f.last_ts)}`}
          {#if f.group && !f.title.includes(f.group)} · {f.group.length > 70 ? f.group.slice(0, 69) + "…" : f.group}{/if}
        </span>
      </div>

      <div class="qa">
        <div class="box"><span class="kicker">What happened</span><span>{f.explanation}</span></div>
        <div class="box"><span class="kicker">Is it harmful?</span><span><b>{harmAnswer[h.value] ?? "Maybe."}</b> {h.byAi ? (ai.answers[`finding:${f.id}`]?.summary ?? harmLine[h.value]) : harmLine[h.value]}</span></div>
        <div class="box"><span class="kicker">What to do</span><span>{f.advice || (f.fixes.length ? "Try the fixes below, starting with the first." : "Ask the AI or search the web below.")}</span></div>
      </div>

      {#if f.status === "fixing"}
        <p class="running"><span class="pill pill--brand">Working</span> A fix is running. Scans and repairs like SFC and DISM take 5–30 minutes. This page updates by itself.</p>
      {/if}

      {#each f.attempts.slice(0, 1) as last}
        {#if f.status !== "fixing" && (last.outcome === "nothing_found" || last.outcome === "not_repaired" || last.outcome === "unclear" || last.outcome === "failed" || last.verified === false)}
          <div class="box next">
            <span class="kicker" style="color: var(--warn-ink)">
              {last.outcome === "nothing_found" ? `“${last.label}” found nothing wrong, so it was not the cause` : last.outcome === "unclear" ? `“${last.label}” finished, but its result could not be read` : `“${last.label}” did not solve it`}
            </span>
            <span>{last.message}</span>
            <span class="muted">What next: try the next fix below, ask the AI, or search the web for this exact problem.</span>
            <div class="row-chips">
              <button class="btn btn--sm" onclick={() => searchWeb(f.search || f.title)}>Search the web</button>
              <button class="btn btn--ghost btn--sm" onclick={() => askWebAi("google", fixQuestion(f, last))}>Ask Google AI if it worked</button>
              <button class="btn btn--ghost btn--sm" onclick={() => askWebAi("chatgpt", fixQuestion(f, last))}>Ask ChatGPT</button>
            </div>
          </div>
        {/if}
        {#if f.status !== "fixing" && !/restore point/i.test(last.label)}
          {@const check = ai.answers[checkKey(last)]}
          {@const busy = !!ai.busy[checkKey(last)]}
          {@const err = ai.errors[checkKey(last)]}
          {#if check || busy || err || ai.status.configured}
            <div class="box aicheck">
              <div class="row-chips">
                <span class="kicker">AI check of “{last.label}”</span>
                {#if check?.fixed}
                  <span class="pill pill--{check.fixed === 'yes' ? 'ok' : check.fixed === 'no' ? 'bad' : 'warn'}">{check.fixed === "yes" ? "Fixed" : check.fixed === "no" ? "Not fixed" : "Not sure"}</span>
                {/if}
                {#if ai.status.configured}
                  <button class="btn btn--ghost btn--sm" style="margin-left: auto" disabled={busy} onclick={() => ask(checkKey(last), fixQuestion(f, last))}>
                    {busy ? "Checking…" : check ? "Check again" : "Ask AI if it worked"}
                  </button>
                {/if}
              </div>
              {#if busy && !check}<span class="muted">The AI is reading the result and searching the web…</span>{/if}
              {#if err}<span class="bad">{err}</span>{/if}
              {#if check}
                <span>{check.summary}</span>
                {#if check.likely_cause}<span class="muted"><b>Likely cause:</b> {check.likely_cause}</span>{/if}
                {#if check.steps.length}<ol>{#each check.steps as s}<li>{s}</li>{/each}</ol>{/if}
                {#if check.fixed === "no"}<span class="muted">Press <b>Ask AI</b> below for the next fix Syscura can run.</span>{/if}
              {/if}
            </div>
          {/if}
        {/if}
      {/each}

      {#each [affectedFiles(f)] as files}
      {#if files.length && f.harmful !== "no"}
        <div class="box files">
          <span class="kicker">{files.length === 1 ? "The file" : "The files"}</span>
          {#each files as path}
            <div class="file">
              <span class="mono path">{path}</span>
              <div class="row-chips">
                <button class="btn btn--ghost btn--sm" onclick={() => reveal(path)}>Open folder</button>
                <button class="btn btn--ghost btn--sm danger" onclick={() => (deleting = { finding: f, path })}>Delete…</button>
              </div>
              {#if fileMsg?.path === path}<span class="msg" class:bad={fileMsg.bad}>{fileMsg.text}</span>{/if}
            </div>
          {/each}
          <span class="muted small">Deleting moves the file to the Recycle Bin, so you can restore it. For files Defender is holding, use “Let Defender remove the threats it found” below.</span>
        </div>
      {/if}
      {/each}

      {#each [f.status === "open" || f.status === "fix_failed" ? workedBefore(f) : null] as again}
      {#if again}
        <div class="fix again">
          <div><b>Fix again: {again.label}</b><small>This fixed the same problem before. It came back, so running it again usually works.</small></div>
          <button class="btn" onclick={() => runFix(f, again)}>Fix again</button>
        </div>
      {/if}
      {/each}

      {#if f.fixes.length}
        <div style="display: flex; flex-direction: column; gap: 10px">
          {#each f.fixes as fix}
            <div class="fix">
              <div><b>{fix.label}</b><small>{riskText[fix.risk]}{fix.undoable ? " · can be undone" : ""}{fix.needs_admin ? " · run by the Syscura service" : ""}</small></div>
              <button class={fix.risk === "safe" ? "btn" : "btn btn--ghost"} disabled={f.status === "fixing"} onclick={() => runFix(f, fix)}>Run</button>
            </div>
          {/each}
        </div>
      {/if}
      {#if message?.id === f.id}<p class="msg" class:bad={message.bad}>{message.text}</p>{/if}

      <AiPanel qkey="finding:{f.id}" question={() => (f.attempts[0] ? fixQuestion(f, f.attempts[0]) : findingQuestion(f))} finding={f.id} onapplied={load} search={f.search} />

      {#if f.message}
        <details class="box">
          <summary>What Windows wrote</summary>
          <pre class="mono">{f.message}</pre>
        </details>
      {/if}

      <div class="history">
        <span class="muted" style="font-weight: 600">What Syscura did</span>
        <div><time>{when(f.first_ts)}</time><span>Noticed this{f.count > 1 ? `, and saw it ${f.count} times so far` : ""}</span></div>
        {#each [...f.attempts].reverse() as a}
          <div>
            <time>{when(a.ts)}</time>
            <span>
              {a.automatic ? "Ran" : "You ran"} <b>{a.label}</b>: {attemptState(a)}{a.undone ? " (undone)" : ""}.
              <span class="muted">{a.message.length > 240 ? a.message.slice(0, 239) + "…" : a.message}</span>
              {#if a.undo && !a.undone}<button class="link" onclick={() => act(f.id, () => api.undoFix(a.id))}>Undo</button>{/if}
            </span>
          </div>
        {/each}
        {#if !f.attempts.length && f.fixes.some((x) => x.risk !== "safe")}
          <div><time>—</time><span>Did not run the riskier fixes by itself: they wait for you.</span></div>
        {/if}
      </div>

      <details class="box">
        <summary>Technical details</summary>
        <dl class="specs" style="margin-top: 10px">
          <div><dt>First seen</dt><dd>{new Date(f.first_ts).toLocaleString()}</dd></div>
          <div><dt>Last seen</dt><dd>{new Date(f.last_ts).toLocaleString()}</dd></div>
          {#each Object.entries(f.evidence).filter(([, val]) => val && val.length < 400) as [k, val]}
            <div><dt>{k}</dt><dd class="mono">{val}</dd></div>
          {/each}
        </dl>
      </details>

      <div class="row-chips">
        {#if f.verdict_by === "you"}
          <button class="btn btn--ghost btn--sm" onclick={() => act(f.id, () => api.setVerdict(f.id, "", "you"))}>Forget my verdict</button>
        {:else}
          <button class="btn btn--ghost btn--sm" title="You know this and trust it: Syscura remembers and will not warn about it again." onclick={() => act(f.id, () => api.setVerdict(f.id, "no", "you"))}>Mark as safe</button>
          {#if f.harmful !== "yes"}<button class="btn btn--ghost btn--sm" onclick={() => act(f.id, () => api.setVerdict(f.id, "yes", "you"))}>This is harmful</button>{/if}
        {/if}
        <button class="btn btn--ghost btn--sm" onclick={() => act(f.id, () => api.ignoreFinding(f.id, f.status !== "ignored"))}>
          {f.status === "ignored" ? "Stop ignoring" : "Ignore this"}
        </button>
      </div>
    </section>
    {/key}
  </div>
{/if}

{#if deleting}
  {@const d = deleting}
  <div class="overlay" role="presentation" onclick={() => (deleting = null)}>
    <div class="panel dialog" role="dialog" aria-modal="true" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && (deleting = null)}>
      <span class="pill pill--bad" style="align-self: flex-start">Delete a file</span>
      <span class="title-m">Move this file to the Recycle Bin?</span>
      <p class="mono path">{d.path}</p>
      <p class="muted">Only do this if you do not need the file. If it is part of a program you use, that program may stop working. The file goes to the Recycle Bin, so you can restore it from there. If it is an installer or archive you downloaded, deleting it is usually the right move.</p>
      <div class="row-chips" style="justify-content: flex-end">
        <button class="btn btn--ghost" onclick={() => (deleting = null)}>Cancel</button>
        <button class="btn danger-solid" onclick={() => recycle(d.path)}>Move to Recycle Bin</button>
      </div>
    </div>
  </div>
{/if}

{#if confirming}
  {@const c = confirming}
  <div class="overlay" role="presentation" onclick={() => (confirming = null)}>
    <div class="panel dialog" role="dialog" aria-modal="true" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && (confirming = null)}>
      <span class="pill pill--{c.fix.risk === 'risky' ? 'bad' : 'warn'}" style="align-self: flex-start">{c.fix.risk === "risky" ? "Changes how your PC works" : "Asks first"}</span>
      <span class="title-m">{c.fix.label}</span>
      <p class="muted">
        For: {c.finding.title}{c.finding.group ? ` (${c.finding.group})` : ""}.
        {c.fix.undoable ? "Syscura records an undo step, so you can reverse it." : "Syscura only uses Microsoft's own tools for this. Nothing is deleted."}
        A restore point is made first when Windows allows it.
        {c.fix.risk === "risky" ? " Only do this if you do not recognise what it is about." : ""}
      </p>
      <div class="row-chips" style="justify-content: flex-end">
        <button class="btn btn--ghost" onclick={() => (confirming = null)}>Cancel</button>
        <button class="btn" onclick={() => { act(c.finding.id, () => api.runFix(c.finding.id, c.fix.index)); confirming = null; }}>Run fix</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .list { align-self: start; }
  .detail { align-self: start; }
  .it { min-width: 0; }
  .it small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .t { white-space: nowrap; font-variant-numeric: tabular-nums; }
  .qa .box span:last-child { overflow-wrap: anywhere; }
  .running { margin: 0; display: flex; gap: 10px; align-items: center; font-size: 14px; color: var(--muted); }
  .next { display: flex; flex-direction: column; gap: 8px; background: var(--warn-bg); font-size: 14px; }
  .fix .btn { min-width: 74px; }
  .files { display: flex; flex-direction: column; gap: 10px; }
  .file { display: flex; flex-direction: column; gap: 6px; padding-top: 8px; border-top: 1px solid var(--line); }
  .path { overflow-wrap: anywhere; font-size: 12.5px; }
  .small { font-size: 13px; }
  .danger { color: var(--bad-ink); }
  .danger-solid { background: var(--bad); color: #fff; }
  .danger-solid:hover { background: var(--bad-ink); }
  .again { border-color: var(--brand); background: var(--brand-soft); }
  .aicheck { display: flex; flex-direction: column; gap: 8px; font-size: 14px; }
  .aicheck ol { margin: 0; padding-left: 20px; display: flex; flex-direction: column; gap: 4px; }
  .bad { color: var(--bad); }
  .msg { margin: 0; font-size: 14px; color: var(--brand); }
  .msg.bad { color: var(--bad); }
  details summary { cursor: pointer; font-weight: 600; font-size: 14px; color: var(--muted); }
  pre { margin: 10px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; max-height: 220px; overflow-y: auto; color: var(--muted); }
  .history span b { font-weight: 600; }
  .history .link { margin-left: 6px; font-size: 13px; }
  .overlay { position: fixed; inset: 0; background: rgba(14, 13, 31, 0.45); display: grid; place-items: center; z-index: 10; }
  .dialog { width: min(500px, 92vw); padding: 30px 32px; gap: 14px; }
  .dialog p { margin: 0; font-size: 14px; }
</style>
