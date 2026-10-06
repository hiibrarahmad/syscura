<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { api } from "./lib/api";
  import { ai, autoCheck, autoStep, refreshAi } from "./lib/ai.svelte";
  import { needsAttention } from "./lib/findings";
  import type { Finding, HardwareInfo, Sensor, StatusInfo } from "./lib/types";
  import Overview from "./views/Overview.svelte";
  import Problems from "./views/Problems.svelte";
  import Hardware from "./views/Hardware.svelte";
  import Events from "./views/Events.svelte";
  import Settings from "./views/Settings.svelte";
  import Backup from "./views/Backup.svelte";
  import Processes from "./views/Processes.svelte";
  import { backupFirst, isCritical } from "./lib/warnings";

  type View = "overview" | "problems" | "processes" | "hardware" | "events" | "backup" | "settings";
  let view = $state<View>("overview");
  let hw = $state<HardwareInfo | null>(null);
  let fromAgent = $state(false);
  let status = $state<StatusInfo | null>(null);
  let sensors = $state<Sensor[]>([]);
  let findings = $state<Finding[]>([]);
  let scanning = $state(false);
  let error = $state("");
  const problemCount = $derived(findings.filter(needsAttention).length);
  const critical = $derived(findings.filter(isCritical));
  // The banner is a reminder, not a wall: closing it hides it until a new
  // serious problem appears (or an old one happens again).
  const criticalKey = $derived(critical.map((f) => `${f.id}:${f.count}`).sort().join(","));
  let dismissed = $state(((): string => { try { return localStorage.getItem("syscura.banner.dismissed") ?? ""; } catch { return ""; } })());
  function dismissBanner() {
    dismissed = criticalKey;
    try { localStorage.setItem("syscura.banner.dismissed", criticalKey); } catch { /* fine */ }
  }

  async function scan(refresh: boolean) {
    scanning = true;
    try {
      const v = await api.hardware(refresh);
      hw = v.hw;
      fromAgent = v.from_agent;
      sensors = v.hw.sensors;
      error = "";
    } catch (e) {
      error = String(e);
    } finally {
      scanning = false;
    }
  }

  async function refreshStatus() {
    try { status = await api.agentStatus(); } catch { status = null; }
  }
  async function refreshProblems() {
    try {
      findings = await api.findings(false);
      autoCheck(findings);
    } catch { findings = []; }
  }
  async function refreshSensors() {
    if (!hw) return;
    try {
      const live = await api.liveSensors();
      // Keep slow-changing readings from the scan (drives), replace live ones.
      const liveSources = new Set(live.map((s) => s.source));
      sensors = [...hw.sensors.filter((s) => !liveSources.has(s.source)), ...live];
    } catch { /* keep last readings */ }
  }

  onMount(() => {
    invoke<string | null>("initial_view").then((v) => {
      if (nav.some((n) => n.id === v)) view = v as View;
    }).catch(() => {});
    // The tray menu's "Show problems".
    const onView = (e: Event) => { const v = (e as CustomEvent<string>).detail; if (nav.some((n) => n.id === v)) go(v as View); };
    window.addEventListener("syscura-view", onView);
    // Fresh hardware read on every start, so the report is always current.
    scan(true);
    refreshStatus();
    refreshProblems();
    refreshAi();
    const a = setInterval(() => { refreshStatus(); refreshProblems(); }, 5000);
    const b = setInterval(refreshSensors, 2000);
    // Automatic AI help: at most one problem a minute (free limits are small).
    const c = setInterval(() => autoStep(findings).then(refreshProblems), 60000);
    return () => { clearInterval(a); clearInterval(b); clearInterval(c); window.removeEventListener("syscura-view", onView); };
  });

  const nav: { id: View; label: string }[] = [
    { id: "overview", label: "Overview" },
    { id: "problems", label: "Problems" },
    { id: "processes", label: "Processes" },
    { id: "hardware", label: "Hardware" },
    { id: "events", label: "Events" },
    { id: "backup", label: "Backup" },
    { id: "settings", label: "Settings" },
  ];
  const go = (v: View) => { view = v; window.scrollTo({ top: 0 }); };
</script>

<div class="app">
  <header class="topbar">
    <div class="brand"><img src="/logo.svg" alt="" />Syscura</div>
    <nav class="tabs" aria-label="Main">
      {#each nav as n}
        <button aria-current={view === n.id ? "page" : undefined} onclick={() => go(n.id)}>
          {n.label}{#if n.id === "problems" && problemCount > 0}<span class="count">&nbsp;· {problemCount}</span>{/if}
        </button>
      {/each}
    </nav>
    <span class="status" title={ai.status.configured ? (ai.status.auto_fix ? "AI fixes safe problems automatically" : "AI suggests fixes") : "AI help is not set up"}>
      <span class="dot" class:off={!status}></span>{status ? "Protection on" : "Protection off"}
    </span>
  </header>

  {#if critical.length && view !== "backup" && dismissed !== criticalKey}
    <section class="critical">
      <span class="pill pill--bad">{critical.length === 1 ? "Serious problem" : `${critical.length} serious problems`}</span>
      <div class="ctext">
        <b>{critical.length === 1 ? critical[0].title : critical.map((c) => c.title).slice(0, 2).join(" · ")}</b>
        <span class="muted">{critical.some(backupFirst) ? "Back up your important files now, then follow the steps on the Problems page." : "See the Problems page for what it means and what to do."}</span>
      </div>
      <button class="btn" onclick={() => go("problems")}>What to do</button>
      {#if critical.some(backupFirst)}<button class="btn btn--ghost" onclick={() => go("backup")}>Back up files</button>{/if}
      <button class="close" title="Close. It comes back only for a new serious problem." aria-label="Close" onclick={dismissBanner}>✕</button>
    </section>
  {/if}

  {#if error && !hw}
    <section class="panel"><b>Could not read the hardware.</b><p class="muted">{error}</p><button onclick={() => scan(true)}>Try again</button></section>
  {:else if view === "overview"}
    <Overview {hw} {fromAgent} {status} {sensors} {findings}
      onproblems={() => go("problems")} onhardware={() => go("hardware")} onsettings={() => go("settings")} />
  {:else if view === "problems"}
    <Problems />
  {:else if view === "processes"}
    <Processes />
  {:else if view === "hardware" && hw}
    <Hardware {hw} {sensors} {fromAgent} onrescan={() => scan(true)} {scanning} />
  {:else if view === "events"}
    <Events />
  {:else if view === "backup"}
    <Backup />
  {:else if view === "settings"}
    <Settings />
  {:else}
    <p class="muted">Reading your PC…</p>
  {/if}
</div>

<style>
  .app { max-width: 1440px; margin: 0 auto; }
  .brand img { width: 30px; height: 30px; }
  .dot.off { background: var(--warn); }
  .critical { display: flex; align-items: center; gap: 16px; flex-wrap: wrap; background: var(--panel); border-radius: var(--r-panel); padding: 18px 24px; box-shadow: inset 0 0 0 1px var(--bad-bg); }
  .ctext { flex: 1; min-width: 240px; display: flex; flex-direction: column; gap: 2px; font-size: 14px; }
  .ctext b { font-weight: 600; font-size: 15.5px; }
  .close { border: 0; background: none; color: var(--muted); font-size: 16px; padding: 6px 10px; }
  .close:hover { color: var(--text); background: var(--panel-2); }
</style>
