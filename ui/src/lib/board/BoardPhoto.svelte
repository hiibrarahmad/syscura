<script lang="ts">
  import type { BoardLayout, Part } from "./layout";

  let {
    layout,
    src,
    boardName,
    selected = $bindable<string | null>(null),
  }: { layout: BoardLayout; src: string; boardName: string; selected?: string | null } = $props();

  const PIN_KINDS = new Set(["cpu", "dimm", "gpu", "m2", "chipset", "io", "sata", "atx24", "vrm", "usb3_header", "cmos"]);
  const storeKey = $derived(`syscura.pins.${boardName}`);

  // Start from the standard layout; the user can drag pins to the right spot.
  function defaults(): Record<string, { x: number; y: number }> {
    const out: Record<string, { x: number; y: number }> = {};
    for (const p of layout.parts) {
      if (!PIN_KINDS.has(p.kind) || p.id === "vrm_left") continue;
      if (p.kind === "dimm" && !p.filled) continue;
      out[p.id] = { x: (p.x + p.w / 2) / layout.width, y: (p.y + p.h / 2) / layout.height };
    }
    return out;
  }

  function load(): Record<string, { x: number; y: number }> {
    const base = defaults();
    try {
      const saved = JSON.parse(localStorage.getItem(storeKey) ?? "{}");
      for (const id of Object.keys(base)) if (saved[id]) base[id] = saved[id];
    } catch { /* storage unavailable: use defaults */ }
    return base;
  }

  let positions = $state(load());
  let editing = $state(false);
  let dragging: string | null = null;
  let frame: HTMLDivElement;
  let inner: HTMLDivElement;
  let natural = $state({ w: 0, h: 0 });
  let box = $state({ left: 0, top: 0, width: 0, height: 0 });

  // The photo is letterboxed inside the frame; pins must follow the photo.
  function fit() {
    if (!frame || !natural.w) return;
    const fw = frame.clientWidth, fh = frame.clientHeight;
    const scale = Math.min(fw / natural.w, fh / natural.h);
    const width = natural.w * scale, height = natural.h * scale;
    box = { left: (fw - width) / 2, top: (fh - height) / 2, width, height };
  }
  $effect(() => {
    const ro = new ResizeObserver(fit);
    ro.observe(frame);
    return () => ro.disconnect();
  });
  function loaded(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    natural = { w: img.naturalWidth, h: img.naturalHeight };
    fit();
  }

  const partOf = (id: string): Part | undefined => layout.parts.find((p) => p.id === id);

  function save() {
    try { localStorage.setItem(storeKey, JSON.stringify(positions)); } catch { /* ignore */ }
  }

  function down(e: PointerEvent, id: string) {
    if (!editing) { selected = id; return; }
    dragging = id;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!dragging) return;
    const r = inner.getBoundingClientRect();
    positions[dragging] = {
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    };
  }
  function up() {
    if (dragging) { dragging = null; save(); }
  }
  function reset() {
    positions = defaults();
    try { localStorage.removeItem(storeKey); } catch { /* ignore */ }
  }
</script>

<div class="wrap">
  <div class="tools">
    <span class="muted">{editing ? "Drag each pin onto the part in the photo. Changes are saved for this board." : "Pins are placed from the standard board layout and may be slightly off for your model."}</span>
    {#if editing}<button onclick={reset}>Reset pins</button>{/if}
    <button class:primary={editing} onclick={() => (editing = !editing)}>{editing ? "Done" : "Adjust pins"}</button>
  </div>
  <div class="frame" bind:this={frame} onpointermove={move} onpointerup={up} role="presentation">
    <div class="inner" bind:this={inner} style="left: {box.left}px; top: {box.top}px; width: {box.width}px; height: {box.height}px">
    <img {src} alt="Photo of {boardName}" draggable="false" onload={loaded} />
    {#each Object.entries(positions) as [id, pos] (id)}
      {@const p = partOf(id)}
      {#if p}
        <button
          class="pin"
          class:sel={selected === id}
          class:edit={editing}
          style="left: {pos.x * 100}%; top: {pos.y * 100}%"
          onpointerdown={(e) => down(e, id)}
          title={p.label}
        >
          <span class="dot"></span>
          <span class="tag">{p.short ?? p.label}</span>
        </button>
      {/if}
    {/each}
    </div>
  </div>
</div>

<style>
  .wrap { display: flex; flex-direction: column; height: 100%; gap: 8px; }
  .tools { display: flex; align-items: center; gap: 8px; font-size: 12.5px; }
  .tools span { flex: 1; }
  .frame { position: relative; flex: 1; min-height: 0; background: #fff; border-radius: 10px; overflow: hidden; touch-action: none; }
  .inner { position: absolute; }
  img { width: 100%; height: 100%; display: block; }
  .pin {
    position: absolute;
    /* the dot (14px) sits exactly on the point; the label trails it */
    transform: translate(-7px, -50%);
    background: none;
    border: none;
    padding: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    cursor: pointer;
  }
  .pin.edit { cursor: grab; }
  .dot {
    width: 14px; height: 14px; border-radius: 50%;
    background: var(--accent);
    border: 3px solid rgba(4, 33, 28, 0.85);
    box-shadow: 0 0 0 4px rgba(63, 224, 184, 0.35);
  }
  .tag {
    font-size: 11px; font-weight: 600;
    padding: 2px 7px; border-radius: 99px;
    background: rgba(8, 14, 19, 0.85); color: #e6edf3;
    white-space: nowrap;
  }
  .pin.sel .dot { background: #fff; box-shadow: 0 0 0 5px var(--accent); }
  .pin:not(.sel):not(:hover) .tag { opacity: 0.85; }
</style>
