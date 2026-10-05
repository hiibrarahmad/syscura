<script lang="ts">
  // The maker's real photo of this board with exact, labelled hotspots.
  import type { BoardLayout } from "./layout";
  import { installedIn } from "./layout";
  import type { BoardProfile, Region } from "./profiles";
  import type { HardwareInfo, Sensor } from "../types";
  import { driveSize, fmtSensor, gb, tempTone } from "../api";

  let {
    hw,
    layout,
    profile,
    src,
    sensors,
    selected = $bindable<string | null>(null),
  }: {
    hw: HardwareInfo;
    layout: BoardLayout;
    profile: BoardProfile;
    src: string;
    sensors: Sensor[];
    selected?: string | null;
  } = $props();

  let allLabels = $state(false);
  let hovered = $state<string | null>(null);
  let stage: HTMLDivElement;
  let size = $state({ w: 0, h: 0 });

  $effect(() => {
    const ro = new ResizeObserver(() => (size = { w: stage.clientWidth, h: stage.clientHeight }));
    ro.observe(stage);
    return () => ro.disconnect();
  });

  // The photo is square; keep room on both sides for the label columns.
  const COL = 210;
  const box = $derived.by(() => {
    const s = Math.max(200, Math.min(size.h, size.w - 2 * COL * 0.62));
    return { s, left: (size.w - s) / 2, top: (size.h - s) / 2 };
  });

  /** What is installed in each region, for overlays and labels. */
  const installed = $derived.by(() => {
    const out = new Map<string, { target: string; text: string }>();
    for (const r of profile.regions) {
      if (r.id.startsWith("dimm:")) {
        const stick = hw.memory.sticks.find((m) => `dimm:${m.slot}` === r.id);
        if (stick) out.set(r.id, { target: r.id, text: `${gb(stick.capacity_bytes)} ${stick.kind}-${stick.configured_mts}` });
        continue;
      }
      const target = installedIn(layout, r.id);
      const part = target ? layout.parts.find((p) => p.id === target) : undefined;
      if (!part) continue;
      if (part.kind === "gpu") out.set(r.id, { target: part.id, text: part.short ?? part.label });
      if (part.kind === "m2") {
        const d = hw.disks.find((x) => x.device_id === part.ref);
        out.set(r.id, { target: part.id, text: `${d?.model ?? part.label}${d ? " · " + driveSize(d.size_bytes) : ""}` });
      }
    }
    return out;
  });

  const shown = $derived(
    profile.regions.filter((r) => allLabels || r.major || installed.has(r.id) || hovered === r.id || selectedRegion(r)),
  );

  function selectedRegion(r: Region): boolean {
    return selected === r.id || installed.get(r.id)?.target === selected;
  }

  // Place label pills in two columns, in the order of their parts, without
  // overlapping each other.
  const callouts = $derived.by(() => {
    // Two-line tags (with what is installed) are taller than one-line tags.
    const height = (r: Region) => (installed.has(r.id) ? 32 : 19);
    const items = shown.map((r) => {
      const [x, y, w, h] = r.rect;
      const cx = box.left + ((x + w / 2) / 100) * box.s;
      const cy = box.top + ((y + h / 2) / 100) * box.s;
      return { r, cx, cy, left: x + w / 2 < 50, y: cy, h: height(r) };
    });
    for (const side of [true, false]) {
      const col = items.filter((i) => i.left === side).sort((a, b) => a.cy - b.cy);
      let lastBottom = -Infinity;
      for (const it of col) {
        it.y = Math.max(it.cy, lastBottom + 3 + it.h / 2);
        lastBottom = it.y + it.h / 2;
      }
      const last = col.length ? col[col.length - 1].y : 0;
      // Pull the column back up if it ran off the bottom.
      const overflow = last - (size.h - 14);
      if (overflow > 0) for (const it of col) it.y = Math.max(12, it.y - overflow);
    }
    const leftX = box.left + box.s * 0.115;
    const rightX = box.left + box.s * 0.875;
    return items.map((i) => ({ ...i, x: i.left ? leftX : rightX }));
  });

  function pick(r: Region) {
    const id = installed.get(r.id)?.target ?? r.id;
    selected = selected === id ? null : id;
  }

  function reading(at: string): Sensor | undefined {
    return sensors.find((s) => s.site.at === at && s.kind === "temperature");
  }

  const pct = (v: number) => `${v}%`;
</script>

