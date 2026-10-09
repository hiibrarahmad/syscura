<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./api";
  import type { DiskPoint } from "./types";

  let points = $state<DiskPoint[] | null>(null);
  onMount(() => {
    api.diskHistory().then((p) => (points = p)).catch(() => (points = null));
  });

  type Trend = { disk: string; days: number; first: DiskPoint; last: DiskPoint; temps: number[]; wears: number[] };
  const trends = $derived.by((): Trend[] => {
    if (!points) return [];
    const by = new Map<string, DiskPoint[]>();
    for (const p of points) by.set(p.disk, [...(by.get(p.disk) ?? []), p]);
    return [...by.entries()].map(([disk, list]) => ({
      disk,
      days: list.length,
      first: list[0],
      last: list[list.length - 1],
      temps: list.map((p) => p.temperature_c).filter((t): t is number => t != null),
      wears: list.map((p) => p.wear_pct).filter((w): w is number => w != null),
    }));
  });

  /** SVG polyline points for a small line chart. */
  function line(values: number[], w = 160, h = 34): string {
    if (values.length < 2) return "";
    const min = Math.min(...values), max = Math.max(...values);
    const span = max - min || 1;
    return values.map((v, i) => `${((i / (values.length - 1)) * w).toFixed(1)},${(h - 3 - ((v - min) / span) * (h - 6)).toFixed(1)}`).join(" ");
  }

  function since(day: string): string {
    return new Date(day + "T00:00:00").toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
  }
</script>

{#if trends.length}
  <section class="panel trends">
    <span class="kicker">Drive health over time</span>
    <p class="muted">One reading a day, kept for a year. Syscura warns when wear climbs fast, a drive runs hot, or its health drops.</p>
    {#each trends as t (t.disk)}
      <div class="row">
        <div class="name"><b>{t.disk}</b><span class="faint">since {since(t.first.day)} · {t.days} reading{t.days === 1 ? "" : "s"}</span></div>
        <div class="stat">
          <span class="faint">Health</span>
          <b class:bad={t.last.health === "Unhealthy"} class:warn={t.last.health === "Warning"}>{t.last.health}</b>
        </div>
        <div class="stat">
          <span class="faint">Wear</span>
          <b>{t.wears.length ? `${t.wears[t.wears.length - 1]} %` : "—"}{t.wears.length > 1 ? ` (+${t.wears[t.wears.length - 1] - t.wears[0]})` : ""}</b>
        </div>
        <div class="stat">
          <span class="faint">Temperature</span>
          {#if t.temps.length > 1}
            <svg viewBox="0 0 160 34" width="160" height="34" role="img" aria-label="Temperature, {Math.min(...t.temps)} to {Math.max(...t.temps)} °C">
              <polyline points={line(t.temps)} fill="none" stroke="var(--brand)" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" />
            </svg>
            <span class="faint small">{Math.min(...t.temps).toFixed(0)}–{Math.max(...t.temps).toFixed(0)} °C</span>
          {:else}
            <b>{t.temps.length ? `${t.temps[0].toFixed(0)} °C` : "—"}</b>
          {/if}
        </div>
      </div>
    {/each}
  </section>
{/if}

<style>
  .trends { padding: 30px 36px; gap: 10px; }
  .trends p { margin: 0; font-size: 14px; }
  .row { display: grid; grid-template-columns: minmax(0, 1.4fr) repeat(3, minmax(0, 1fr)); gap: 16px; align-items: center; padding: 12px 0; border-top: 1px solid var(--line); font-size: 14px; }
  .name, .stat { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .name b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .faint { font-size: 12.5px; }
  .small { font-size: 12px; }
  .bad { color: var(--bad); }
  .warn { color: var(--warn); }
  @media (max-width: 760px) { .row { grid-template-columns: 1fr 1fr; } }
</style>
