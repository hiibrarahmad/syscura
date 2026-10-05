<script lang="ts">
  import { untrack } from "svelte";
  import BoardMap from "../lib/board/BoardMap.svelte";
  import BoardPhoto from "../lib/board/BoardPhoto.svelte";
  import PhotoMap from "../lib/board/PhotoMap.svelte";
  import { buildLayout } from "../lib/board/layout";
  import { findProfile } from "../lib/board/profiles";
  import { portKey, usbPanels } from "../lib/prefs.svelte";
  import PartDetail from "../lib/PartDetail.svelte";
  import PartImage from "../lib/PartImage.svelte";
  import { imageData } from "../lib/api";
  import type { HardwareInfo, PartImage as PartImageT, Sensor } from "../lib/types";

  let {
    hw,
    sensors,
    images,
    selected = $bindable<string | null>(null),
    onrescan,
    onimages,
    scanning,
  }: {
    hw: HardwareInfo;
    sensors: Sensor[];
    images: PartImageT[];
    selected?: string | null;
    onrescan: () => void;
    onimages: () => void;
    scanning: boolean;
  } = $props();

  const profile = $derived(findProfile(hw.board.product));
  const layout = $derived(
    buildLayout(hw, { profile, usbPanel: (u) => usbPanels[portKey(u.hub, u.port)] ?? u.panel }),
  );
  // Boards with a verified profile open on the real photo.
  let mode = $state<"map" | "photo">(untrack(() => (findProfile(hw.board.product) ? "photo" : "map")));
  let showSensors = $state(true);

  const boardImage = $derived(images.find((i) => i.key === "board"));
  let boardSrc = $state<string | null>(null);
  $effect(() => {
    const img = boardImage;
    if (img?.status === "ready") imageData(img).then((d) => (boardSrc = d));
    else boardSrc = null;
  });

  // The strip under the board: every real part, in a sensible order.
  const ORDER = ["board", "cpu", "gpu", "dimm", "m2", "disk", "usb"];
  const strip = $derived(
    layout.parts
      .filter((p) => ORDER.includes(p.kind) && (p.kind !== "dimm" || p.filled))
      .sort((a, b) => ORDER.indexOf(a.kind) - ORDER.indexOf(b.kind)),
  );
</script>

<div class="hw">
  <div class="main">
    <div class="toolbar">
      <div class="seg" role="tablist">
        <button role="tab" aria-selected={mode === "map"} class:on={mode === "map"} onclick={() => (mode = "map")}>Board map</button>
        <button role="tab" aria-selected={mode === "photo"} class:on={mode === "photo"} onclick={() => (mode = "photo")} disabled={!boardSrc && !profile} title={boardSrc ? "" : "No board photo yet"}>Real photo</button>
      </div>
      {#if mode === "map"}
        <label class="check"><input type="checkbox" bind:checked={showSensors} /> Sensors</label>
      {/if}
      <span class="hint muted">Click any part for details{mode === "map" ? " · coloured dots are sensors" : ""}</span>
      <button onclick={onrescan} disabled={scanning}>{scanning ? "Scanning…" : "Rescan hardware"}</button>
    </div>

    <div class="stage card">
      {#if mode === "photo" && boardSrc && profile}
        <PhotoMap {hw} {layout} {profile} src={boardSrc} {sensors} bind:selected />
      {:else if mode === "photo" && boardSrc}
        <BoardPhoto {layout} src={boardSrc} boardName={hw.board.product} bind:selected />
      {:else if mode === "photo"}
        <div class="loading muted">Downloading the photo of your board…</div>
      {:else}
        <BoardMap {layout} {sensors} bind:selected {showSensors} />
      {/if}
    </div>

    <div class="strip">
      {#each strip as p (p.id)}
        <button class="chip" class:on={selected === p.id} onclick={() => (selected = p.id)}>
          <PartImage image={images.find((i) => i.key === p.imageKey)} kind={p.kind} size={38} />
          <span class="t">
            <b>{p.kind === "dimm" ? p.label : p.label.length > 30 ? p.label.slice(0, 29) + "…" : p.label}</b>
            <small class="faint">{p.kind === "dimm" ? "Memory" : p.kind === "m2" ? "M.2 SSD" : p.kind === "disk" ? "Drive" : p.kind === "usb" ? "USB device" : p.kind === "gpu" ? "Graphics" : p.kind === "cpu" ? "Processor" : "Motherboard"}</small>
          </span>
        </button>
      {/each}
    </div>
  </div>

  {#if selected}
    <PartDetail {hw} {layout} partId={selected} {sensors} {images} onclose={() => (selected = null)} onchanged={onimages} />
  {/if}
</div>

<style>
  .hw { display: flex; gap: 14px; height: 100%; min-height: 0; }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 10px; }
  .toolbar { display: flex; align-items: center; gap: 12px; }
  .hint { flex: 1; font-size: 12.5px; }
  .seg { display: flex; background: var(--bg-2); border: 1px solid var(--line); border-radius: 9px; padding: 2px; }
  .seg button { border: none; background: none; padding: 5px 12px; border-radius: 7px; color: var(--muted); }
  .seg button.on { background: var(--panel-2); color: var(--text); box-shadow: 0 0 0 1px var(--line-2); }
  .check { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 13px; cursor: pointer; }
  .check input { accent-color: var(--accent); }
  .stage { flex: 1; min-height: 0; padding: 14px; overflow: hidden; }
  .loading { display: grid; place-items: center; height: 100%; }
  .strip { display: flex; gap: 8px; overflow-x: auto; padding-bottom: 2px; flex: none; }
  .chip { display: flex; align-items: center; gap: 8px; padding: 6px 12px 6px 6px; border-radius: 10px; background: var(--panel); border-color: var(--line); flex: none; text-align: left; }
  .chip.on { border-color: var(--accent); background: var(--accent-soft); }
  .chip .t { display: flex; flex-direction: column; line-height: 1.2; }
  .chip b { font-weight: 600; font-size: 12.5px; white-space: nowrap; }
  .chip small { font-size: 11px; }
</style>
