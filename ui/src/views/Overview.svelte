<script lang="ts">
  import PartImage from "../lib/PartImage.svelte";
  import { api, driveSize, duration, fmtSensor, gb, mb, tempTone } from "../lib/api";
  import type { Finding, HardwareInfo, PartImage as PartImageT, Sensor, StatusInfo, UsbDevice } from "../lib/types";
  import { needsAttention } from "../lib/findings";
  import { portKey, usbPanels } from "../lib/prefs.svelte";

  let {
    hw,
    fromAgent,
    status,
    sensors,
    images,
    findings,
    onopen,
    onproblems,
  }: {
    hw: HardwareInfo | null;
    fromAgent: boolean;
    status: StatusInfo | null;
    sensors: Sensor[];
    images: PartImageT[];
    findings: Finding[];
    onopen: (partId: string) => void;
    onproblems: () => void;
  } = $props();

  const img = (key: string) => images.find((i) => i.key === key);
  const temp = (at: string) => sensors.find((s) => s.site.at === at && s.kind === "temperature");
  const gpuTemp = $derived(temp("gpu"));
  const cpuTemp = $derived(temp("cpu"));
  const cpuLoad = $derived(sensors.find((s) => s.site.at === "cpu" && s.kind === "load"));
  const cpuClock = $derived(sensors.find((s) => s.site.at === "cpu" && s.kind === "clock"));
  const panelText = (u: UsbDevice) => {
    const p = usbPanels[portKey(u.hub, u.port)] ?? u.panel;
    return p === "back" ? "rear panel" : p === "front" ? "case front" : p === "internal" ? "internal" : "port not set";
  };
  let agentMsg = $state("");
  let agentBusy = $state(false);
  async function agentAction(fn: () => Promise<string>) {
    agentBusy = true;
    try { agentMsg = await fn(); } catch (e) { agentMsg = String(e); } finally { agentBusy = false; }
  }
  const hottestDisk = $derived(
    sensors.filter((s) => s.site.at === "disk" && s.value != null).sort((a, b) => (b.value ?? 0) - (a.value ?? 0))[0],
  );
  const attention = $derived(findings.filter(needsAttention));
  const harmful = $derived(attention.filter((f) => f.harmful === "yes"));
  const unhealthyDisks = $derived(hw?.disks.filter((d) => d.health !== "Healthy") ?? []);
  const ramTotal = $derived(hw?.memory.sticks.reduce((a, s) => a + s.capacity_bytes, 0) ?? 0);

  const verdict = $derived.by(() => {
    if (unhealthyDisks.length) return { tone: "danger", text: `${unhealthyDisks.length} drive${unhealthyDisks.length > 1 ? "s" : ""} need attention` };
    if (harmful.length) return { tone: "danger", text: `${harmful.length} harmful problem${harmful.length > 1 ? "s" : ""} found` };
    if (!status) return { tone: "warn", text: "Background protection is not running" };
    if (attention.length) return { tone: "warn", text: `${attention.length} thing${attention.length > 1 ? "s" : ""} worth a look` };
    return { tone: "ok", text: "Your PC looks healthy" };
  });
</script>

