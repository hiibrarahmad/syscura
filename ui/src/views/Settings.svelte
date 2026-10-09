<script lang="ts">
  import { ai, refreshAi } from "../lib/ai.svelte";
  import { api } from "../lib/api";
  import { LINKS } from "../lib/links";
  import SocialLinks from "../lib/SocialLinks.svelte";
  import { onMount } from "svelte";
  import type { Prefs, StatusInfo } from "../lib/types";

  let agent = $state<StatusInfo | null>(null);
  type Update = Awaited<ReturnType<typeof api.updateCheck>>;
  let upd = $state<Update | null>(null);
  let updMsg = $state<{ text: string; bad?: boolean } | null>(null);
  let updBusy = $state(false);
  async function checkUpdate(force: boolean) {
    updBusy = true;
    updMsg = null;
    try { upd = await api.updateCheck(force); } catch (e) { updMsg = { text: String(e), bad: true }; } finally { updBusy = false; }
  }
  async function installUpdate() {
    updBusy = true;
    updMsg = { text: "Downloading the update and checking it…" };
    try { updMsg = { text: await api.updateInstall() }; } catch (e) { updMsg = { text: String(e), bad: true }; updBusy = false; }
  }
  let prefs = $state<Prefs | null>(null);
  async function setPref(change: Partial<Prefs>) {
    if (!prefs) return;
    const next = { ...prefs, ...change };
    try { await api.setAppPrefs(next); prefs = next; } catch (e) { updMsg = { text: String(e), bad: true }; }
  }
  onMount(() => {
    api.appPrefs().then((p) => (prefs = p)).catch(() => {});
    api.agentStatus().then((s) => (agent = s)).catch(() => {});
    checkUpdate(false);
  });

  let key = $state("");
  let busy = $state(false);
  let msg = $state<{ text: string; bad?: boolean } | null>(null);

  async function save() {
    busy = true;
    msg = null;
    try {
      const model = await api.aiSaveKey(key);
      key = "";
      msg = { text: `Connected. Syscura will use ${model}.` };
      await refreshAi();
    } catch (e) {
      msg = { text: String(e), bad: true };
    } finally {
      busy = false;
    }
  }

  async function forget() {
    await api.aiForgetKey();
    msg = { text: "The key was removed from this PC." };
    await refreshAi();
  }

  async function toggleAuto(e: Event) {
    await api.aiSetAutoFix((e.currentTarget as HTMLInputElement).checked);
    await refreshAi();
  }
</script>

