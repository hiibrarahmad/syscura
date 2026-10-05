<script lang="ts">
  import { ago, api } from "../lib/api";
  import type { StoredEvent } from "../lib/types";

  let events = $state<StoredEvent[]>([]);
  let errorsOnly = $state(false);
  let offline = $state(false);
  let loading = $state(true);

  async function load() {
    try {
      events = await api.events(300, errorsOnly);
      offline = false;
    } catch (e) {
      offline = String(e).includes("agent_offline");
      events = [];
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    errorsOnly;
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  });

  function summary(e: StoredEvent): string {
    if (e.source === "hardware") return `${e.data.change}: ${e.data.part}`;
    const vals = Object.entries(e.data).filter(([, v]) => v && v.length < 120).slice(0, 3);
    return vals.map(([k, v]) => (/^\d+$/.test(k) ? v : `${k}: ${v}`)).join(" · ");
  }
</script>

<div class="ev">
  <div class="toolbar">
    <h2>Event log</h2>
    <span class="muted">Critical, error and warning events Windows recorded, plus hardware changes Syscura noticed.</span>
    <label class="check"><input type="checkbox" bind:checked={errorsOnly} /> Errors only</label>
  </div>

  <div class="list card">
    {#if loading}
      <p class="empty muted">Loading…</p>
    {:else if offline}
      <div class="empty">
        <p><b>The Syscura agent is not running.</b></p>
        <p class="muted">It records problems in the background. Start it with <span class="mono">syscura-agent run</span>, or from an admin terminal install it as a service with <span class="mono">syscura-agent install</span>.</p>
      </div>
    {:else if events.length === 0}
      <p class="empty muted">Nothing recorded yet. That's a good sign.</p>
    {:else}
      <table>
        <thead><tr><th>When</th><th>Level</th><th>Source</th><th>ID</th><th>Provider</th><th>Details</th></tr></thead>
        <tbody>
          {#each events as e (e.id)}
            <tr>
              <td class="faint" title={new Date(e.ts).toLocaleString()}>{ago(e.ts)}</td>
              <td><span class="lvl l-{e.level}">{e.level}</span></td>
              <td>{e.source === "hardware" ? "Hardware" : e.channel}</td>
              <td class="mono">{e.event_id}</td>
              <td>{e.provider}</td>
              <td class="details">{summary(e)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>

<style>
  .ev { display: flex; flex-direction: column; gap: 10px; height: 100%; }
  .toolbar { display: flex; align-items: baseline; gap: 12px; }
  h2 { margin: 0; font-size: 18px; }
  .toolbar .muted { flex: 1; font-size: 12.5px; }
  .check { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 13px; cursor: pointer; }
  .check input { accent-color: var(--accent); }
  .list { flex: 1; min-height: 0; overflow: auto; user-select: text; }
  .empty { padding: 30px; text-align: center; }
  .empty p { margin: 6px 0; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { position: sticky; top: 0; background: var(--panel); text-align: left; font-weight: 600; color: var(--faint); font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.06em; padding: 10px 12px; border-bottom: 1px solid var(--line); }
  td { padding: 8px 12px; border-bottom: 1px solid var(--line); vertical-align: top; white-space: nowrap; }
  td.details { white-space: normal; color: var(--muted); max-width: 520px; }
  .lvl { font-size: 11.5px; font-weight: 600; padding: 2px 8px; border-radius: 99px; text-transform: capitalize; }
  .l-critical, .l-error { background: rgba(255, 92, 108, 0.14); color: var(--danger); }
  .l-warning { background: rgba(244, 183, 64, 0.14); color: var(--warn); }
  .l-info, .l-verbose { background: var(--accent-soft); color: var(--accent); }
</style>