<div class="ov">
  <section class="hero card">
    <div class="hero-pic">
      <PartImage image={img("board")} kind="board" size={200} cover />
    </div>
    <div class="hero-text">
      <span class="pill t-{verdict.tone}">{verdict.text}</span>
      <h1>{hw?.board.product ?? "Reading your hardware…"}</h1>
      {#if hw}
        <p class="muted">
          {hw.cpus[0]?.name.replace(/\s+\d+-Core Processor\s*$/i, "")} · {gb(ramTotal)} {hw.memory.sticks[0]?.kind} ·
          {hw.gpus.find((g) => g.vendor)?.name ?? "integrated graphics"} · {hw.system.os}
        </p>
      {/if}
      <div class="tiles">
        <div class="tile">
          <span class="k">CPU</span>
          {#if cpuTemp}
            <span class="v t-{tempTone(cpuTemp.value)}">{fmtSensor(cpuTemp)}</span>
            <span class="s">temperature{cpuLoad ? ` · ${fmtSensor(cpuLoad)} load` : ""}</span>
          {:else}
            <span class="v t-ok">{cpuLoad ? fmtSensor(cpuLoad) : "—"}</span>
            <span class="s">load{cpuClock ? ` · ${(cpuClock.value! / 1000).toFixed(2)} GHz` : ""}</span>
          {/if}
        </div>
        <div class="tile">
          <span class="k">GPU</span>
          <span class="v t-{tempTone(gpuTemp?.value)}">{gpuTemp ? fmtSensor(gpuTemp) : "—"}</span>
          <span class="s">{gpuTemp ? "temperature" : "no reading"}</span>
        </div>
        <div class="tile">
          <span class="k">Drives</span>
          <span class="v t-{hottestDisk ? tempTone(hottestDisk.value) : unhealthyDisks.length ? 'danger' : 'ok'}">{hottestDisk ? fmtSensor(hottestDisk) : unhealthyDisks.length ? "Check" : "Healthy"}</span>
          <span class="s">{hottestDisk ? "hottest drive" : `${hw?.disks.length ?? 0} drives`}</span>
        </div>
        <button class="tile link" onclick={onproblems}>
          <span class="k">Problems</span>
          <span class="v t-{harmful.length ? 'danger' : attention.length ? 'warn' : 'ok'}">{status ? attention.length : "—"}</span>
          <span class="s">{status ? (harmful.length ? `${harmful.length} harmful · open the list` : "need a look · open the list") : "agent offline"}</span>
        </button>
      </div>
    </div>
  </section>

  {#if hw}
    <section class="parts">
      <button class="part card" onclick={() => onopen("cpu")}>
        <PartImage image={img("cpu:0")} kind="cpu" size={56} />
        <div><small>Processor</small><b>{hw.cpus[0]?.name.replace(/\s+\d+-Core Processor\s*$/i, "")}</b><span class="muted">{hw.cpus[0]?.cores} cores · {hw.cpus[0]?.threads} threads · {hw.cpus[0]?.socket}</span></div>
      </button>
      {#each hw.gpus.filter((g) => g.vendor) as g, i}
        <button class="part card" onclick={() => onopen(`gpu:${hw.gpus.indexOf(g)}`)}>
          <PartImage image={img(`gpu:${hw.gpus.indexOf(g)}`)} kind="gpu" size={56} />
          <div><small>Graphics</small><b>{g.name}</b><span class="muted">{gb(g.vram_bytes)} · {g.board_partner || g.vendor}</span></div>
        </button>
      {/each}
      {#each [...new Set(hw.memory.sticks.map((s) => s.part_number))] as pn}
        {@const group = hw.memory.sticks.filter((s) => s.part_number === pn)}
        <button class="part card" onclick={() => onopen(`dimm:${group[0].slot}`)}>
          <PartImage image={img(`ram:${pn}`)} kind="ram" size={56} />
          <div><small>Memory</small><b>{group.length} × {gb(group[0].capacity_bytes)} {group[0].kind}</b><span class="muted">{group[0].manufacturer} {pn} · {group.map((s) => s.slot).join(", ")}</span></div>
        </button>
      {/each}
      {#each hw.disks as d}
        <button class="part card" onclick={() => onopen(`disk:${d.device_id}`)}>
          <PartImage image={img(`disk:${d.device_id}`)} kind="disk" size={56} />
          <div><small>{d.media === "Unknown" ? "Drive" : d.media} · {d.bus}</small><b>{d.model}</b><span class="muted">{driveSize(d.size_bytes)} · <span class:bad={d.health !== "Healthy"}>{d.health}</span></span></div>
        </button>
      {/each}
      {#each hw.usb.filter((u) => u.kind !== "Hub") as u}
        <button class="part card" onclick={() => onopen(`usb:${u.instance_id}`)}>
          <PartImage image={img(`usb:${u.instance_id}`)} kind="usb" size={56} />
          <div><small>USB · {panelText(u)}</small><b>{u.name}</b><span class="muted">{u.kind}</span></div>
        </button>
      {/each}
    </section>

    <section class="foot">
      <div class="card agent">
        <h3>Background protection</h3>
        {#if status}
          <p><span class="dot ok"></span>Running for {duration(status.uptime_secs)} · using <b>{mb(status.working_set_bytes)}</b> of RAM · {status.events_total} events logged</p>
          <p class="faint">Watching: {status.sensors.join(", ")}</p>
        {:else}
          <p><span class="dot warn"></span>Background protection is off, so problems are not being watched.</p>
          <div class="btns">
            <button class="primary" disabled={agentBusy} onclick={() => agentAction(api.startAgent)}>Start protection</button>
          </div>
        {/if}
        {#if status}
          <div class="btns">
            <button disabled={agentBusy} onclick={() => agentAction(api.installService)} title="Starts with Windows and can apply fixes that need admin rights">Install as a Windows service…</button>
          </div>
        {/if}
        {#if agentMsg}<p class="faint">{agentMsg}</p>{/if}
        {#if !fromAgent}<p class="faint">Hardware was read by this window (drive temperatures need the service).</p>{/if}
      </div>
      {#if hw.notes.length}
        <div class="card notes">
          <h3>Good to know</h3>
          {#each hw.notes as n}<p>{n}</p>{/each}
        </div>
      {/if}
    </section>
  {/if}
</div>

<style>
  .ov { display: flex; flex-direction: column; gap: 14px; height: 100%; overflow-y: auto; padding-right: 4px; }
  .hero { display: flex; gap: 20px; padding: 18px; align-items: stretch; }
  .hero-pic { width: 300px; flex: none; }
  .hero-text { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; }
  h1 { margin: 2px 0 0; font-size: 26px; letter-spacing: -0.01em; }
  .hero-text p { margin: 0; }
  .pill { align-self: flex-start; font-size: 12px; font-weight: 600; padding: 3px 10px; border-radius: 99px; background: var(--accent-soft); color: var(--ok); }
  .pill.t-warn { background: rgba(244, 183, 64, 0.12); color: var(--warn); }
  .pill.t-danger { background: rgba(255, 92, 108, 0.12); color: var(--danger); }
  .tiles { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; margin-top: auto; padding-top: 10px; }
  .tile { background: var(--bg-2); border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; }
  .tile.link { text-align: left; cursor: pointer; font: inherit; color: inherit; }
  .tile.link:hover { border-color: var(--accent); }
  .tile .k { font-size: 11.5px; color: var(--faint); text-transform: uppercase; letter-spacing: 0.06em; }
  .tile .v { font-size: 24px; font-weight: 650; }
  .tile .s { font-size: 12px; color: var(--muted); }
  .t-ok { color: var(--ok); } .t-warn { color: var(--warn); } .t-danger { color: var(--danger); } .t-none { color: var(--muted); }
  .parts { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; }
  .part { display: flex; gap: 12px; align-items: center; padding: 10px; text-align: left; cursor: pointer; }
  .part:hover { border-color: var(--accent); }
  .part div { display: flex; flex-direction: column; min-width: 0; }
  .part small { font-size: 11px; color: var(--faint); text-transform: uppercase; letter-spacing: 0.06em; }
  .part b { font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .part .muted { font-size: 12.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .bad { color: var(--danger); }
  .foot { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .foot .card { padding: 14px 16px; }
  .foot h3 { margin: 0 0 6px; font-size: 13px; }
  .foot p { margin: 4px 0; font-size: 13px; }
  .btns { display: flex; gap: 8px; margin: 8px 0 2px; }
  .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: 8px; }
  .dot.ok { background: var(--ok); box-shadow: 0 0 8px var(--ok); }
  .dot.warn { background: var(--warn); }
</style>
