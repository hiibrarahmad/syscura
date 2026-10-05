<script lang="ts">
  import type { BoardLayout, Part, SensorPin } from "./layout";
  import { sensorsAt } from "./layout";
  import type { Sensor } from "../types";
  import { fmtSensor, tempTone } from "../api";

  let {
    layout,
    sensors,
    selected = $bindable<string | null>(null),
    showSensors = true,
  }: { layout: BoardLayout; sensors: Sensor[]; selected?: string | null; showSensors?: boolean } = $props();

  const byId = $derived(new Map(layout.parts.map((p) => [p.id, p])));
  const onBoard = $derived(layout.parts.filter((p) => !p.photoOnly && !["board", "disk", "usb", "gpu"].includes(p.kind)));
  const cards = $derived(layout.parts.filter((p) => p.kind === "gpu"));
  const external = $derived(layout.parts.filter((p) => p.kind === "disk" || p.kind === "usb"));
  const W = $derived(layout.width);
  const H = $derived(layout.height);

  function select(id: string) {
    selected = selected === id ? null : id;
  }
  function key(e: KeyboardEvent, id: string) {
    if (e.key === "Enter" || e.key === " ") { e.preventDefault(); select(id); }
  }

  // A soft S-curve cable from an off-board device to its connector.
  function cable(p: Part): string | null {
    const target = p.linkTo ? byId.get(p.linkTo) : undefined;
    if (!target) return null;
    const left = p.x < 0;
    const sx = left ? p.x + p.w : p.x;
    const sy = p.y + p.h / 2;
    const tx = left ? target.x + 2 : target.x + target.w - 2;
    const ty = Math.min(Math.max(sy, target.y + 3), target.y + target.h - 3);
    const mx = (sx + tx) / 2;
    return `M${sx},${sy} C${mx},${sy} ${mx},${ty} ${tx},${ty}`;
  }

  function pinReading(pin: SensorPin): Sensor | undefined {
    return sensorsAt(sensors, pin.site).find((s) => s.kind === "temperature");
  }

  // Mounting holes of the ATX family, scaled to the board size.
  const holes = $derived(
    [[0.03, 0.04], [0.55, 0.04], [0.97, 0.04], [0.03, 0.96], [0.55, 0.96], [0.97, 0.96], [0.55, 0.52]].map(([x, y]) => [x * W, y * H]),
  );
</script>

