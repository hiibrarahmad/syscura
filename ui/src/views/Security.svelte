<script lang="ts">
  import { onMount } from "svelte";
  import { api, ago } from "../lib/api";
  import type { AppUpdate, SecurityCheck, SecurityReport } from "../lib/types";

  let report = $state<SecurityReport | null>(null);
  let error = $state("");
  let msg = $state<Record<string, { text: string; bad?: boolean }>>({});
  let busy = $state<Record<string, boolean>>({});

  async function load(refresh: boolean) {
    try {
      report = await api.security(refresh);
      error = "";
      // A new check runs in the background; ask again shortly.
      if (report.refreshing) setTimeout(() => load(false).then(() => { if (report?.refreshing) setTimeout(() => load(false), 4000); }), 4000);
    } catch (e) {
      error = String(e) === "agent_offline" ? "Background protection is off. Start it on the Overview page to check your security settings." : String(e);
    }
  }

  async function fix(c: SecurityCheck) {
    busy[c.id] = true;
    try {
      const m = await api.applyAction({ finding: null, title: c.title, action: c.action, params: {}, label: c.action_label, automatic: false });
      msg[c.id] = { text: `${m} Syscura checks again in a moment.` };
      setTimeout(() => load(true), 6000);
    } catch (e) {
      msg[c.id] = { text: String(e), bad: true };
    } finally {
      busy[c.id] = false;
    }
  }

  // Ransomware tripwire
  let canary = $state(false);
  let canaryMsg = $state("");
  async function loadOptions() {
    try { canary = (await api.agentOptions())["ransomware_canary"] === "on"; } catch { /* agent off */ }
  }
  async function setCanary(on: boolean) {
    try {
      await api.setAgentOption("ransomware_canary", on ? "on" : "off");
      canary = on;
      canaryMsg = on ? "On. The tripwire files appear within a minute." : "Off. The tripwire files are removed within a minute.";
    } catch (e) {
      canaryMsg = String(e).replace("agent_offline", "Start protection first.");
    }
  }

  // Outdated programs
  let apps = $state<AppUpdate[] | null>(null);
  let appsBusy = $state(false);
  let appsMsg = $state<{ text: string; bad?: boolean } | null>(null);
  async function loadApps() {
    appsBusy = true;
    appsMsg = null;
    try { apps = await api.outdatedApps(); } catch (e) { appsMsg = { text: String(e), bad: true }; } finally { appsBusy = false; }
  }
  async function updateApps(ids: string[]) {
    try { appsMsg = { text: await api.updateApps(ids) }; } catch (e) { appsMsg = { text: String(e), bad: true }; }
  }

  onMount(() => {
    load(false);
    loadOptions();
  });

  const order: Record<string, number> = { bad: 0, warn: 1, unknown: 2, good: 3, info: 4 };
  const sorted = $derived(report ? [...report.checks].sort((a, b) => order[a.status] - order[b.status]) : []);
  const bad = $derived(report?.checks.filter((c) => c.status === "bad").length ?? 0);
  const headline = $derived.by(() => {
    if (!report || !report.checks.length) return { lead: "Checking your", em: "security settings…" };
    if (report.score >= 90) return { lead: "Windows' protection", em: "is switched on." };
    if (bad === 0) return { lead: "Mostly protected.", em: "A few things could be stronger." };
    return { lead: `${bad} protection${bad === 1 ? " is" : "s are"}`, em: "switched off." };
  });
  const pill: Record<string, [string, string]> = {
    good: ["pill--ok", "On"], bad: ["pill--bad", "Fix this"], warn: ["pill--warn", "Could be better"], info: ["pill--quiet", "Optional"], unknown: ["pill--quiet", "Unknown"],
  };
</script>

