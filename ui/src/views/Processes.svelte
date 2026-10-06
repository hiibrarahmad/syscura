<script lang="ts">
  // Every running program, live: what it is, where it runs from, whether its
  // signature is valid, and why Syscura warns about it. The agent reads this
  // only while this page is open, so it costs nothing otherwise.
  import { onMount } from "svelte";
  import { api, mb } from "../lib/api";
  import { askWebAi, searchWeb } from "../lib/web";
  import type { ProcessInfo } from "../lib/types";

  let list = $state<ProcessInfo[]>([]);
  let offline = $state(false);
  let loading = $state(true);
  let mode = $state<"all" | "warn" | "unsigned" | "apps">("all");
  let search = $state("");
  let open = $state<number | null>(null);
  let msg = $state<{ pid: number; text: string; bad?: boolean } | null>(null);
  let stopping = $state<ProcessInfo | null>(null);

  async function load() {
    try {
      list = await api.processes();
      offline = false;
    } catch (e) {
      offline = String(e).includes("agent_offline");
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    load();
    const t = setInterval(load, 3000);
    return () => clearInterval(t);
  });

  const warnings = $derived(list.filter((p) => p.warning));
  const unsigned = $derived(list.filter((p) => p.signature === "unsigned" || p.signature === "invalid"));
  const totalMem = $derived(list.reduce((a, p) => a + p.memory_bytes, 0));
  const totalCpu = $derived(Math.min(100, list.reduce((a, p) => a + p.cpu_pct, 0)));
  const shown = $derived(
    list
      .filter((p) =>
        mode === "warn" ? !!p.warning : mode === "unsigned" ? p.signature === "unsigned" || p.signature === "invalid" : mode === "apps" ? p.location !== "system" : true,
      )
      .filter((p) => !search.trim() || `${p.name} ${p.path} ${p.pid} ${p.signer}`.toLowerCase().includes(search.trim().toLowerCase()))
      .sort((a, b) => (b.warning ? 1 : 0) - (a.warning ? 1 : 0) || b.cpu_pct - a.cpu_pct || b.memory_bytes - a.memory_bytes),
  );

  const placeText: Record<string, string> = { system: "Windows", program_files: "Program Files", user: "User folder", other: "Other folder" };
  function sigPill(p: ProcessInfo): { t: string; s: string } {
    if (p.signature === "valid") return { t: "ok", s: p.signer && p.signer.length < 28 ? p.signer : "Signed" };
    if (p.signature === "unsigned") return { t: p.location === "user" ? "warn" : "quiet", s: "Not signed" };
    if (p.signature === "invalid") return { t: "bad", s: "Bad signature" };
    if (!p.path) return { t: "quiet", s: "Protected" };
    return { t: "quiet", s: p.signature === "unknown" ? "Unknown" : "Checking…" };
  }

  function since(ms: number): string {
    if (!ms) return "";
    const s = Math.max(0, (Date.now() - ms) / 1000);
    if (s < 3600) return `${Math.floor(s / 60)} min`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`;
    return `${Math.floor(s / 86400)} days`;
  }

  async function scan(p: ProcessInfo) {
    try {
      msg = { pid: p.pid, text: await api.applyAction({ finding: null, title: `Defender scan of ${p.name}`, action: "defender.scan_path", params: { path: p.path }, label: `Scan ${p.name} with Defender`, automatic: false }) + " The result appears on the Problems page." };
    } catch (e) {
      msg = { pid: p.pid, text: String(e).replace("agent_offline", "Start protection first."), bad: true };
    }
  }
  async function stop(p: ProcessInfo) {
    stopping = null;
    try {
      msg = { pid: p.pid, text: await api.applyAction({ finding: null, title: `Stopped ${p.name}`, action: "process.stop", params: { pid: String(p.pid), name: p.name }, label: `Stop ${p.name}`, automatic: false }) };
      setTimeout(load, 1500);
    } catch (e) {
      msg = { pid: p.pid, text: String(e), bad: true };
    }
  }
  const question = (p: ProcessInfo) => ({
    title: `Is "${p.name}" safe? (running on Windows)`,
    explanation: p.warning ? `Syscura warns: ${p.warning}.` : "",
    details: { "Program file": p.path || "not readable", Signature: p.signature === "valid" ? `valid, signed by ${p.signer}` : p.signature || "not checked", "Started by": p.parent_name || "unknown" },
    system: "",
  });
</script>

<section class="panel panel--brand summary" style="padding: 30px 36px">
  <div style="display: flex; flex-direction: column; gap: 6px">
    <span class="muted" style="font-size: 14px">Running right now · updates every 3 seconds</span>
    <span class="sline">{list.length} programs running. {warnings.length === 0 ? "None look suspicious." : `${warnings.length} need${warnings.length === 1 ? "s" : ""} a look.`}</span>
  </div>
  <div class="stat"><b>{warnings.length}</b><span>warnings</span></div>
  <div class="stat"><b>{unsigned.length}</b><span>not signed</span></div>
  <div class="stat"><b>{Math.round(totalCpu)} %</b><span>processor</span></div>
  <div class="stat"><b>{(totalMem / 1024 ** 3).toFixed(1)} GB</b><span>memory in use</span></div>
</section>

<div class="row-chips">
  <button class="chip" aria-pressed={mode === "all"} onclick={() => (mode = "all")}>Everything</button>
  <button class="chip" aria-pressed={mode === "warn"} onclick={() => (mode = "warn")}>Warnings · {warnings.length}</button>
  <button class="chip" aria-pressed={mode === "unsigned"} onclick={() => (mode = "unsigned")}>Not signed · {unsigned.length}</button>
  <button class="chip" aria-pressed={mode === "apps"} onclick={() => (mode = "apps")}>Apps only</button>
  <label for="pr-search" class="muted" style="margin-left: auto; font-size: 14px">Search</label>
  <input id="pr-search" class="search" type="text" placeholder="Name, folder or publisher" bind:value={search} />
</div>

<section class="panel timeline">
  {#if loading}
    <p class="muted empty">Reading running programs…</p>
  {:else if offline}
    <div class="empty"><b>Background protection is off.</b><p class="muted">Press <b>Start</b> on the Overview page to watch running programs.</p></div>
  {:else if shown.length === 0}
    <p class="muted empty">{search ? "Nothing matches your search." : "Nothing here. That's a good sign."}</p>
  {:else}
    <div class="head row">
      <span>Program</span><span class="num">Processor</span><span class="num">Memory</span><span>Signature</span>
    </div>
    {#each shown as p (p.pid)}
      {@const sig = sigPill(p)}
      <div class="item" class:warn={!!p.warning}>
        <button class="row" aria-expanded={open === p.pid} onclick={() => (open = open === p.pid ? null : p.pid)}>
          <span class="main">
            <b>{p.name}</b>
            <small>{p.warning ? `⚠ ${p.warning}` : `${placeText[p.location]} · process ${p.pid}`}</small>
          </span>
          <span class="num">{p.cpu_pct >= 0.1 ? `${p.cpu_pct.toFixed(1)} %` : "0 %"}</span>
          <span class="num">{mb(p.memory_bytes)}</span>
          <span><span class="pill pill--{p.warning ? 'bad' : sig.t}">{p.warning ? "Warning" : sig.s}</span></span>
        </button>
        {#if open === p.pid}
          <div class="detail">
            <dl class="specs">
              <div><dt>Program file</dt><dd class="mono">{p.path || "Windows does not show it (protected system process)"}</dd></div>
              <div><dt>Publisher</dt><dd>{p.signature === "valid" ? p.signer : p.signature === "unsigned" ? "Not signed" : p.signature === "invalid" ? `Signature NOT valid (${p.signer})` : "Not checked yet"}</dd></div>
              <div><dt>Runs from</dt><dd>{placeText[p.location]}</dd></div>
              <div><dt>Started by</dt><dd>{p.parent_name ? `${p.parent_name} (process ${p.parent})` : "A program that has ended"}</dd></div>
              <div><dt>Running for</dt><dd>{since(p.started_ms) || "Not reported"}</dd></div>
              <div><dt>Threads</dt><dd>{p.threads}</dd></div>
            </dl>
            <div class="row-chips">
              {#if p.path}
                <button class="btn btn--ghost btn--sm" onclick={() => api.reveal(p.path)}>Open folder</button>
                <button class="btn btn--sm" onclick={() => scan(p)}>Scan with Defender</button>
              {/if}
              {#if p.location !== "system"}<button class="btn btn--ghost btn--sm danger" onclick={() => (stopping = p)}>Stop…</button>{/if}
              <button class="btn btn--ghost btn--sm" onclick={() => searchWeb(`${p.name} process is it safe`)}>Search the web</button>
              <button class="btn btn--ghost btn--sm" onclick={() => askWebAi("google", question(p))}>Ask Google AI</button>
            </div>
            {#if msg?.pid === p.pid}<p class="msg" class:bad={msg.bad}>{msg.text}</p>{/if}
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</section>

{#if stopping}
  {@const s = stopping}
  <div class="overlay" role="presentation" onclick={() => (stopping = null)}>
    <div class="panel dialog" role="dialog" aria-modal="true" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && (stopping = null)}>
      <span class="pill pill--warn" style="align-self: flex-start">Asks first</span>
      <span class="title-m">Stop {s.name}?</span>
      <p class="muted">The program closes right away and anything unsaved in it is lost. Its file is not deleted, so it may start again later. Windows' own critical programs are never stopped.</p>
      <div class="row-chips" style="justify-content: flex-end">
        <button class="btn btn--ghost" onclick={() => (stopping = null)}>Cancel</button>
        <button class="btn" onclick={() => stop(s)}>Stop it</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .sline { font-size: 32px; font-weight: 600; letter-spacing: -0.03em; line-height: 1.1; }
  .empty { padding: 30px 0; text-align: center; }
  .row { display: grid; grid-template-columns: minmax(0, 1fr) 90px 100px 170px; gap: 16px; align-items: center; width: 100%; text-align: left; background: none; border: 0; border-radius: 0; color: inherit; padding: 14px 0; font-size: inherit; font-weight: inherit; }
  .row:hover { border: 0; }
  button.row:hover b { color: var(--brand); }
  .head { font-size: 13px; color: var(--muted); font-weight: 600; padding: 8px 0; border-bottom: 1px solid var(--line); }
  .item { border-bottom: 1px solid var(--line); }
  .item:last-child { border-bottom: 0; }
  .item.warn .main small { color: var(--bad-ink); }
  .main { min-width: 0; display: flex; flex-direction: column; }
  .main b { font-weight: 600; font-size: 15px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .main small { color: var(--muted); font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .num { text-align: right; font-variant-numeric: tabular-nums; }
  .detail { padding: 0 0 18px; display: flex; flex-direction: column; gap: 12px; }
  .msg { margin: 0; font-size: 14px; color: var(--brand); }
  .msg.bad { color: var(--bad); }
  .danger { color: var(--bad-ink); }
  .overlay { position: fixed; inset: 0; background: rgba(14, 13, 31, 0.45); display: grid; place-items: center; z-index: 10; }
  .dialog { width: min(480px, 92vw); padding: 30px 32px; gap: 14px; }
  .dialog p { margin: 0; font-size: 14px; }
  @media (max-width: 1000px) { .row { grid-template-columns: minmax(0, 1fr) 70px 90px; } .row > :last-child { display: none; } }
</style>