<svg viewBox="{layout.view.x} {layout.view.y} {layout.view.w} {layout.view.h}" class="map" role="group" aria-label="Motherboard map">
  <defs>
    <linearGradient id="pcb" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="var(--pcb-2)" />
      <stop offset="1" stop-color="var(--pcb)" />
    </linearGradient>
    <pattern id="traces" width="24" height="24" patternUnits="userSpaceOnUse">
      <path d="M0 6h9l4 4h11M0 18h5l3-3h16M12 0v4M18 24v-6" stroke="var(--trace)" stroke-width="0.5" fill="none" />
    </pattern>
    <pattern id="pins" width="2.2" height="2.2" patternUnits="userSpaceOnUse">
      <circle cx="1.1" cy="1.1" r="0.45" fill="#c9a24a" />
    </pattern>
    <pattern id="fins" width="3" height="3" patternUnits="userSpaceOnUse">
      <rect width="1.6" height="3" fill="#3a4651" />
    </pattern>
    <filter id="glow" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="1.6" result="b" />
      <feMerge><feMergeNode in="b" /><feMergeNode in="SourceGraphic" /></feMerge>
    </filter>
  </defs>

  <!-- cables first so parts draw on top -->
  {#each external as p (p.id)}
    {@const d = cable(p)}
    {#if d}
      <path {d} class="cable" class:hot={selected === p.id || selected === p.linkTo} />
    {/if}
  {/each}

  <!-- the board -->
  <g class="part boardbg" role="button" tabindex="0" aria-label="Motherboard" onclick={() => select("board")} onkeydown={(e) => key(e, "board")} class:sel={selected === "board"}>
    <rect x="0" y="0" width={W} height={H} rx="4" fill="url(#pcb)" class="outline" />
    <rect x="0" y="0" width={W} height={H} rx="4" fill="url(#traces)" opacity="0.9" />
    {#each holes as [hx, hy]}<circle cx={hx} cy={hy} r="2.4" class="hole" />{/each}
    <text x={W * 0.6} y={H - 14} class="silk">{byId.get("board")?.label}</text>
  </g>

  {#each onBoard as p (p.id)}
    <g class="part k-{p.kind}" class:sel={selected === p.id} class:filled={p.filled} role="button" tabindex="0" aria-label={p.label}
       onclick={(e) => { e.stopPropagation(); select(p.id); }} onkeydown={(e) => key(e, p.id)}>
      <title>{p.label}</title>
      {#if p.kind === "io"}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="1.5" class="metal" />
        {#each Array(Math.floor(p.h / 13)) as _, i}
          <rect x={p.x + 4} y={p.y + 5 + i * 13} width={p.w - 8} height="8" rx="1" class="port" />
        {/each}
        <text x={p.x + p.w / 2} y={p.y + p.h + 6} class="lbl c">{p.short}</text>
      {:else if p.kind === "vrm"}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="1.5" fill="url(#fins)" class="sink" />
        {#if p.short}<text x={p.x + p.w - 3} y={p.y + p.h / 2 + 1.8} class="lbl r on">{p.short}</text>{/if}
      {:else if p.kind === "cpu"}
        <rect x={p.x - 6} y={p.y - 6} width={p.w + 12} height={p.h + 12} rx="3" class="bracket" />
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="2" class="socket" />
        <rect x={p.x + 3} y={p.y + 3} width={p.w - 6} height={p.h - 6} fill="url(#pins)" opacity="0.55" />
        {#if p.filled}
          <rect x={p.x + 6} y={p.y + 6} width={p.w - 12} height={p.h - 12} rx="1.5" class="ihs" />
          <text x={p.x + p.w / 2} y={p.y + p.h / 2} class="lbl c chip">{(p.label.match(/(Ryzen \d+ \w+|i\d-\w+|Core \w+ \w+)/) ?? [p.short])[0]}</text>
          <text x={p.x + p.w / 2} y={p.y + p.h / 2 + 6} class="lbl c small">{p.short}</text>
        {/if}
      {:else if p.kind === "dimm"}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="1" class="slot" />
        <rect x={p.x - 0.5} y={p.y - 2} width={p.w + 1} height="3" class="latch" />
        <rect x={p.x - 0.5} y={p.y + p.h - 1} width={p.w + 1} height="3" class="latch" />
        {#if p.filled}
          <rect x={p.x + 0.6} y={p.y + 3} width={p.w - 1.2} height={p.h - 6} rx="0.8" class="stick" />
          {#each Array(8) as _, i}<rect x={p.x + 1.6} y={p.y + 8 + i * 15} width={p.w - 3.2} height="9" class="chipbit" />{/each}
        {/if}
        <text x={p.x + p.w / 2} y={p.y + p.h + 8} class="lbl c small">{p.short}</text>
      {:else if p.kind === "pcie"}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="1" class="slot pcie" />
        <rect x={p.x + p.w - 4} y={p.y - 0.5} width="4" height={p.h + 1} class="latch" />
        <text x={p.x + p.w + 4} y={p.y + p.h - 1} class="lbl small">{p.short}</text>
      {:else if p.kind === "m2"}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="1" class="m2" />
        {#each [0, 1, 2] as i}<rect x={p.x + 8 + i * 22} y={p.y + 2} width="16" height={p.h - 4} class="chipbit dark" />{/each}
        <circle cx={p.x + p.w - 3} cy={p.y + p.h / 2} r="1.6" class="hole" />
        <text x={p.x} y={p.y - 2} class="lbl small">{p.short}</text>
      {:else if p.kind === "chipset"}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="4" class="sink plate" />
        <path d="M{p.x + 8},{p.y + p.h - 8} l{p.w - 16},-{p.h - 16}" class="accent-line" />
        <text x={p.x + p.w / 2} y={p.y + p.h / 2 + 2} class="lbl c on">{p.short}</text>
      {:else if p.kind === "sata"}
        {#each [0, 1, 2, 3] as i}<rect x={p.x} y={p.y + i * 11} width={p.w} height="8" rx="1" class="connector" />{/each}
        <text x={p.x + p.w / 2} y={p.y - 2} class="lbl c small">{p.short}</text>
      {:else if p.kind === "atx24"}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="1" class="connector" />
        <rect x={p.x + 1.5} y={p.y + 1.5} width={p.w - 3} height={p.h - 3} fill="url(#pins)" opacity="0.7" />
        <text x={p.x - 2} y={p.y + p.h / 2} class="lbl r small">{p.short}</text>
      {:else if p.kind === "cmos"}
        <circle cx={p.x + p.w / 2} cy={p.y + p.h / 2} r={p.w / 2} class="battery" />
        <text x={p.x + p.w / 2} y={p.y + p.h / 2 + 1.8} class="lbl c small dark">CR2032</text>
      {:else if p.kind === "superio"}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="1" class="ic" />
        <text x={p.x + p.w / 2} y={p.y + p.h + 6} class="lbl c small">{p.short}</text>
      {:else}
        <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="1" class="connector" />
        <rect x={p.x + 1} y={p.y + 1} width={p.w - 2} height={p.h - 2} fill="url(#pins)" opacity="0.7" />
        {#if p.short}<text x={p.x + p.w / 2} y={p.y - 2} class="lbl c small">{p.short}</text>{/if}
      {/if}
    </g>
  {/each}

  <!-- graphics cards sit over their slot -->
  {#each cards as p (p.id)}
    <g class="part k-gpu" class:sel={selected === p.id} role="button" tabindex="0" aria-label={p.label}
       onclick={() => select(p.id)} onkeydown={(e) => key(e, p.id)}>
      <title>{p.label}</title>
      <rect x={p.x} y={p.y} width={p.w} height={p.h} rx="3" class="shroud" />
      <circle cx={p.x + p.w * 0.3} cy={p.y + p.h / 2} r={p.h * 0.38} class="fan" />
      <circle cx={p.x + p.w * 0.7} cy={p.y + p.h / 2} r={p.h * 0.38} class="fan" />
      <rect x={p.x + p.w - 6} y={p.y + 2} width="4" height={p.h - 4} rx="1" class="accent-bar" />
      <text x={p.x + 6} y={p.y + p.h - 3} class="lbl on">{p.short}</text>
    </g>
  {/each}

  <!-- drives and USB devices off the board -->
  {#each external as p (p.id)}
    <g class="part ext k-{p.kind}" class:sel={selected === p.id} role="button" tabindex="0" aria-label={p.label}
       onclick={() => select(p.id)} onkeydown={(e) => key(e, p.id)}>
      <title>{p.label}</title>
      <rect x={p.x} y={p.y} width={p.w} height={p.h} rx={p.kind === "usb" ? p.h / 2 : 3} class="device" />
      {#if p.kind === "disk"}
        <circle cx={p.x + 13} cy={p.y + p.h / 2} r="8" class="platter" />
        <text x={p.x + 25} y={p.y + p.h / 2 - 1} class="lbl">{p.short}</text>
        <text x={p.x + 25} y={p.y + p.h / 2 + 6} class="lbl small faint">{p.linkTo === "io" ? "USB drive" : "SATA drive"}</text>
      {:else}
        <circle cx={p.x + p.h / 2} cy={p.y + p.h / 2} r="3" class="usbdot" />
        <text x={p.x + p.h / 2 + 6} y={p.y + p.h / 2 + 2} class="lbl">{p.short.length > 16 ? p.short.slice(0, 15) + "…" : p.short}</text>
      {/if}
    </g>
  {/each}

  <!-- sensor pins -->
  {#if showSensors}
    {#each layout.pins as pin (pin.id)}
      {@const r = pinReading(pin)}
      {@const tone = tempTone(r?.value)}
      <g class="pin t-{tone}" role="button" tabindex="0" aria-label="{pin.label} sensor" onclick={() => select(pin.partId)} onkeydown={(e) => key(e, pin.partId)}>
        <title>{pin.label} sensor: {pin.where}</title>
        <circle cx={pin.x} cy={pin.y} r="5" class="pulse" />
        <circle cx={pin.x} cy={pin.y} r="2.2" class="dot" filter="url(#glow)" />
        {#if r}
          <rect x={pin.x + 4} y={pin.y - 10} width="22" height="8" rx="4" class="reading" />
          <text x={pin.x + 15} y={pin.y - 4.3} class="lbl c reading-t">{fmtSensor(r)}</text>
        {/if}
      </g>
    {/each}
  {/if}
</svg>

<style>
  .map { width: 100%; height: 100%; display: block; }
  .outline { stroke: #24463c; stroke-width: 0.8; }
  .hole { fill: var(--bg); stroke: #a7a07a; stroke-width: 0.8; }
  .silk { fill: #6f9a8b; font-size: 5px; font-weight: 600; letter-spacing: 0.6px; text-anchor: middle; opacity: 0.8; }
  .lbl { font-size: 4.4px; fill: var(--muted); font-family: var(--font); pointer-events: none; }
  .lbl.small { font-size: 3.6px; }
  .lbl.c { text-anchor: middle; }
  .lbl.r { text-anchor: end; }
  .lbl.on { fill: #dfe7ee; }
  .lbl.dark { fill: #333; }
  .lbl.faint { fill: var(--faint); }
  .lbl.chip { fill: #2a2f35; font-weight: 700; font-size: 4.2px; }
  .metal { fill: #59636d; stroke: #8b96a1; stroke-width: 0.5; }
  .port { fill: #20262c; }
  .sink { stroke: #4b5864; stroke-width: 0.6; }
  .sink.plate { fill: #2e3842; }
  .bracket { fill: none; stroke: #6f7a85; stroke-width: 1.2; }
  .socket { fill: #e8e2cf; stroke: #b9b19a; stroke-width: 0.5; }
  .ihs { fill: #c9ced4; stroke: #8f979f; stroke-width: 0.5; }
  .slot { fill: var(--slot); stroke: #3a4652; stroke-width: 0.5; }
  .slot.pcie { fill: #1d252e; }
  .latch { fill: #6b7783; }
  .stick { fill: #1f6b4f; stroke: #2b8d69; stroke-width: 0.4; }
  .chipbit { fill: #10161b; }
  .chipbit.dark { fill: #0b0f13; }
  .m2 { fill: #16201d; stroke: #3b6a5a; stroke-width: 0.6; }
  .connector { fill: #e9e9e2; stroke: #b5b5ab; stroke-width: 0.4; }
  .battery { fill: #c7ccd1; stroke: #8f979f; stroke-width: 0.6; }
  .ic { fill: #111; stroke: #555; stroke-width: 0.4; }
  .accent-line { stroke: var(--accent); stroke-width: 0.8; opacity: 0.6; }
  .shroud { fill: #1b2129; stroke: #3c4855; stroke-width: 0.8; }
  .fan { fill: #0f1418; stroke: #47525e; stroke-width: 0.8; }
  .accent-bar { fill: var(--accent); opacity: 0.7; }
  .device { fill: var(--panel-2); stroke: var(--line-2); stroke-width: 0.6; }
  .platter { fill: #aab3bc; stroke: #6e7882; stroke-width: 0.6; }
  .usbdot { fill: var(--accent); }
  .cable { fill: none; stroke: var(--line-2); stroke-width: 0.9; stroke-dasharray: 2 1.6; }
  .cable.hot { stroke: var(--accent); stroke-width: 1.2; stroke-dasharray: none; }

  .part { cursor: pointer; outline: none; }
  .part:hover :global(rect), .part:hover :global(circle) { filter: brightness(1.25); }
  .part.sel :global(rect:first-of-type), .part.sel :global(circle:first-of-type) { stroke: var(--accent); stroke-width: 1.4; }
  .part:focus-visible :global(rect:first-of-type) { stroke: var(--accent); stroke-width: 1.2; }
  .boardbg.sel .outline { stroke: var(--accent); stroke-width: 1.4; }
  .boardbg:hover :global(rect) { filter: none; }

  .pin { cursor: pointer; }
  .pin .dot { fill: var(--faint); }
  .pin .pulse { fill: none; stroke: var(--faint); stroke-width: 0.5; opacity: 0.6; transform-box: fill-box; transform-origin: center; animation: pulse 2.4s ease-out infinite; }
  .pin.t-ok .dot { fill: var(--ok); } .pin.t-ok .pulse { stroke: var(--ok); }
  .pin.t-warn .dot { fill: var(--warn); } .pin.t-warn .pulse { stroke: var(--warn); }
  .pin.t-danger .dot { fill: var(--danger); } .pin.t-danger .pulse { stroke: var(--danger); }
  .reading { fill: rgba(5, 10, 14, 0.85); stroke: var(--line-2); stroke-width: 0.4; }
  .reading-t { fill: #e6edf3; font-size: 4px; font-weight: 600; }
  @keyframes pulse { 0% { transform: scale(0.5); opacity: 0.9; } 100% { transform: scale(1.6); opacity: 0; } }
</style>
