<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { api } from "./lib/api";
  import type { HardwareInfo, PartImage, Sensor, StatusInfo } from "./lib/types";
  import Overview from "./views/Overview.svelte";
  import Hardware from "./views/Hardware.svelte";
  import Events from "./views/Events.svelte";
  import Problems from "./views/Problems.svelte";
  import { needsAttention } from "./lib/findings";
  import type { Finding } from "./lib/types";

  type View = "overview" | "problems" | "hardware" | "events";
  let view = $state<View>("overview");
  let hw = $state<HardwareInfo | null>(null);
  let fromAgent = $state(false);
  let status = $state<StatusInfo | null>(null);
  let sensors = $state<Sensor[]>([]);
  let images = $state<PartImage[]>([]);
  let selected = $state<string | null>(null);
  let scanning = $state(false);
  let error = $state("");
  let findings = $state<Finding[]>([]);
  let agentUp = $state(false);
  const problemCount = $derived(findings.filter(needsAttention).length);
  async function refreshProblems() {
    try {
      findings = await api.findings(false);
      agentUp = true;
    } catch {
      findings = [];
      agentUp = false;
    }
  }

  async function scan(refresh: boolean) {
    scanning = true;
    try {
      const v = await api.hardware(refresh);
      hw = v.hw;
      fromAgent = v.from_agent;
      sensors = v.hw.sensors;
      error = "";
      refreshImages();
    } catch (e) {
      error = String(e);
    } finally {
      scanning = false;
    }
  }

  async function refreshStatus() {
    try { status = await api.agentStatus(); } catch { status = null; }
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
  async function refreshImages() {
    try { images = await api.partImages(); } catch { /* ignore */ }
  }

  onMount(() => {
    invoke<string | null>("initial_view").then((v) => {
      if (v === "overview" || v === "problems" || v === "hardware" || v === "events") view = v;
    }).catch(() => {});
    scan(false);
    refreshStatus();
    refreshProblems();
    const a = setInterval(() => { refreshStatus(); refreshProblems(); }, 5000);
    const b = setInterval(refreshSensors, 2000);
    const c = setInterval(() => {
      if (images.some((i) => i.status === "queued" || i.status === "searching") || images.length === 0) refreshImages();
    }, 3000);
    return () => { clearInterval(a); clearInterval(b); clearInterval(c); };
  });

  function openPart(id: string) {
    selected = id;
    view = "hardware";
  }

  const nav: { id: View; label: string; icon: string }[] = [
    { id: "overview", label: "Overview", icon: "M3 12l9-8 9 8M5 10v10h5v-6h4v6h5V10" },
    { id: "problems", label: "Problems", icon: "M12 3l9 16H3zM12 10v4M12 17h.01" },
    { id: "hardware", label: "Hardware", icon: "M4 4h16v16H4zM9 9h6v6H9zM9 1v3M15 1v3M9 20v3M15 20v3M1 9h3M1 15h3M20 9h3M20 15h3" },
    { id: "events", label: "Events", icon: "M4 6h16M4 12h16M4 18h10" },
  ];
</script>

<div class="shell">
  <nav class="side">
    <div class="brand">
      <svg viewBox="0 0 1024 1024" width="30" height="30" aria-hidden="true">
        <rect x="32" y="32" width="960" height="960" rx="220" fill="#0f2a2c" />
        <path d="M512 262l196 72v150c0 132-84 232-196 280-112-48-196-148-196-280V334z" fill="#3fe0b8" />
        <path d="M352 520h88l38-74 58 150 40-104 32 28h64" fill="none" stroke="#062022" stroke-width="34" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>Syscura</span>
    </div>
    {#each nav as n}
      <button class="nav" class:on={view === n.id} onclick={() => (view = n.id)}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d={n.icon} /></svg>
        {n.label}
        {#if n.id === "problems" && problemCount > 0}<span class="badge">{problemCount}</span>{/if}
      </button>
    {/each}
    <div class="grow"></div>
    <div class="agent">
      <span class="dot" class:ok={!!status}></span>
      <span>{status ? "Protection on" : "Agent offline"}</span>
    </div>
  </nav>

  <main>
    {#if error && !hw}
      <div class="card errorbox"><b>Could not read the hardware.</b><p class="muted">{error}</p><button onclick={() => scan(true)}>Try again</button></div>
    {:else if view === "overview"}
      <Overview {hw} {fromAgent} {status} {sensors} {images} {findings} onopen={openPart} onproblems={() => (view = "problems")} />
    {:else if view === "hardware" && hw}
      <Hardware {hw} {sensors} {images} bind:selected onrescan={() => scan(true)} onimages={refreshImages} {scanning} />
    {:else if view === "problems"}
      <Problems />
    {:else if view === "events"}
      <Events />
    {:else}
      <p class="muted">Reading your hardware…</p>
    {/if}
  </main>
</div>

<style>
  .shell { display: flex; height: 100%; }
  .side { width: 200px; flex: none; background: var(--bg-2); border-right: 1px solid var(--line); display: flex; flex-direction: column; gap: 4px; padding: 16px 12px; }
  .brand { display: flex; align-items: center; gap: 10px; font-size: 18px; font-weight: 700; letter-spacing: -0.01em; padding: 0 6px 18px; }
  .nav { display: flex; align-items: center; gap: 10px; background: none; border: 1px solid transparent; text-align: left; padding: 8px 10px; border-radius: 9px; color: var(--muted); }
  .nav:hover { color: var(--text); background: var(--panel); border-color: transparent; }
  .nav.on { background: var(--panel); color: var(--text); border-color: var(--line); }
  .nav.on svg { color: var(--accent); }
  .grow { flex: 1; }
  .badge { margin-left: auto; background: var(--warn); color: #241a00; font-size: 11px; font-weight: 700; border-radius: 99px; padding: 0 7px; line-height: 18px; }
  .agent { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--muted); padding: 8px 10px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--warn); }
  .dot.ok { background: var(--ok); box-shadow: 0 0 8px var(--ok); }
  main { flex: 1; min-width: 0; padding: 18px; overflow: hidden; }
  .errorbox { padding: 20px; max-width: 520px; }
</style>
