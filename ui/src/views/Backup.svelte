<script lang="ts">
  import { onMount } from "svelte";
  import { api, driveSize } from "../lib/api";
  import type { BackupInfo, Prefs } from "../lib/types";

  let info = $state<BackupInfo | null>(null);
  let chosen = $state<Record<string, boolean>>({ desktop: true, documents: true, pictures: true, videos: false, music: false, downloads: false });
  let dest = $state("");
  let msg = $state<{ text: string; bad?: boolean } | null>(null);
  let rpMsg = $state("");

  // Repeating backup
  let prefs = $state<Prefs | null>(null);
  let every = $state(7);
  let schedMsg = $state<{ text: string; bad?: boolean } | null>(null);
  async function loadPrefs() {
    try {
      prefs = await api.appPrefs();
      if (prefs.backup_schedule) every = prefs.backup_schedule.every_days;
    } catch { /* fine */ }
  }
  async function saveSchedule(on: boolean) {
    if (!prefs) return;
    const folders = Object.entries(chosen).filter(([, v]) => v).map(([k]) => k);
    const next: Prefs = { ...prefs, backup_schedule: on ? { every_days: every, destination: dest, folders, last_ms: 0 } : null };
    try {
      await api.setAppPrefs(next);
      prefs = next;
      schedMsg = { text: on ? `Saved. Every ${every} day${every === 1 ? "" : "s"}, ${folders.length} folder${folders.length === 1 ? "" : "s"} go to ${dest}Syscura Backup\Scheduled.` : "The repeating backup is off." };
    } catch (e) {
      schedMsg = { text: String(e), bad: true };
    }
  }

  async function load() {
    try {
      info = await api.backupInfo();
      if (!dest && info.targets.length) dest = info.targets.find((t) => !t.system_drive)?.root ?? info.targets[0].root;
    } catch (e) {
      msg = { text: String(e), bad: true };
    }
  }

  onMount(() => {
    load();
    loadPrefs();
    const t = setInterval(() => { if (info?.status.running) load(); }, 1500);
    return () => clearInterval(t);
  });

  const target = $derived(info?.targets.find((t) => t.root === dest));

  async function start() {
    msg = null;
    try {
      const where = await api.backupStart(dest, Object.entries(chosen).filter(([, v]) => v).map(([k]) => k));
      msg = { text: `Backing up to ${where}…` };
      await load();
    } catch (e) {
      msg = { text: String(e), bad: true };
    }
  }

  async function restorePoint() {
    try {
      rpMsg = await api.applyAction({ finding: null, title: "Restore point (made by you)", action: "restore_point", params: {}, label: "Create a restore point", automatic: false });
    } catch (e) {
      rpMsg = String(e).replace("agent_offline", "Start protection first.");
    }
  }
</script>

