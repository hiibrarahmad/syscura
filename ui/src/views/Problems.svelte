<script lang="ts">
  import { onMount } from "svelte";
  import { ago, api } from "../lib/api";
  import { needsAttention } from "../lib/findings";
  import type { Finding, FixOption } from "../lib/types";

  let findings = $state<Finding[]>([]);
  let offline = $state(false);
  let loading = $state(true);
  let showAll = $state(false);
  let showNoise = $state(false);
  let filter = $state("all");
  let open = $state<number | null>(null);
  let message = $state<{ id: number; text: string; bad?: boolean } | null>(null);
  let confirming = $state<{ finding: Finding; fix: FixOption } | null>(null);

  async function load() {
    try {
      findings = await api.findings(showAll);
      offline = false;
    } catch (e) {
      offline = String(e).includes("agent_offline");
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    load();
    const t = setInterval(load, 4000);
    return () => clearInterval(t);
  });

  const SEV: Record<string, number> = { critical: 0, error: 1, warning: 2, info: 3, verbose: 4 };
  const CATS = ["all", "security", "software", "hardware", "storage", "network"];
  const visible = $derived(
    findings
      .filter((f) => (showNoise || f.category !== "noise") && (filter === "all" || f.category === filter))
      .sort((a, b) => SEV[a.severity] - SEV[b.severity] || b.last_ts - a.last_ts),
  );
  const noiseCount = $derived(findings.filter((f) => f.category === "noise").length);
  const counts = $derived({
    harmful: findings.filter((f) => f.harmful === "yes" && f.status !== "ignored").length,
    open: findings.filter(needsAttention).length,
    fixed: findings.filter((f) => f.status === "fixed").length,
  });

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

  const harmText = { yes: "Harmful", maybe: "Maybe harmful", no: "Not harmful" };
  const statusText: Record<string, string> = {
    open: "Needs attention",
    fixing: "Fixing…",
    fixed: "Fixed",
    fix_failed: "Fix did not work",
    ignored: "Ignored",
  };
  const riskText = { safe: "Safe", caution: "Asks first", risky: "Risky" };

  function attemptState(a: Finding["attempts"][number]): string {
    if (!a.ok) return "failed";
    if (a.verified === true) return "worked";
    if (a.verified === false) return "did not hold";
    return "checking…";
  }
</script>