<div class="st">
  <section class="panel" style="padding: 36px 44px; gap: 14px">
    <span class="eyebrow">Settings</span>
    <h1 class="title-xl">Free AI help. <span class="accent">You stay in charge.</span></h1>
    <span class="muted">No key? Every problem and event still has <b>Search the web</b> and <b>Ask ChatGPT / Claude / Copilot / Gemini</b> buttons that open your browser.</span>
  </section>

  <section class="panel">
    <span class="kicker">AI help with Google Gemini · free key</span>
    <p class="muted">
      When something goes wrong, the AI searches the web for that exact error on your Windows version, explains it in plain words,
      and picks fixes. Syscura still decides what is allowed: the AI can only choose from Syscura's list of safe actions —
      it cannot run commands — and anything that changes your PC asks you first.
    </p>

    {#if ai.status.configured}
      <p class="okline"><span class="dot"></span>Connected · model <b>{ai.status.model ?? "chosen on first use"}</b></p>
      <label class="check">
        <input type="checkbox" checked={ai.status.auto_fix} onchange={toggleAuto} />
        <span><b>Fix safe problems automatically.</b> While Syscura is open, new problems are sent to the AI; if it picks a <i>safe</i> fix
        (like starting a stopped service), Syscura runs it and checks it worked. Everything else waits for you.</span>
      </label>
      <div class="btns"><button onclick={forget}>Remove key</button></div>
    {:else}
      <ol class="how">
        <li>Open <button class="link" onclick={() => api.open("https://aistudio.google.com/apikey")}>aistudio.google.com/apikey</button> and sign in with any Google account.</li>
        <li>Press <b>Create API key</b> and copy it. It is free; no card is needed.</li>
        <li>Paste it here.</li>
      </ol>
      <div class="keyrow">
        <input type="password" placeholder="Paste your Gemini API key" bind:value={key} autocomplete="off" />
        <button class="primary" disabled={busy || key.trim().length < 20} onclick={save}>{busy ? "Checking…" : "Connect"}</button>
      </div>
    {/if}
    {#if msg}<p class:bad={msg.bad} class="msg">{msg.text}</p>{/if}

    <h4>Privacy</h4>
    <ul class="muted small">
      <li>The key is stored in Windows Credential Manager, encrypted for your Windows account.</li>
      <li>Only the problem (title, Windows' message, event details) and a one-line description of your Windows version and hardware are sent.</li>
      <li>Your user name, PC name, user-folder paths, e-mail and IP addresses are masked before anything leaves the PC.</li>
      <li>Free keys are subject to Google's terms; Google may use free-tier prompts to improve its products.</li>
    </ul>
  </section>

  <section class="panel">
    <span class="kicker">Background protection</span>
    {#if agent}
      <p><b>{agent.service ? "On, as a Windows service." : "On for this session only."}</b>
        {agent.service ? "It starts with Windows by itself, even before you sign in." : "It stops when you sign out. Install the service so it starts with Windows."}</p>
      <p class="muted">Syscura {agent.version} · using {(agent.working_set_bytes / 1048576).toFixed(1)} MB of memory · {agent.events_total.toLocaleString()} events recorded</p>
      {#if !agent.service}<div class="btns"><button class="btn btn--sm" onclick={async () => { try { msg = { text: await api.installService() }; } catch (e) { msg = { text: String(e), bad: true }; } agent = await api.agentStatus().catch(() => agent); }}>Install as a Windows service</button></div>{/if}
    {:else}
      <p><b>Off.</b> Open the Overview page and press <b>Start</b>.</p>
    {/if}
    <p class="muted">Closing the window keeps Syscura in the taskbar tray (the ^ arrow next to the clock). Right-click its icon for <b>Quit</b>. Background protection keeps running either way.</p>
  </section>

  <section class="panel">
    <span class="kicker">Updates</span>
    {#if upd?.available}
      <p><b>Syscura {upd.latest} is available.</b> You have {upd.current}.{upd.published ? ` Released ${upd.published}.` : ""}</p>
      <p class="muted">Syscura downloads it from GitHub, checks Syscura's signature and the published checksum, and installs it over this copy (one admin prompt). Your history, verdicts, settings and AI key are kept, and Syscura opens again when it is done.</p>
      <div class="btns">
        <button class="btn btn--sm" disabled={updBusy} onclick={installUpdate}>{updBusy ? "Working…" : `Download and install ${upd.latest}`}</button>
        <button class="btn btn--ghost btn--sm" onclick={() => api.open(upd!.page)}>What's new</button>
      </div>
    {:else if upd}
      <p><b>You have the latest version</b> ({upd.current}).</p>
      <p class="muted">{prefs?.auto_update_check === false ? "Automatic checks are off; press Check now when you like." : "Syscura checks GitHub once a day and tells you when there is a new version."}</p>
      <div class="btns"><button class="btn btn--ghost btn--sm" disabled={updBusy} onclick={() => checkUpdate(true)}>{updBusy ? "Checking…" : "Check now"}</button></div>
    {:else}
      <div class="btns"><button class="btn btn--ghost btn--sm" disabled={updBusy} onclick={() => checkUpdate(true)}>{updBusy ? "Checking…" : "Check for updates"}</button></div>
    {/if}
    {#if updMsg}<p class:bad={updMsg.bad} class="msg">{updMsg.text}</p>{/if}
    {#if prefs}
      <label class="check">
        <input type="checkbox" checked={prefs.auto_update_check} onchange={(e) => setPref({ auto_update_check: (e.currentTarget as HTMLInputElement).checked })} />
        <span><b>Check for updates once a day.</b> Syscura asks GitHub for the latest version number; nothing about your PC is sent. Updates are only installed when you press the button, and only if they carry Syscura's signature.</span>
      </label>
      <label class="check">
        <input type="checkbox" checked={prefs.weekly_summary} onchange={(e) => setPref({ weekly_summary: (e.currentTarget as HTMLInputElement).checked })} />
        <span><b>Weekly summary.</b> One notification a week: what Syscura found and fixed.</span>
      </label>
    {/if}
  </section>

  <section class="panel">
    <span class="kicker">About and help</span>
    <p>Syscura is free and open source (MIT). Made by <b>Ibrar Ahmad</b>.</p>
    <div class="links">
      <button class="btn btn--ghost btn--sm" onclick={() => api.open(LINKS.report)}>Report a problem</button>
      <button class="btn btn--ghost btn--sm" onclick={() => api.open(LINKS.hardware)}>Wrong hardware info?</button>
      <button class="btn btn--ghost btn--sm" onclick={() => api.open(LINKS.discussions)}>Ask a question</button>
      <button class="btn btn--ghost btn--sm" onclick={() => api.open(LINKS.releases)}>Check for updates</button>
      <button class="btn btn--ghost btn--sm" onclick={() => api.open(LINKS.repo)}>Source code</button>
    </div>
    <SocialLinks />
    <p class="muted">If Syscura helped you, a ⭐ on GitHub helps others find it.</p>
  </section>
</div>

<style>
  .st { display: flex; flex-direction: column; gap: var(--gap); max-width: 960px; }
  section.panel:not(:first-child) { padding: 32px 40px; gap: 14px; }
  h4 { margin: 8px 0 0; font-size: 14px; font-weight: 600; color: var(--muted); }
  p { margin: 0; font-size: 14.5px; }
  .how { margin: 0; padding-left: 20px; font-size: 14.5px; display: flex; flex-direction: column; gap: 6px; }
  .keyrow input { max-width: 460px; }
  .keyrow { display: flex; gap: 8px; }
  .check { display: flex; gap: 10px; align-items: flex-start; font-size: 14.5px; cursor: pointer; }
  .check input { margin-top: 3px; accent-color: var(--accent); }
  .okline { display: flex; align-items: center; gap: 8px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--ok); }
  .btns { display: flex; gap: 8px; }
  .links { display: flex; gap: 10px 18px; flex-wrap: wrap; align-items: center; }
  .msg { color: var(--ok); }
  .msg.bad { color: var(--danger); }
  .small { font-size: 13.5px; margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 4px; }
</style>
