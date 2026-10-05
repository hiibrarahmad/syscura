<script lang="ts">
  import { imageData } from "./api";
  import type { PartImage } from "./types";
  import PartIcon from "./PartIcon.svelte";

  let { image, kind, size = 64, cover = false }: { image?: PartImage; kind: string; size?: number; cover?: boolean } = $props();

  let src = $state<string | null>(null);

  $effect(() => {
    const img = image;
    src = null;
    if (img?.status === "ready") {
      imageData(img).then((d) => { if (image === img) src = d; });
    }
  });

  const busy = $derived(image?.status === "searching" || image?.status === "queued");
</script>

<div class="pic" class:cover style="--size: {size}px">
  {#if src}
    <img {src} alt="" draggable="false" />
  {:else}
    <PartIcon {kind} size={Math.round(size * 0.55)} />
    {#if busy}<span class="spin" title="Finding a picture…"></span>{/if}
  {/if}
</div>

<style>
  .pic {
    width: var(--size);
    height: var(--size);
    flex: none;
    border-radius: 10px;
    background: var(--bg-2);
    border: 1px solid var(--line);
    display: grid;
    place-items: center;
    overflow: hidden;
    position: relative;
    color: var(--faint);
  }
  .pic.cover { width: 100%; height: var(--size); }
  img { width: 100%; height: 100%; object-fit: contain; background: #fff; }
  .spin {
    position: absolute;
    right: 6px;
    bottom: 6px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid var(--line-2);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
