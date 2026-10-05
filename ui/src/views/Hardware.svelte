<script lang="ts">
  import { buildReport, reportText, type Section } from "../lib/report";
  import { driveSize, fmtSensor, gb } from "../lib/api";
  import type { HardwareInfo, Sensor } from "../lib/types";

  let {
    hw,
    sensors,
    fromAgent,
    onrescan,
    scanning,
  }: { hw: HardwareInfo; sensors: Sensor[]; fromAgent: boolean; onrescan: () => void; scanning: boolean } = $props();

  const sections = $derived(buildReport(hw, sensors.filter((s) => s.source === "windows" || s.source === "nvml" || s.source === "lhm")));
  let filter = $state("");
  let only = $state<string | null>(null);
  let copied = $state(false);

  const shown = $derived(
    sections
      .filter((s) => !only || s.id === only)
      .map((s) => {
        const q = filter.trim().toLowerCase();
        if (!q) return s;
        return {
          ...s,
          groups: s.groups
            .map((g) => ({ ...g, rows: g.rows.filter(([k, val]) => `${s.title} ${g.heading ?? ""} ${k} ${val ?? ""}`.toLowerCase().includes(q)) }))
            .filter((g) => g.rows.length),
        };
      })
      .filter((s) => s.groups.some((g) => g.rows.length)),
  );

  async function copy() {
    try {
      await navigator.clipboard.writeText(reportText(sections, hw));
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch { /* clipboard blocked */ }
  }

  const sensor = (at: string, kind: string) => sensors.find((s) => s.site.at === at && s.kind === kind && s.value != null);
  const name = $derived(
    (hw.system.model ? [hw.system.manufacturer, hw.system.model].filter(Boolean).join(" ") : "") || [hw.board.manufacturer, hw.board.product].filter(Boolean).join(" ") || "This PC",
  );
  const ram = $derived(hw.memory.sticks.reduce((a, s) => a + s.capacity_bytes, 0) || hw.memory.usable_bytes);
  const partCount = $derived(hw.cpus.length + hw.memory.sticks.length + hw.gpus.length + hw.disks.length + hw.displays.length + hw.battery.length);
  const badDisks = $derived(hw.disks.filter((d) => d.health !== "Healthy" && d.health !== "Unknown"));
  const scannedAt = $derived(new Date(hw.collected_ms || Date.now()).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }));

  /** The one line that names what the section is about. */
  function title(s: Section): string {
    const sticks = hw.memory.sticks;
    switch (s.id) {
      case "windows": return `${hw.os.name}${hw.os.version ? ` ${hw.os.version}` : ""}`;
      case "system": return name;
      case "board": return [hw.board.manufacturer, hw.board.product].filter(Boolean).join(" ") || "Motherboard";
      case "cpu": return hw.cpus.map((c) => c.name.trim()).join(", ") || s.title;
      case "memory":
        if (!sticks.length) return ram ? `${gb(ram)} installed` : s.title;
        return sticks.every((x) => x.capacity_bytes === sticks[0].capacity_bytes)
          ? `${sticks.length} × ${gb(sticks[0].capacity_bytes)} ${[sticks[0].manufacturer, sticks[0].kind].filter(Boolean).join(" ")}`
          : `${gb(ram)} ${sticks[0].kind}`;
      case "gpu": return hw.gpus.map((g) => g.name).join(", ") || s.title;
      case "displays": return hw.displays.map((d) => [d.manufacturer, d.model].filter(Boolean).join(" ")).join(", ") || s.title;
      case "storage": return hw.disks.length === 1 ? hw.disks[0].model : `${hw.disks.length} drives`;
      case "network": return hw.network.filter((n) => n.connected).map((n) => n.name)[0] ?? `${hw.network.length} adapters`;
      default: {
        const n = s.groups.filter((g) => g.heading).length;
        return n > 1 ? `${n} ${s.title.toLowerCase()}` : s.groups[0]?.heading ?? s.title;
      }
    }
  }

  function about(s: Section): string {
    if (s.note) return s.note;
    switch (s.id) {
      case "storage": return badDisks.length ? `${badDisks.map((d) => d.model).join(", ")} report a problem. Back up your files.` : hw.disks.map((d) => `${d.model} (${driveSize(d.size_bytes)})`).join(" · ");
      case "memory": return `${hw.memory.sticks.length} of ${hw.memory.total_slots || hw.memory.sticks.length} slots used${hw.memory.max_bytes ? ` · up to ${gb(hw.memory.max_bytes)} supported` : ""}.`;
      case "windows": return `Build ${hw.os.build} · ${hw.os.architecture}`;
      default: return "";
    }
  }

  function stats(s: Section): [string, string][] {
    const out: [string, string][] = [];
    const add = (v: Sensor | undefined, label: string) => v && out.push([fmtSensor(v), label]);
    if (s.id === "cpu") {
      add(sensor("cpu", "load"), "load");
      const clock = sensor("cpu", "clock");
      if (clock?.value) out.push([`${(clock.value / 1000).toFixed(2)} GHz`, "clock"]);
      add(sensor("cpu", "temperature"), "temperature");
      const c = hw.cpus[0];
      if (c?.cores) out.push([`${c.cores} / ${c.threads}`, "cores / threads"]);
    } else if (s.id === "gpu") {
      add(sensor("gpu", "temperature"), "temperature");
      add(sensor("gpu", "load"), "load");
      add(sensor("gpu", "power"), "power");
      if (hw.gpus[0]?.vram_bytes) out.push([gb(hw.gpus[0].vram_bytes), "video memory"]);
    } else if (s.id === "memory") {
      if (ram) out.push([gb(ram), "installed"]);
      const speed = hw.memory.sticks[0]?.configured_mts || hw.memory.sticks[0]?.speed_mts;
      if (speed) out.push([String(speed), "MT/s"]);
    } else if (s.id === "storage") {
      out.push([driveSize(hw.disks.reduce((a, d) => a + d.size_bytes, 0)), "total"]);
      const free = hw.volumes.filter((v) => !v.removable).reduce((a, v) => a + v.free_bytes, 0);
      if (free) out.push([driveSize(free), "free"]);
    }
    return out.slice(0, 3);
  }
