<script lang="ts">
  // Simple line icons per part type, used until a real picture arrives.
  let { kind, size = 24 }: { kind: string; size?: number } = $props();
  const k = $derived(kind.split(":")[0]);
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  {#if k === "board"}
    <rect x="3" y="3" width="18" height="18" rx="2" /><rect x="7" y="7" width="6" height="6" /><path d="M16 6v6M18 6v6M7 16h10M7 18.5h6" />
  {:else if k === "cpu"}
    <rect x="6" y="6" width="12" height="12" rx="1.5" /><rect x="9.5" y="9.5" width="5" height="5" /><path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3" />
  {:else if k === "gpu"}
    <rect x="2" y="7" width="20" height="10" rx="1.5" /><circle cx="8" cy="12" r="2.6" /><circle cx="15.5" cy="12" r="2.6" /><path d="M5 17v3M2 9H1" />
  {:else if k === "ram" || k === "dimm"}
    <rect x="2" y="7" width="20" height="9" rx="1" /><path d="M5 10h2v3H5zM9.5 10h2v3h-2zM14 10h2v3h-2zM18 10h1.5v3H18M4 16v2M8 16v2M12 16v2M16 16v2M20 16v2" />
  {:else if k === "disk" || k === "m2"}
    <rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="12" cy="11" r="4" /><circle cx="12" cy="11" r="0.8" /><path d="M7 17h3" />
  {:else if k === "usb"}
    <path d="M12 3v14M12 3l-2 3h4zM8 10v3l4 2M16 9v3l-4 2" /><circle cx="12" cy="19" r="2" />
  {:else}
    <circle cx="12" cy="12" r="8" /><path d="M12 8v5M12 16h.01" />
  {/if}
</svg>
