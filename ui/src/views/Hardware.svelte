<script lang="ts">
  import DiskTrends from "../lib/DiskTrends.svelte";
  import { buildReport, reportText, type Section } from "../lib/report";
  import { api, driveSize, fmtSensor, gb } from "../lib/api";
  import { ai } from "../lib/ai.svelte";
  import { WEB_AIS, askWebAi, searchWeb } from "../lib/web";
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

  // ---- Details Windows does not report: found online or added by hand.
  // Kept per PC (by maker, model and board), in this app's local storage.
  type Extra = { value: string; source: "ai" | "you" };
  const deviceName = $derived(
    [hw.system.manufacturer, hw.system.model, hw.board.manufacturer && `(board: ${hw.board.manufacturer} ${hw.board.product})`]
      .filter(Boolean)
      .join(" ") || name,
  );
  const extrasKey = $derived(`syscura.hw.extra:${[hw.system.manufacturer, hw.system.model, hw.board.product].join("|")}`);
  let extras = $state<Record<string, Extra>>({});
  $effect(() => {
    try { extras = JSON.parse(localStorage.getItem(extrasKey) ?? "{}"); } catch { extras = {}; }
  });
  function saveExtras() {
    try { localStorage.setItem(extrasKey, JSON.stringify(extras)); } catch { /* fine */ }
  }
  const fieldKey = (s: Section, heading: string | undefined, k: string) => `${s.title}${heading ? ` > ${heading}` : ""} > ${k}`;
  // Live readings and wear cannot be looked up; only fixed specifications.
  const lookupable = (k: string) => !/temp|load|clock|fan|power draw|voltage|wear|health|hours|charge|free|speed now|link now|in use/i.test(k);
  const missing = $derived(
    sections.flatMap((s) =>
      s.groups.flatMap((g) => g.rows.filter(([k, v]) => v == null && lookupable(k)).map(([k]) => fieldKey(s, g.heading, k))),
    ),
  );
  const stillMissing = $derived(missing.filter((f) => !extras[f]));
  let lookup = $state<{ busy: boolean; text: string; bad?: boolean; sources: { title: string; url: string }[] }>({ busy: false, text: "", sources: [] });
  async function findWithAi() {
    lookup = { busy: true, text: "Searching the maker's pages for this exact model…", sources: [] };
    try {
      const r = await api.hwLookup(deviceName, stillMissing);
      for (const [k, v] of Object.entries(r.values)) extras[k] = { value: v, source: "ai" };
      saveExtras();
      const n = Object.keys(r.values).length;
      lookup = {
        busy: false,
        text: n ? `Found ${n} of ${stillMissing.length + n} missing details${r.note ? `: ${r.note}` : "."}` : "No reliable source states these for this exact model. You can add them yourself.",
        sources: r.sources,
      };
    } catch (e) {
      lookup = { busy: false, text: String(e), bad: true, sources: [] };
    }
  }
  function webQuestion() {
    return {
      title: `Specifications of ${deviceName}`,
      explanation: "Windows does not report these details. Please give the official values for this exact model, with the source.",
      details: Object.fromEntries(stillMissing.slice(0, 30).map((f, i) => [`Detail ${i + 1}`, f])),
      system: `${hw.os.name} ${hw.os.version}`,
    };
  }
  let editing = $state<string | null>(null);
  let draft = $state("");
  function saveDraft(key: string) {
    const v = draft.trim();
    if (v) extras[key] = { value: v.slice(0, 120), source: "you" };
    else delete extras[key];
    saveExtras();
    editing = null;
  }

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

{#if stillMissing.length && !filter}
  <section class="panel missing-panel">
    <div class="mtop">
      <div>
        <span class="kicker">{stillMissing.length} detail{stillMissing.length === 1 ? "" : "s"} Windows does not report</span>
        <p class="muted">Every PC reports different things. Syscura can look up the official specifications of <b>{deviceName}</b> and fill in the gaps. They are marked <i>found online</i>, so you always know what was read from your PC.</p>
      </div>
      <div class="row-chips">
        {#if ai.status.configured}
          <button class="btn btn--sm" disabled={lookup.busy} onclick={findWithAi}>{lookup.busy ? "Searching…" : "Find them with AI"}</button>
        {/if}
        <button class="btn btn--ghost btn--sm" onclick={() => searchWeb(`${deviceName} specifications`)}>Search the web</button>
        {#each WEB_AIS.filter((w) => w.id === "chatgpt" || w.id === "google") as w}
          <button class="btn btn--ghost btn--sm" onclick={() => askWebAi(w.id, webQuestion())}>Ask {w.name}</button>
        {/each}
      </div>
    </div>
    {#if lookup.text}<p class="small" class:bad={lookup.bad}>{lookup.text}</p>{/if}
    {#if lookup.sources.length}
      <p class="small muted">Sources: {#each lookup.sources as src, i}<button class="link" onclick={() => api.open(src.url)}>{src.title || src.url}</button>{i < lookup.sources.length - 1 ? " · " : ""}{/each}</p>
    {/if}
    {#if !ai.status.configured}<p class="small muted">With a free Gemini key (Settings) Syscura fills them in by itself. Without one, ask an AI website, then click <b>add</b> next to a detail to type it in.</p>{/if}
  </section>
{/if}

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
              {@const key = fieldKey(s, g.heading, k)}
              <div>
                <dt>{k}</dt>
                {#if val != null}
                  <dd>{val}</dd>
                {:else if editing === key}
                  <dd class="edit">
                    <!-- svelte-ignore a11y_autofocus -->
                    <input type="text" bind:value={draft} autofocus placeholder="Type the value" onkeydown={(e) => { if (e.key === "Enter") saveDraft(key); if (e.key === "Escape") editing = null; }} />
                    <button class="btn btn--sm" onclick={() => saveDraft(key)}>Save</button>
                  </dd>
                {:else if extras[key]}
                  <dd>
                    {extras[key].value}
                    <button class="tag" title="Not read from this PC. Click to change." onclick={() => { editing = key; draft = extras[key].value; }}>{extras[key].source === "ai" ? "found online" : "added by you"}</button>
                  </dd>
                {:else}
                  <dd class="missing">
                    Not reported
                    {#if lookupable(k)}<button class="tag" onclick={() => { editing = key; draft = ""; }}>add</button>{/if}
                  </dd>
                {/if}
              </div>
            {/each}
          </dl>
        </div>
      {/each}
    </div>
  </article>
{:else}
  <section class="panel"><p class="muted" style="margin: 0">Nothing matches “{filter}”.</p></section>
{/each}

{#if !filter && (!only || only === "storage")}<DiskTrends />{/if}

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
  .tag { margin-left: 8px; font-size: 11.5px; font-style: normal; font-weight: 500; padding: 1px 9px; border-radius: var(--r-pill); border: 0; background: var(--brand-tint); color: var(--brand-tint-ink); }
  .tag:hover { background: var(--brand); color: var(--brand-ink); }
  dd.edit { display: flex; gap: 6px; align-items: center; }
  dd.edit input { padding: 5px 12px; font-size: 13px; width: 160px; }
  .missing-panel { padding: 26px 32px; gap: 10px; }
  .mtop { display: flex; justify-content: space-between; gap: 20px; flex-wrap: wrap; align-items: flex-start; }
  .mtop p { margin: 6px 0 0; font-size: 14px; max-width: 640px; }
  .small { font-size: 13.5px; margin: 0; }
  .bad { color: var(--bad); }
  .link { font-size: 13px; }
  .warnnote { color: var(--warn-ink); }
  .device { user-select: text; }
</style>