</script>

<div class="grid-hero">
  <section class="panel" style="padding: 36px 44px; gap: 14px">
    <span class="eyebrow">Read from this PC today at {scannedAt}</span>
    <h1 class="title-xl">
      {partCount} parts found.
      <span class="accent">{badDisks.length ? `${badDisks.length} drive${badDisks.length > 1 ? "s" : ""} need${badDisks.length > 1 ? "" : "s"} a look.` : hw.notes.length ? `${hw.notes.length} thing${hw.notes.length > 1 ? "s" : ""} worth knowing.` : "Nothing looks wrong."}</span>
    </h1>
    <span class="muted">Everything here is read straight from Windows and your devices{fromAgent ? "" : ". Start protection for drive temperature and wear"}. Anything your PC does not report says <i>Not reported</i>.</span>
    <div class="row-chips" style="margin-top: 6px">
      <button class="btn btn--ghost btn--sm" onclick={onrescan} disabled={scanning}>{scanning ? "Reading…" : "Read again"}</button>
      <button class="btn btn--ghost btn--sm" onclick={copy}>{copied ? "Copied ✓" : "Copy report"}</button>
    </div>
  </section>
  <section class="panel panel--brand" style="gap: 10px; padding: 32px 36px">
    <span class="muted" style="font-size: 14px">This PC</span>
    <span class="title-m" style="font-size: 26px">{name}</span>
    <dl class="pcspecs">
      <dt class="muted">Windows</dt><dd>{hw.os.name.replace("Windows ", "")}{hw.os.version ? ` ${hw.os.version}` : ""} · {hw.os.build}</dd>
      {#if hw.board.bios_version}<dt class="muted">BIOS</dt><dd>{hw.board.bios_version}{hw.board.bios_date ? ` · ${hw.board.bios_date}` : ""}</dd>{/if}
      <dt class="muted">Security</dt><dd>{hw.system.firmware || "Firmware not reported"}{hw.system.secure_boot != null ? ` · Secure Boot ${hw.system.secure_boot ? "on" : "off"}` : ""}</dd>
      {#if hw.system.virtual_machine}<dt class="muted">Runs in</dt><dd>{hw.system.virtual_machine}</dd>{/if}
    </dl>
  </section>
</div>

<div class="row-chips" role="tablist" aria-label="Part type">
  <button class="chip" role="tab" aria-selected={only === null} onclick={() => (only = null)}>Everything</button>
  {#each sections as s}
    <button class="chip" role="tab" aria-selected={only === s.id} onclick={() => (only = only === s.id ? null : s.id)}>{s.title}</button>
  {/each}
  <input class="search" style="margin-left: auto" type="text" placeholder="Search parts and details" bind:value={filter} />
</div>

{#each shown as s (s.id)}
  {@const st = stats(s)}
  <article class="device">
    <div class="device__about">
      <span class="kicker">{s.title}</span>
      <span class="title-m">{title(s)}</span>
      {#if about(s)}<span class="muted" class:warnnote={!!s.note}>{about(s)}</span>{/if}
      {#if st.length}
        <div class="device__stats">
          {#each st as [v, label]}<div class="stat"><b>{v}</b><span>{label}</span></div>{/each}
        </div>
      {/if}
    </div>
    <div class="device__detail">
      {#each s.groups.filter((g) => g.rows.length) as g}
        <div class="grp">
          {#if g.heading}<h4>{g.heading}{#if g.badge}<span class="pill pill--bad">{g.badge}</span>{/if}</h4>{/if}
          <dl class="specs">
            {#each g.rows as [k, val]}
              <div><dt>{k}</dt><dd class:missing={val == null}>{val ?? "Not reported"}</dd></div>
            {/each}
          </dl>
        </div>
      {/each}
    </div>
  </article>
{:else}
  <section class="panel"><p class="muted" style="margin: 0">Nothing matches “{filter}”.</p></section>
{/each}

{#if hw.notes.length && !filter && !only}
  <section class="panel" style="padding: 30px 36px; gap: 12px">
    <span class="kicker">Good to know</span>
    {#each hw.notes as n}<p class="muted" style="margin: 0">{n}</p>{/each}
  </section>
{/if}

<style>
  .pcspecs { display: grid; grid-template-columns: 90px 1fr; row-gap: 6px; margin: 6px 0 0; font-size: 14px; }
  .pcspecs dd { margin: 0; }
  .grp { display: flex; flex-direction: column; gap: 4px; }
  h4 { margin: 0; font-size: 14.5px; font-weight: 600; display: flex; gap: 10px; align-items: center; }
  .grp + .grp { margin-top: 6px; }
  dd.missing { color: var(--faint); font-weight: 400; font-style: italic; }
  .warnnote { color: var(--warn-ink); }
  .device { user-select: text; }
</style>