<div class="bk">
  <section class="panel" style="padding: 36px 44px; gap: 14px">
    <span class="eyebrow">Backup</span>
    <h1 class="title-xl">Copy your files to another drive. <span class="accent">Nothing is deleted.</span></h1>
    <span class="muted">Syscura only <b>copies</b>: it never deletes, moves or overwrites anything in your folders. Do this right away when Syscura warns about a failing drive, a virus or repeated crashes.</span>
  </section>

  {#if info}
    <section class="panel">
      <h3>1. What to back up</h3>
      <div class="folders">
        {#each info.folders as f}
          <label class="check"><input type="checkbox" bind:checked={chosen[f.id]} /> <span><b>{f.name}</b> <span class="faint mono">{f.path}</span></span></label>
        {/each}
      </div>

      <h3>2. Where to</h3>
      <div class="targets">
        {#each info.targets as t}
          <label class="target" class:on={dest === t.root}>
            <input type="radio" name="dest" value={t.root} bind:group={dest} />
            <span><b>{t.root}</b> {t.label} {t.removable ? "(USB / removable)" : ""}<br />
              <span class="faint">{driveSize(t.free_bytes)} free of {driveSize(t.size_bytes)}</span></span>
          </label>
        {/each}
      </div>
      {#if target?.system_drive}
        <p class="warn">This is the drive Windows is on. A copy here protects against mistakes, but not against this drive failing. Use another drive or a USB drive if you can.</p>
      {/if}

      <div class="go">
        <button class="btn" disabled={info.status.running || !dest} onclick={start}>{info.status.running ? "Backing up…" : "Back up now"}</button>
        {#if msg}<span class:bad={msg.bad} class="m">{msg.text}</span>{/if}
      </div>

      {#if info.status.destination}
        <div class="status">
          {#if info.status.running}<p>⏳ Copying <b>{info.status.current}</b>… (large folders can take a while; you can keep using the PC)</p>{/if}
          {#each info.status.done as d}<p class:bad={!d.ok}>{d.ok ? "✓" : "⚠"} {d.name} {d.detail ? `— ${d.detail}` : ""}</p>{/each}
          {#if info.status.finished_ms}
            <p class="ok">Finished. Your copy is in <button class="link" onclick={() => api.open(info!.status.destination)}>{info.status.destination}</button></p>
          {/if}
        </div>
      {/if}
    </section>

    <section class="panel">
      <h3>Repeat automatically</h3>
      <p class="muted">While Syscura runs in the tray, it repeats this backup by itself, using the folders and drive chosen above. Each run copies only what changed into the same <b>Scheduled</b> folder, and never deletes anything. If the drive is unplugged, it waits and reminds you.</p>
      {#if prefs?.backup_schedule}
        <p><b>On:</b> every {prefs.backup_schedule.every_days} day{prefs.backup_schedule.every_days === 1 ? "" : "s"} to {prefs.backup_schedule.destination}{prefs.backup_schedule.last_ms ? `, last ran ${new Date(prefs.backup_schedule.last_ms).toLocaleDateString()}` : ", first run within a minute"}.</p>
      {/if}
      <div class="go">
        <label class="check">Every <input class="days" type="number" min="1" max="31" bind:value={every} /> days</label>
        <button class="btn btn--sm" disabled={!dest} onclick={() => saveSchedule(true)}>{prefs?.backup_schedule ? "Update schedule" : "Turn on"}</button>
        {#if prefs?.backup_schedule}<button class="btn btn--ghost btn--sm" onclick={() => saveSchedule(false)}>Turn off</button>{/if}
        {#if schedMsg}<span class:bad={schedMsg.bad} class="m">{schedMsg.text}</span>{/if}
      </div>
    </section>

    <section class="panel">
      <h3>System restore point</h3>
      <p class="muted">Saves Windows' settings and system files so you can roll back if an update or driver breaks something. Your own files are not included (use the backup above for those). Needs the Syscura service.</p>
      <div class="go"><button onclick={restorePoint}>Create a restore point</button>{#if rpMsg}<span class="m">{rpMsg}</span>{/if}</div>
    </section>
  {:else}
    <p class="muted">Loading…</p>
  {/if}
</div>

<style>
  .bk { display: flex; flex-direction: column; gap: var(--gap); max-width: 960px; }
  section.panel:not(:first-child) { padding: 32px 40px; gap: 12px; }
  h3 { margin: 6px 0 0; font-size: 18px; font-weight: 600; letter-spacing: -0.01em; }
  .folders { display: flex; flex-direction: column; gap: 4px; }
  .check { display: flex; gap: 8px; align-items: center; font-size: 14.5px; cursor: pointer; }
  .check input, .target input { accent-color: var(--accent); }
  .targets { display: flex; gap: 8px; flex-wrap: wrap; }
  .target { display: flex; gap: 8px; align-items: flex-start; border: 1px solid var(--line-2); border-radius: var(--r-box); padding: 12px 16px; cursor: pointer; font-size: 14px; }
  .target.on { border-color: var(--accent); background: var(--accent-soft); }
  .go { display: flex; gap: 10px; align-items: center; margin-top: 4px; }
  .m { font-size: 13px; color: var(--accent); }
  .bad { color: var(--danger) !important; }
  .warn { margin: 0; font-size: 13px; color: var(--warn); }
  .status { background: var(--panel-2); border-radius: var(--r-box); padding: 14px 18px; font-size: 14px; }
  .status p { margin: 3px 0; }
  .ok { color: var(--ok); }
  .days { width: 64px; padding: 5px 10px; margin: 0 4px; }
  section p { margin: 0; font-size: 14.5px; }
</style>