<div class="pb">
  <div class="head">
    <div>
      <h2>Problems</h2>
      <p class="muted">What Windows reported, explained. Safe fixes run by themselves; anything that changes your PC asks first and can be undone.</p>
    </div>
    <div class="stats">
      <div class="stat"><b class:bad={counts.harmful > 0}>{counts.harmful}</b><span>harmful</span></div>
      <div class="stat"><b class:warn={counts.open > 0}>{counts.open}</b><span>need attention</span></div>
      <div class="stat"><b class="ok">{counts.fixed}</b><span>fixed</span></div>
    </div>
  </div>

  <div class="toolbar">
    <div class="seg">
      {#each CATS as c}
        <button class:on={filter === c} onclick={() => (filter = c)}>{c === "all" ? "All" : c[0].toUpperCase() + c.slice(1)}</button>
      {/each}
    </div>
    <label class="check"><input type="checkbox" bind:checked={showNoise} /> Harmless noise ({noiseCount})</label>
    <label class="check"><input type="checkbox" bind:checked={showAll} onchange={load} /> Show fixed &amp; ignored</label>
  </div>

  <div class="list">
    {#if loading}
      <p class="empty muted">Loading…</p>
    {:else if offline}
      <div class="empty card">
        <b>The Syscura agent is not running.</b>
        <p class="muted">It watches Windows in the background. Start it with <span class="mono">syscura-agent run</span>, or install it as a service from an admin terminal: <span class="mono">syscura-agent install</span>.</p>
      </div>
    {:else if visible.length === 0}
      <div class="empty card"><b>Nothing to worry about.</b><p class="muted">No problems in this view.</p></div>
    {:else}
      {#each visible as f (f.id)}
        <article class="item card sev-{f.severity}" class:openitem={open === f.id}>
          <button class="row" onclick={() => (open = open === f.id ? null : f.id)}>
            <span class="bar"></span>
            <span class="main">
              <span class="title">{f.title}{#if f.group && !f.title.includes(f.group)}<span class="grp"> · {f.group.length > 50 ? f.group.slice(0, 49) + "…" : f.group}</span>{/if}</span>
              <span class="expl">{f.explanation}</span>
            </span>
            <span class="tags">
              <span class="harm h-{f.harmful}">{harmText[f.harmful]}</span>
              <span class="st s-{f.status}">{statusText[f.status]}</span>
              <span class="faint small">{f.count}× · {ago(f.last_ts)}</span>
            </span>
          </button>

          {#if open === f.id}
            <div class="body">
              {#if f.advice}<p class="advice">{f.advice}</p>{/if}

              {#if f.fixes.length}
                <h4>Fixes</h4>
                <div class="fixes">
                  {#each f.fixes as fix}
                    <button class="fix r-{fix.risk}" disabled={f.status === "fixing"} onclick={() => runFix(f, fix)}>
                      <span>{fix.label}</span>
                      <small>{riskText[fix.risk]}{fix.undoable ? " · can be undone" : ""}{fix.needs_admin ? " · needs the service" : ""}</small>
                    </button>
                  {/each}
                </div>
              {:else}
                <p class="faint small">No automatic fix for this one. Follow the advice above.</p>
              {/if}

              {#if message?.id === f.id}<p class="msg" class:bad={message.bad}>{message.text}</p>{/if}

              {#if f.attempts.length}
                <h4>What was tried</h4>
                {#each f.attempts as a}
                  <div class="attempt">
                    <span class="as as-{attemptState(a).replace(/[ …]/g, '')}">{attemptState(a)}</span>
                    <span class="faint small">{ago(a.ts)} · {a.automatic ? "automatically" : "by you"}</span>
                    <span class="al">{a.label}</span>
                    <span class="am muted">{a.message}{a.undone ? " (undone)" : ""}</span>
                    {#if a.undo && !a.undone}<button class="small-btn" onclick={() => act(f.id, () => api.undoFix(a.id))}>Undo</button>{/if}
                  </div>
                {/each}
              {/if}

              <h4>Details</h4>
              <dl>
                <dt>First seen</dt><dd>{new Date(f.first_ts).toLocaleString()}</dd>
                <dt>Last seen</dt><dd>{new Date(f.last_ts).toLocaleString()}</dd>
                {#each Object.entries(f.evidence).filter(([k, v]) => v && v.length < 400) as [k, v]}
                  <dt>{k}</dt><dd class="mono">{v}</dd>
                {/each}
              </dl>

              <div class="foot">
                <button class="small-btn" onclick={() => act(f.id, () => api.ignoreFinding(f.id, f.status !== "ignored"))}>
                  {f.status === "ignored" ? "Stop ignoring" : "Ignore this"}
                </button>
              </div>
            </div>
          {/if}
        </article>
      {/each}
    {/if}
  </div>
</div>

{#if confirming}
  {@const c = confirming}
  <div class="overlay" role="presentation" onclick={() => (confirming = null)}>
    <div class="dialog card" role="dialog" aria-modal="true" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && (confirming = null)}>
      <h3>{c.fix.risk === "risky" ? "This changes how your PC works" : "Run this fix?"}</h3>
      <p><b>{c.fix.label}</b></p>
      <p class="muted">
        For: {c.finding.title}{c.finding.group ? ` (${c.finding.group})` : ""}.
        {c.fix.undoable ? "Syscura records an undo step, so you can reverse it." : "Syscura only uses Microsoft's own tools for this; nothing is deleted."}
        {c.fix.risk === "risky" ? " Only do this if you do not recognise what it is about." : ""}
      </p>
      <div class="dlg-btns">
        <button onclick={() => (confirming = null)}>Cancel</button>
        <button class="primary" onclick={() => { act(c.finding.id, () => api.runFix(c.finding.id, c.fix.index)); confirming = null; }}>Run fix</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .pb { display: flex; flex-direction: column; gap: 12px; height: 100%; }
  .head { display: flex; justify-content: space-between; align-items: flex-end; gap: 20px; }
  h2 { margin: 0; font-size: 20px; }
  .head p { margin: 4px 0 0; font-size: 13px; max-width: 640px; }
  .stats { display: flex; gap: 10px; }
  .stat { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 8px 14px; display: flex; flex-direction: column; align-items: center; min-width: 92px; }
  .stat b { font-size: 22px; } .stat span { font-size: 11.5px; color: var(--faint); }
  .bad { color: var(--danger); } .warn { color: var(--warn); } .ok { color: var(--ok); }
  .toolbar { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; }
  .seg { display: flex; background: var(--bg-2); border: 1px solid var(--line); border-radius: 9px; padding: 2px; }
  .seg button { border: none; background: none; padding: 5px 11px; border-radius: 7px; color: var(--muted); font-size: 13px; }
  .seg button.on { background: var(--panel-2); color: var(--text); box-shadow: 0 0 0 1px var(--line-2); }
  .check { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 13px; cursor: pointer; }
  .check input { accent-color: var(--accent); }
  .list { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 8px; padding-right: 4px; }
  .empty { padding: 24px; text-align: center; }
  .empty p { margin: 6px 0 0; }
  .item { overflow: hidden; flex: none; }
  .row { display: flex; gap: 12px; width: 100%; text-align: left; background: none; border: none; border-radius: 0; padding: 12px 14px 12px 0; align-items: flex-start; }
  .bar { width: 4px; align-self: stretch; border-radius: 0 3px 3px 0; background: var(--faint); flex: none; }
  .sev-critical .bar { background: var(--danger); } .sev-error .bar { background: #ff8a5c; }
  .sev-warning .bar { background: var(--warn); } .sev-info .bar { background: var(--accent); }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .title { font-weight: 600; font-size: 14.5px; }
  .grp { color: var(--muted); font-weight: 500; }
  .expl { color: var(--muted); font-size: 13px; overflow: hidden; text-overflow: ellipsis; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; user-select: text; }
  .openitem .expl { -webkit-line-clamp: unset; line-clamp: unset; }
  .tags { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; flex: none; }
  .harm, .st { font-size: 11.5px; font-weight: 600; padding: 2px 9px; border-radius: 99px; white-space: nowrap; }
  .h-yes { background: rgba(255, 92, 108, 0.15); color: var(--danger); }
  .h-maybe { background: rgba(244, 183, 64, 0.15); color: var(--warn); }
  .h-no { background: var(--accent-soft); color: var(--accent); }
  .st { background: var(--bg-2); color: var(--muted); }
  .s-fixed { color: var(--ok); } .s-fix_failed { color: var(--danger); } .s-fixing { color: var(--accent); }
  .small { font-size: 11.5px; }
  .body { padding: 0 16px 14px 20px; display: flex; flex-direction: column; gap: 8px; user-select: text; }
  .advice { margin: 0; background: var(--accent-soft); border-left: 3px solid var(--accent); padding: 8px 10px; border-radius: 6px; font-size: 13px; }
  h4 { margin: 6px 0 0; font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--faint); }
  .fixes { display: flex; gap: 8px; flex-wrap: wrap; }
  .fix { display: flex; flex-direction: column; align-items: flex-start; text-align: left; padding: 7px 12px; gap: 2px; max-width: 360px; }
  .fix small { font-size: 11px; color: var(--faint); }
  .fix.r-safe { border-color: rgba(63, 224, 184, 0.5); }
  .fix.r-risky { border-color: rgba(255, 92, 108, 0.5); }
  .msg { margin: 0; font-size: 13px; color: var(--accent); }
  .msg.bad { color: var(--danger); }
  .attempt { display: grid; grid-template-columns: 96px 150px 1fr auto; gap: 2px 10px; align-items: baseline; font-size: 13px; padding: 4px 0; border-top: 1px solid var(--line); }
  .attempt .am { grid-column: 3 / 5; font-size: 12.5px; overflow-wrap: anywhere; }
  .as { font-weight: 600; font-size: 12px; }
  .as-worked { color: var(--ok); } .as-failed, .as-didnothold { color: var(--danger); } .as-checking { color: var(--warn); }
  dl { display: grid; grid-template-columns: 140px 1fr; gap: 4px 10px; margin: 0; font-size: 12.5px; }
  dt { color: var(--faint); } dd { margin: 0; overflow-wrap: anywhere; }
  .foot { display: flex; justify-content: flex-end; }
  .small-btn { font-size: 12px; padding: 3px 10px; }
  .overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.55); display: grid; place-items: center; z-index: 10; }
  .dialog { width: min(460px, 90vw); padding: 20px; display: flex; flex-direction: column; gap: 8px; }
  .dialog h3 { margin: 0; }
  .dialog p { margin: 0; font-size: 13.5px; }
  .dlg-btns { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
</style>
