<script lang="ts">
  import { ai } from "../lib/ai.svelte";
  import { needsAttention } from "../lib/findings";
  import { nav } from "../lib/nav.svelte";
  import { api, ago, driveSize, duration, gb, mb } from "../lib/api";
  import SocialLinks from "../lib/SocialLinks.svelte";
  import type { Finding, HardwareInfo, SecurityReport, Sensor, StatusInfo, Summary } from "../lib/types";
  import { onMount } from "svelte";

  let {
    hw,
    fromAgent,
    status,
    sensors,
    findings,
    onproblems,
    onhardware,
    onsettings,
    onsecurity,
  }: {
    hw: HardwareInfo | null;
    fromAgent: boolean;
    status: StatusInfo | null;
    sensors: Sensor[];
    findings: Finding[];
    onproblems: () => void;
    onhardware: () => void;
    onsettings: () => void;
    onsecurity: () => void;
  } = $props();

  let week = $state<Summary | null>(null);
  let security = $state<SecurityReport | null>(null);
  onMount(() => {
    api.summary(7).then((s) => (week = s)).catch(() => {});
    api.security(false).then((r) => (security = r.checks.length ? r : null)).catch(() => {});
  });

  const find = (at: string, kind: string) => sensors.find((s) => s.site.at === at && s.kind === kind);
  const cpuTemp = $derived(find("cpu", "temperature"));
  const cpuLoad = $derived(find("cpu", "load"));
  const gpuTemp = $derived(find("gpu", "temperature"));
  const gpuLoad = $derived(find("gpu", "load"));

  const SEV: Record<string, number> = { critical: 0, error: 1, warning: 2, info: 3, verbose: 4 };
  const attention = $derived(
    findings
      .filter(needsAttention)
      .sort((a, b) => (a.harmful === "yes" ? 0 : 1) - (b.harmful === "yes" ? 0 : 1) || SEV[a.severity] - SEV[b.severity]),
  );
  const handling = $derived(findings.filter((f) => f.status === "fixing"));
  const fixed = $derived(findings.filter((f) => f.status === "fixed"));
  const routine = $derived(findings.filter((f) => f.status === "open" && !needsAttention(f)));
  const badDisks = $derived(hw?.disks.filter((d) => d.health !== "Healthy" && d.health !== "Unknown") ?? []);
  const lowSpace = $derived(hw?.volumes.filter((v) => !v.removable && v.size_bytes > 0 && v.free_bytes / v.size_bytes < 0.1) ?? []);
  const ram = $derived(hw?.memory.sticks.reduce((a, s) => a + s.capacity_bytes, 0) || hw?.memory.usable_bytes || 0);

  const words = ["No", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine"];
  const headline = $derived.by(() => {
    if (!status) return { lead: "Protection is off.", em: "Start it to watch your PC." };
    const n = attention.length + badDisks.length;
    const serious = attention.some((f) => f.harmful === "yes") || badDisks.length > 0;
    const lead = serious ? "Your PC needs attention." : n ? "Your PC is mostly healthy." : "Your PC is healthy.";
    return { lead, em: n === 0 ? "Nothing needs you." : `${words[n] ?? n} ${n === 1 ? "thing needs" : "things need"} you.` };
  });

  const name = $derived(
    hw
      ? (hw.system.model ? [hw.system.manufacturer, hw.system.model].filter(Boolean).join(" ") : "") ||
          [hw.board.manufacturer, hw.board.product].filter(Boolean).join(" ") ||
          "This PC"
      : "",
  );
  const today = new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  const drives = $derived.by(() => {
    if (!hw) return "—";
    if (badDisks.length) return `${badDisks.length} need${badDisks.length === 1 ? "s" : ""} a look`;
    const n = hw.disks.length;
    return n === 1 ? "Healthy" : n === 2 ? "Both healthy" : `All ${n} healthy`;
  });

  const systemDrive = $derived(hw?.volumes.filter((v) => !v.removable && v.size_bytes > 0).slice(0, 2) ?? []);
  function when(iso: string): string {
    if (!iso) return "Never";
    const t = new Date(iso).getTime();
    if (!t) return "Never";
    const days = Math.floor((Date.now() - t) / 86_400_000);
    return days <= 0 ? `Today, ${new Date(t).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}` : days === 1 ? "Yesterday" : `${days} days ago`;
  }

  function openProblem(id: number) {
    nav.problem = id;
    onproblems();
  }

  let agentMsg = $state("");
  let agentBusy = $state(false);
  async function agentAction(fn: () => Promise<string>) {
    agentBusy = true;
    try { agentMsg = await fn(); } catch (e) { agentMsg = String(e); } finally { agentBusy = false; }
  }
</script>

<div class="grid-hero" style="flex: 1">
  <section class="panel">
    <div style="display: flex; flex-direction: column; gap: 14px">
      <span class="eyebrow">{today}{hw?.os.last_boot_ms ? ` · PC started ${ago(hw.os.last_boot_ms)}` : ""}</span>
      <h1 class="headline">{headline.lead} <em>{headline.em}</em></h1>
    </div>
    <div class="tasks">
      {#if !status}
        <div class="task">
          <span class="ring ring--bad"></span>
          <div><h3>Start background protection</h3><p>Problems are not being watched or fixed while it is off.</p></div>
          <button class="btn" disabled={agentBusy} onclick={() => agentAction(api.startAgent)}>Start</button>
        </div>
      {/if}
      {#each badDisks as d}
        <div class="task">
          <span class="ring ring--bad"></span>
          <div><h3>{d.model} reports “{d.health}”</h3><p>Back up your files from this drive now.</p></div>
          <button class="btn" onclick={onhardware}>Details</button>
        </div>
      {/each}
      {#each attention.slice(0, 4) as f (f.id)}
        <div class="task">
          <span class="ring ring--bad"></span>
          <div><h3>{f.title}</h3><p>{f.explanation}</p></div>
          <button class={f.harmful === "yes" ? "btn" : "btn btn--ghost"} onclick={() => openProblem(f.id)}>{f.fixes.length ? "Fix" : "See why"}</button>
        </div>
      {/each}
      {#if attention.length > 4}
        <div class="task task--quiet">
          <span class="ring ring--quiet"></span>
          <div><h3>{attention.length - 4} more</h3><p>Smaller things worth a look when you have time.</p></div>
          <button class="link" onclick={onproblems}>Show</button>
        </div>
      {/if}
      {#each handling as f (f.id)}
        <div class="task">
          <span class="ring ring--watch"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M12 6v6l4 2" /></svg></span>
          <div><h3>{f.title}</h3><p>Syscura is fixing this now and will check the result.</p></div>
          <span class="muted" style="font-size: 13px">Working</span>
        </div>
      {/each}
      {#if lowSpace.length}
        <div class="task">
          <span class="ring ring--watch"></span>
          <div><h3>{lowSpace.map((v) => v.letter).join(", ")} almost full</h3><p>Less than 10% free space. Windows slows down and updates can fail.</p></div>
          <button class="btn btn--ghost" onclick={onhardware}>Details</button>
        </div>
      {/if}
      {#if fixed.length}
        <div class="task task--quiet">
          <span class="ring ring--ok"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5L19 7" /></svg></span>
          <div><h3>{fixed.length} fixed</h3><p>{fixed.slice(0, 2).map((f) => f.title).join(" · ")}</p></div>
          <button class="link" onclick={onproblems}>Show</button>
        </div>
      {/if}
      {#if routine.length}
        <div class="task task--quiet">
          <span class="ring ring--ok"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5L19 7" /></svg></span>
          <div><h3>{routine.length} routine Windows message{routine.length === 1 ? "" : "s"}</h3><p>Logged on every PC. Nothing to do.</p></div>
          <button class="link" onclick={onproblems}>Show</button>
        </div>
      {/if}
    </div>
  </section>

  <section class="panel panel--brand">
    <div style="display: flex; justify-content: space-between; align-items: baseline">
      <span>Right now</span><span class="mono" style="font-size: 12px; color: var(--on-brand-faint)">LIVE · 2 s</span>
    </div>
    <div class="metrics">
      <div class="metric">
        <span>Processor</span>
        {#if cpuLoad?.value != null}<span class="big-num">{Math.round(cpuLoad.value)}<small>%</small></span>
        {:else if cpuTemp?.value != null}<span class="big-num">{Math.round(cpuTemp.value)}<small>°C</small></span>
        {:else}<span class="big-num">—</span>{/if}
      </div>
      <div class="metric">
        <span>Graphics</span>
        {#if gpuTemp?.value != null}<span class="big-num">{Math.round(gpuTemp.value)}<small>°C</small></span>
        {:else if gpuLoad?.value != null}<span class="big-num">{Math.round(gpuLoad.value)}<small>%</small></span>
        {:else}<span class="big-num">—</span>{/if}
      </div>
      <div class="metric"><span>Memory</span><span class="big-num">{ram ? gb(ram).replace(" GB", "") : "—"}<small>GB installed</small></span></div>
      <div class="metric"><span>Drives</span><span class="big-num" style="font-size: 30px; letter-spacing: -0.03em">{drives}</span></div>
    </div>

    <div class="block">
      <span class="bhead">Protection</span>
      {#if status?.defender}
        {@const d = status.defender}
        <div class="line"><span class="muted">Microsoft Defender</span><b>{d.antivirus && d.realtime ? "On, watching files" : d.antivirus ? "On, but real-time is OFF" : "Off"}</b></div>
        <div class="line"><span class="muted">Virus definitions</span><b>{d.signature_age_days <= 0 ? "Up to date (today)" : d.signature_age_days === 1 ? "1 day old" : `${d.signature_age_days} days old`}</b></div>
        <div class="line"><span class="muted">Last quick scan</span><b>{when(d.last_quick_scan)}</b></div>
        <div class="line"><span class="muted">Last full scan</span><b>{d.last_full_scan ? when(d.last_full_scan) : "Never. Worth running once"}</b></div>
        <div class="line"><span class="muted">Tamper protection</span><b>{d.tamper_protected ? "On" : "Off"}</b></div>
      {:else if status}
        <div class="line"><span class="muted">Microsoft Defender</span><b>Checking…</b></div>
      {:else}
        <div class="line"><span class="muted">Background protection</span><b>Off</b></div>
      {/if}
      {#if security}
        <div class="line"><span class="muted">Security settings</span><button class="lnk" onclick={onsecurity}><b>{security.score} / 100{security.checks.some((c) => c.status === "bad") ? " · see what to fix" : ""}</b></button></div>
      {/if}
      {#if status}
        <div class="line"><span class="muted">Syscura</span><b>{status.service ? "Windows service" : "This session"} · {mb(status.working_set_bytes)}</b></div>
      {/if}
    </div>

    <div class="block">
      <span class="bhead">Today</span>
      {#if status}
        <div class="line"><span class="muted">Windows events (24 h)</span><b>{(status.last_24h.critical + status.last_24h.error).toLocaleString()} errors · {status.last_24h.warning.toLocaleString()} warnings</b></div>
      {/if}
      {#if hw?.os.last_boot_ms}<div class="line"><span class="muted">PC running for</span><b>{duration(Math.floor((Date.now() - hw.os.last_boot_ms) / 1000))}</b></div>{/if}
      {#each systemDrive as v}
        <div class="line"><span class="muted">Free on {v.letter}</span><b>{driveSize(v.free_bytes)} of {driveSize(v.size_bytes)} ({Math.round((v.free_bytes / v.size_bytes) * 100)} %)</b></div>
      {/each}
      {#if hw?.battery.length}<div class="line"><span class="muted">Battery</span><b>{hw.battery[0].charge_pct ?? "?"} %{hw.battery[0].wear_pct != null ? ` · ${hw.battery[0].wear_pct} % worn` : ""}</b></div>{/if}
      <div class="line"><span class="muted">Fixed by Syscura</span><b>{fixed.length} problem{fixed.length === 1 ? "" : "s"}</b></div>
    </div>
    {#if week}
      <div class="block">
        <span class="bhead">This week</span>
        <div class="line"><span class="muted">New problems</span><b>{week.new_problems}{week.security_problems ? ` · ${week.security_problems} about security` : ""}</b></div>
        <div class="line"><span class="muted">Fixed</span><b>{week.fixed_automatically} by Syscura · {week.fixed_by_you} by you</b></div>
        <div class="line"><span class="muted">Windows errors</span><b>{(week.events.critical + week.events.error).toLocaleString()} · {week.events.warning.toLocaleString()} warnings</b></div>
      </div>
    {/if}
    {#if hw}
      <button class="pc" onclick={onhardware}>
        <b>{name}</b>
        <span class="muted">{[hw.cpus[0]?.name.trim(), hw.gpus[0]?.name, ram ? `${gb(ram)} ${hw.memory.sticks[0]?.kind ?? ""}`.trim() : ""].filter(Boolean).join(" · ")}</span>
        <span class="muted">{hw.os.name}{hw.os.version ? ` ${hw.os.version}` : ""} · build {hw.os.build}</span>
      </button>
    {/if}
  </section>
</div>

<footer class="footer">
  <span>
    {#if status}
      {status.service ? "Protection runs as a Windows service" : "Protection runs for this session"} · on for {duration(status.uptime_secs)} · {mb(status.working_set_bytes)} of memory · watching {status.sensors.length} logs
    {:else}
      Background protection is off
    {/if}
    {#if agentMsg} · {agentMsg}{/if}
  </span>
  <span class="right">
    {#if ai.status.configured}
      <span>AI help on{ai.status.model ? ` (${ai.status.model})` : ""} · {ai.status.auto_fix ? "fixes safe problems itself" : "suggests fixes"}</span>
    {:else}
      <button class="link" onclick={onsettings}>Set up free AI help</button>
    {/if}
    {#if status && !status.service}<button class="link" disabled={agentBusy} onclick={() => agentAction(api.installService)}>Install as a Windows service</button>{/if}
    <SocialLinks compact />
  </span>
</footer>

<style>
  .task p { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .block { display: flex; flex-direction: column; gap: 2px; }
  .bhead { font-size: 13px; font-weight: 600; color: var(--on-brand-faint); letter-spacing: 0.02em; margin-bottom: 4px; }
  .line { display: flex; justify-content: space-between; gap: 16px; padding: 7px 0; border-top: 1px solid var(--on-brand-line); font-size: 14px; }
  .line b { font-weight: 500; text-align: right; font-variant-numeric: tabular-nums; }
  .pc { margin-top: auto; display: flex; flex-direction: column; gap: 4px; font-size: 13.5px; text-align: left; background: none; border: 0; border-radius: 0; padding: 0; color: inherit; }
  .pc b { font-weight: 600; }
  .pc:hover { border: 0; }
  .pc:hover b { text-decoration: underline; }
  .right { display: flex; gap: 18px; align-items: center; flex-wrap: wrap; }
  .footer .link { font-size: 13px; }
  .lnk { background: none; border: 0; padding: 0; color: inherit; text-align: right; }
  .lnk:hover b { text-decoration: underline; }
</style>