<div class="sec">
  <section class="panel hero">
    <div class="htext">
      <span class="eyebrow">Security{report?.checked_ms ? ` · checked ${ago(report.checked_ms)}` : ""}</span>
      <h1 class="title-xl">{headline.lead} <span class="accent">{headline.em}</span></h1>
      <span class="muted">Syscura reads Windows' own security settings. Nothing changes unless you press a button, and changes that can be undone show up on the Problems page with an Undo.</span>
    </div>
    {#if report && report.checks.length}
      <div class="score" class:low={report.score < 60} class:mid={report.score >= 60 && report.score < 85}>
        <span class="num">{report.score}</span><span class="of">/ 100</span>
      </div>
    {/if}
  </section>

  {#if error}
    <section class="panel"><p class="muted">{error}</p></section>
  {:else if report}
    <section class="panel list">
      <div class="lhead">
        <span class="kicker">Windows' protection</span>
        <button class="btn btn--ghost btn--sm" disabled={report.refreshing} onclick={() => load(true)}>{report.refreshing ? "Checking…" : "Check again"}</button>
      </div>
      {#each sorted as c (c.id)}
        <div class="row">
          <span class="pill {pill[c.status][0]}">{pill[c.status][1]}</span>
          <div class="body">
            <b>{c.title}</b>
            <span class="muted">{c.detail}</span>
            {#if c.advice && c.status !== "good"}<span class="advice">{c.advice}</span>{/if}
            {#if msg[c.id]}<span class="m" class:bad={msg[c.id].bad}>{msg[c.id].text}</span>{/if}
          </div>
          {#if c.action && c.status !== "good"}
            <button class={c.status === "bad" ? "btn btn--sm" : "btn btn--ghost btn--sm"} disabled={busy[c.id]} onclick={() => fix(c)}>{busy[c.id] ? "Working…" : c.action_label}</button>
          {/if}
        </div>
      {/each}
    </section>
  {:else}
    <p class="muted">Loading…</p>
  {/if}

  <section class="panel list">
    <span class="kicker">Ransomware tripwire</span>
    <p>Syscura puts one small hidden file in your Documents and Pictures folders and checks it every 5 seconds. Nothing has a reason to touch it; ransomware encrypts every file it finds. If it changes, you get a warning at once, while there is still time to unplug the network and your backup drive.</p>
    <label class="check">
      <input type="checkbox" checked={canary} onchange={(e) => setCanary((e.currentTarget as HTMLInputElement).checked)} />
      <span><b>Watch for ransomware</b> <span class="muted">(uses no measurable CPU; turning it off removes the files)</span></span>
    </label>
    {#if canaryMsg}<span class="m">{canaryMsg}</span>{/if}
  </section>

  <section class="panel list">
    <div class="lhead">
      <span class="kicker">Programs with updates</span>
      <button class="btn btn--ghost btn--sm" disabled={appsBusy} onclick={loadApps}>{appsBusy ? "Asking winget…" : apps ? "Check again" : "Check my programs"}</button>
    </div>
    <p class="muted">Old browsers, PDF readers and runtimes are the most common way in for malware. Syscura asks Windows' package manager (winget) which programs have a newer version. Updates run in a window you can watch.</p>
    {#if apps}
      {#if apps.length === 0}
        <p><b>Everything winget knows about is up to date.</b></p>
      {:else}
        <div class="apps">
          {#each apps as a (a.id)}
            <div class="app">
              <span><b>{a.name || a.id}</b> <span class="faint mono">{a.id}</span></span>
              <span class="mono ver">{a.version} → {a.available}</span>
              <button class="btn btn--ghost btn--sm" onclick={() => updateApps([a.id])}>Update</button>
            </div>
          {/each}
        </div>
        <div><button class="btn btn--sm" onclick={() => updateApps(apps!.map((a) => a.id))}>Update all {apps.length}</button></div>
      {/if}
    {/if}
    {#if appsMsg}<span class="m" class:bad={appsMsg.bad}>{appsMsg.text}</span>{/if}
  </section>
</div>

<style>
  .sec { display: flex; flex-direction: column; gap: var(--gap); max-width: 1040px; }
  .hero { flex-direction: row; align-items: center; justify-content: space-between; gap: 32px; padding: 36px 44px; flex-wrap: wrap; }
  .htext { display: flex; flex-direction: column; gap: 14px; flex: 1; min-width: 280px; }
  .score { display: flex; align-items: baseline; gap: 6px; padding: 22px 30px; border-radius: var(--r-box); background: var(--ok-bg); color: var(--ok-ink); }
  .score.mid { background: var(--warn-bg); color: var(--warn-ink); }
  .score.low { background: var(--bad-bg); color: var(--bad-ink); }
  .score .num { font-size: 64px; font-weight: 600; letter-spacing: -0.05em; line-height: 1; font-variant-numeric: tabular-nums; }
  .score .of { font-size: 18px; }
  .list { padding: 30px 40px; gap: 12px; }
  .lhead { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
  .row { display: grid; grid-template-columns: 130px minmax(0, 1fr) auto; gap: 16px; align-items: start; padding: 14px 0; border-top: 1px solid var(--line); }
  .row .pill { justify-self: start; margin-top: 2px; }
  .body { display: flex; flex-direction: column; gap: 3px; font-size: 14.5px; }
  .body b { font-weight: 600; }
  .advice { font-size: 13.5px; color: var(--text); }
  .m { font-size: 13px; color: var(--ok); }
  .m.bad { color: var(--bad); }
  p { margin: 0; font-size: 14.5px; }
  .check { display: flex; gap: 10px; align-items: flex-start; font-size: 14.5px; cursor: pointer; }
  .check input { margin-top: 3px; accent-color: var(--brand); }
  .apps { display: flex; flex-direction: column; }
  .app { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 16px; align-items: center; padding: 10px 0; border-top: 1px solid var(--line); font-size: 14px; }
  .ver { font-size: 12.5px; color: var(--muted); }
  @media (max-width: 760px) { .row { grid-template-columns: 1fr; } .app { grid-template-columns: 1fr; } }
</style>
