<script lang="ts">
  import { onMount } from "svelte";
  import AiPanel from "../lib/AiPanel.svelte";
  import { api } from "../lib/api";
  import type { Question, StoredEvent } from "../lib/types";

  let events = $state<StoredEvent[]>([]);
  let mode = $state<"look" | "all" | "routine">("look");
  let offline = $state(false);
  let loading = $state(true);
  let search = $state("");
  let open = $state<number | null>(null);

  async function load() {
    try {
      events = await api.events(500, false);
      offline = false;
    } catch (e) {
      offline = String(e).includes("agent_offline");
      events = [];
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    load();
    const t = setInterval(load, 6000);
    return () => clearInterval(t);
  });

  const message = (e: StoredEvent) => e.data._message ?? "";
  const worth = (e: StoredEvent) => e.level === "critical" || e.level === "error" || e.level === "warning" || e.source === "hardware";
  const counts = $derived({
    critical: events.filter((e) => e.level === "critical").length,
    error: events.filter((e) => e.level === "error").length,
    warning: events.filter((e) => e.level === "warning").length,
    info: events.filter((e) => e.level === "info" || e.level === "verbose").length,
  });
  const lookCount = $derived(counts.critical + counts.error);
  const shown = $derived(
    events
      .filter((e) => (mode === "all" ? true : mode === "look" ? worth(e) : !worth(e)))
      .filter((e) =>
        !search.trim() ||
        `${e.provider} ${e.event_id} ${e.channel} ${message(e)} ${Object.values(e.data).join(" ")}`.toLowerCase().includes(search.trim().toLowerCase()),
      ),
  );

  function summary(e: StoredEvent): string {
    if (e.source === "hardware") return `${e.data.change}: ${e.data.part}`;
    const m = message(e);
    if (m) return m.split(/\r?\n/)[0];
    const vals = Object.entries(e.data).filter(([k, v]) => !k.startsWith("_") && v && v.length < 120).slice(0, 3);
    return vals.map(([k, v]) => (/^\d+$/.test(k) ? v : `${k}: ${v}`)).join(" · ") || `${e.provider} event ${e.event_id}`;
  }

  const marker = (e: StoredEvent) => (e.level === "critical" || e.level === "error" ? "bad" : e.level === "warning" ? "brand" : "");
  const levelText: Record<string, string> = { critical: "Critical", error: "Error", warning: "Warning", info: "Information", verbose: "Detail" };
  const pill = (e: StoredEvent) =>
    e.level === "critical" || e.level === "error" ? { t: "bad", s: levelText[e.level] } : e.level === "warning" ? { t: "warn", s: "Warning" } : { t: "quiet", s: "Routine" };

  function when(ms: number): string {
    const d = new Date(ms);
    return d.toDateString() === new Date().toDateString()
      ? d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
      : d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }

  function question(e: StoredEvent): Question {
    const details: Record<string, string> = {
      "Log": e.channel,
      "Source": e.provider,
      "Event ID": String(e.event_id),
      "Level": e.level,
      "When": new Date(e.ts).toLocaleString(),
    };
    if (message(e)) details["Windows message"] = message(e);
    for (const [k, v] of Object.entries(e.data)) if (!k.startsWith("_") && v && v.length < 600) details[k] = v;
    return { title: `${e.provider} event ${e.event_id}`, explanation: "", details, system: "" };
  }
</script>

<section class="panel panel--brand summary" style="padding: 30px 36px">
  <div style="display: flex; flex-direction: column; gap: 6px">
    <span class="muted" style="font-size: 14px">Last 500 recorded events</span>
    <span class="sline">{events.length.toLocaleString()} events read. {lookCount === 0 ? "None need you." : `${lookCount.toLocaleString()} worth a look.`}</span>
  </div>
  <div class="stat"><b>{counts.critical.toLocaleString()}</b><span>critical</span></div>
  <div class="stat"><b>{counts.error.toLocaleString()}</b><span>errors</span></div>
  <div class="stat"><b>{counts.warning.toLocaleString()}</b><span>warnings</span></div>
  <div class="stat"><b>{counts.info.toLocaleString()}</b><span>information</span></div>
</section>

<div class="row-chips">
  <button class="chip" aria-pressed={mode === "look"} onclick={() => (mode = "look")}>Worth a look</button>
  <button class="chip" aria-pressed={mode === "all"} onclick={() => (mode = "all")}>Everything</button>
  <button class="chip" aria-pressed={mode === "routine"} onclick={() => (mode = "routine")}>Routine only</button>
  <label for="ev-search" class="muted" style="margin-left: auto; font-size: 14px">Search</label>
  <input id="ev-search" class="search" type="text" placeholder="Event ID or words" bind:value={search} />
</div>

<section class="panel timeline">
  {#if loading}
    <p class="muted empty">Loading…</p>
  {:else if offline}
    <div class="empty"><b>Background protection is off.</b><p class="muted">Press <b>Start</b> on the Overview page to record events.</p></div>
  {:else if shown.length === 0}
    <p class="muted empty">{search ? "No events match your search." : "Nothing here. That's a good sign."}</p>
  {:else}
    {#each shown as e (e.id)}
      {@const p = pill(e)}
      <div class="event-wrap">
        <button class="event" aria-expanded={open === e.id} onclick={() => (open = open === e.id ? null : e.id)}>
          <time title={new Date(e.ts).toLocaleString()}>{when(e.ts)}</time>
          <span class="marker marker--{marker(e)}"></span>
          <div class="ebody">
            <b>{summary(e)}</b>
            <small>{levelText[e.level] ?? e.level} · {e.source === "hardware" ? "Hardware" : e.provider.replace("Microsoft-Windows-", "")} · ID {e.event_id}</small>
          </div>
          <span class="pill pill--{p.t}">{p.s}</span>
        </button>
        {#if open === e.id}
          <div class="detail">
            {#if message(e)}<p class="box msg">{message(e)}</p>{/if}
            <dl class="specs">
              <div><dt>When</dt><dd>{new Date(e.ts).toLocaleString()}</dd></div>
              <div><dt>Log</dt><dd>{e.channel}</dd></div>
              <div><dt>Source</dt><dd>{e.provider}</dd></div>
              <div><dt>Event ID</dt><dd>{e.event_id}</dd></div>
              {#each Object.entries(e.data).filter(([k, v]) => !k.startsWith("_") && v) as [k, v]}<div><dt>{k}</dt><dd class="mono">{v}</dd></div>{/each}
            </dl>
            {#if e.source !== "hardware"}
              <AiPanel qkey="event:{e.id}" question={() => question(e)} finding={null} />
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</section>

<style>
  .sline { font-size: 32px; font-weight: 600; letter-spacing: -0.03em; line-height: 1.1; }
  .empty { padding: 30px 0; text-align: center; }
  .event-wrap { border-bottom: 1px solid var(--line); }
  .event-wrap:last-child { border-bottom: 0; }
  .event { width: 100%; text-align: left; background: none; border: 0; border-radius: 0; color: inherit; padding: 18px 0; border-bottom: 0; font-size: inherit; font-weight: inherit; }
  .event:hover { border: 0; }
  .event:hover b { color: var(--brand); }
  .ebody { min-width: 0; }
  .ebody b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .detail { padding: 0 0 20px 102px; display: flex; flex-direction: column; gap: 14px; user-select: text; }
  .msg { margin: 0; white-space: pre-line; font-size: 14px; }
  @media (max-width: 1000px) { .detail { padding-left: 0; } }
</style>