<div class="stage" bind:this={stage}>
  {#if size.w > 0}
    <div class="photo" style="left: {box.left}px; top: {box.top}px; width: {box.s}px; height: {box.s}px">
      <img {src} alt="{hw.board.product}" draggable="false" />

      {#each profile.regions as r (r.id)}
        {@const inst = installed.get(r.id)}
        <button
          class="region k-{r.kind}"
          class:filled={!!inst}
          class:sel={selectedRegion(r)}
          class:hover={hovered === r.id}
          style="left: {pct(r.rect[0])}; top: {pct(r.rect[1])}; width: {pct(r.rect[2])}; height: {pct(r.rect[3])}"
          onclick={() => pick(r)}
          onmouseenter={() => (hovered = r.id)}
          onmouseleave={() => (hovered = null)}
          aria-label={r.label}
          title={r.label}
        ></button>
      {/each}

      {#each profile.sensors as sp}
        {@const s = sp.at === "disk" ? sensors.find((x) => x.site.at === "disk" && x.kind === "temperature") : reading(sp.at)}
        <button class="sensor t-{tempTone(s?.value)}" style="left: {pct(sp.x)}; top: {pct(sp.y)}" title={sp.where} onclick={() => (selected = installed.get(sp.region)?.target ?? sp.region)}>
          <span class="ring"></span><span class="dot"></span>
          {#if s}<span class="val">{fmtSensor(s)}</span>{/if}
        </button>
      {/each}
    </div>

    <svg class="lines" width={size.w} height={size.h} aria-hidden="true">
      {#each callouts as c (c.r.id)}
        <path
          d="M{c.x},{c.y} L{c.x + (c.left ? 14 : -14)},{c.y} L{c.cx},{c.cy}"
          class:hot={selectedRegion(c.r) || hovered === c.r.id}
          class:inst={installed.has(c.r.id)}
        />
        <circle cx={c.cx} cy={c.cy} r="3" class:inst={installed.has(c.r.id)} />
      {/each}
    </svg>

    {#each callouts as c (c.r.id)}
      {@const inst = installed.get(c.r.id)}
      <button
        class="tag"
        class:left={c.left}
        class:inst={!!inst}
        class:sel={selectedRegion(c.r)}
        style="{c.left ? `right: ${size.w - c.x}px` : `left: ${c.x}px`}; top: {c.y}px"
        onclick={() => pick(c.r)}
        onmouseenter={() => (hovered = c.r.id)}
        onmouseleave={() => (hovered = null)}
      >
        <b>{c.r.label}</b>{#if inst}<span>{inst.text}</span>{/if}
      </button>
    {/each}
  {/if}

  <div class="bar">
    <span class="verified">✓ Exact layout for {hw.board.product} · photo: asus.com</span>
    <label><input type="checkbox" bind:checked={allLabels} /> All labels</label>
  </div>
</div>

<style>
  .stage { position: relative; width: 100%; height: 100%; overflow: hidden; }
  .photo { position: absolute; }
  .photo img { width: 100%; height: 100%; display: block; pointer-events: none; }
  .region {
    position: absolute; padding: 0; background: transparent;
    border: 1.5px solid transparent; border-radius: 3px; cursor: pointer;
  }
  .region:hover, .region.hover { border-color: rgba(255, 255, 255, 0.85); background: rgba(255, 255, 255, 0.08); }
  .region.filled { border-color: rgba(63, 224, 184, 0.75); background: rgba(63, 224, 184, 0.16); }
  .region.sel { border-color: var(--accent); background: rgba(63, 224, 184, 0.28); box-shadow: 0 0 0 3px rgba(63, 224, 184, 0.35), 0 0 18px rgba(63, 224, 184, 0.5); }
  .sensor { position: absolute; transform: translate(-50%, -50%); background: none; border: none; padding: 0; width: 18px; height: 18px; cursor: pointer; }
  .sensor .dot { position: absolute; inset: 5px; border-radius: 50%; background: #8b9bab; box-shadow: 0 0 8px currentColor; }
  .sensor .ring { position: absolute; inset: 0; border-radius: 50%; border: 2px solid #8b9bab; animation: ping 2.2s ease-out infinite; }
  .sensor.t-ok .dot { background: var(--ok); } .sensor.t-ok .ring { border-color: var(--ok); }
  .sensor.t-warn .dot { background: var(--warn); } .sensor.t-warn .ring { border-color: var(--warn); }
  .sensor.t-danger .dot { background: var(--danger); } .sensor.t-danger .ring { border-color: var(--danger); }
  .sensor .val { position: absolute; left: 20px; top: -3px; font-size: 11.5px; font-weight: 700; color: #fff; background: rgba(5, 10, 14, 0.85); padding: 1px 6px; border-radius: 99px; white-space: nowrap; }
  @keyframes ping { 0% { transform: scale(0.6); opacity: 1; } 100% { transform: scale(1.7); opacity: 0; } }

  .lines { position: absolute; inset: 0; pointer-events: none; }
  .lines path { fill: none; stroke: rgba(160, 175, 190, 0.55); stroke-width: 1; }
  .lines path.inst { stroke: rgba(63, 224, 184, 0.7); }
  .lines path.hot { stroke: var(--accent); stroke-width: 1.8; }
  .lines circle { fill: rgba(200, 210, 220, 0.9); }
  .lines circle.inst { fill: var(--accent); }

  .tag {
    position: absolute; transform: translateY(-50%);
    display: flex; flex-direction: column; align-items: flex-start; gap: 0;
    padding: 2px 9px; border-radius: 7px; font-size: 11.5px; line-height: 1.25;
    background: rgba(13, 19, 26, 0.92); border: 1px solid var(--line-2); color: #dfe7ee;
    max-width: 205px; text-align: left; white-space: nowrap;
  }
  .tag.left { align-items: flex-end; text-align: right; }
  .tag b { font-weight: 650; letter-spacing: 0.02em; }
  .tag span { color: var(--accent); font-size: 11px; overflow: hidden; text-overflow: ellipsis; max-width: 190px; }
  .tag.inst { border-color: rgba(63, 224, 184, 0.55); }
  .tag.sel, .tag:hover { border-color: var(--accent); background: #0d2420; }

  .bar { position: absolute; left: 4px; right: 4px; bottom: 0; display: flex; justify-content: space-between; align-items: center; font-size: 12px; color: var(--muted); pointer-events: none; }
  .bar label { pointer-events: auto; display: flex; gap: 6px; align-items: center; cursor: pointer; }
  .bar input { accent-color: var(--accent); }
  .verified { color: var(--accent); }
</style>
