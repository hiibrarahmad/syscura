<script lang="ts">
  import { ai, refreshAi } from "../lib/ai.svelte";
  import { api } from "../lib/api";

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
  .msg { color: var(--ok); }
  .msg.bad { color: var(--danger); }
  .small { font-size: 13.5px; margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 4px; }
</style>
