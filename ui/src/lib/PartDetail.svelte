<script lang="ts">
  import type { BoardLayout, Part } from "./board/layout";
  import { sensorsAt } from "./board/layout";
  import type { HardwareInfo, PartImage as PartImageT, Sensor } from "./types";
  import { api, driveSize, fmtSensor, gb, tempTone } from "./api";
  import { decodeRamPart } from "./ramparts";
  import { portKey, setUsbPanel, usbPanels } from "./prefs.svelte";
  import type { Panel } from "./types";
  import PartImage from "./PartImage.svelte";

  let {
    hw,
    layout,
    partId,
    sensors,
    images,
    onclose,
    onchanged,
  }: {
    hw: HardwareInfo;
    layout: BoardLayout;
    partId: string;
    sensors: Sensor[];
    images: PartImageT[];
    onclose: () => void;
    onchanged: () => void;
  } = $props();

  const part = $derived<Part | undefined>(layout.parts.find((p) => p.id === partId));
  const image = $derived(part?.imageKey ? images.find((i) => i.key === part.imageKey) : undefined);
  const pins = $derived(layout.pins.filter((p) => p.partId === partId || (partId === "vrm_left" && p.partId === "vrm")));

  type Row = [string, string];
  interface Info { title: string; subtitle: string; rows: Row[]; connection?: string; tips: string[]; sensorList: Sensor[] }

  const info = $derived.by<Info>(() => {
    const i = computeInfo();
    // The board's own description of the slot, if it adds something.
    if (part?.desc && !i.tips.includes(part.desc) && (part.kind === "dimm" || part.kind === "pcie")) i.tips.push(part.desc);
    return i;
  });

  const usbDevice = $derived(part?.kind === "usb" ? hw.usb.find((x) => x.instance_id === part.ref) : undefined);
  const usbKey = $derived(usbDevice ? portKey(usbDevice.hub, usbDevice.port) : "");
  const usbChoice = $derived(usbDevice ? (usbPanels[usbKey] ?? null) : null);
  function choosePanel(panel: Panel) {
    setUsbPanel(usbKey, usbChoice === panel ? null : panel);
  }

  function computeInfo(): Info {
    const p = part;
    if (!p) return { title: "", subtitle: "", rows: [], tips: [], sensorList: [] };
    const tips: string[] = [];
    switch (p.kind) {
      case "board": {
        const b = hw.board;
        const usedDimms = hw.memory.sticks.length;
        return {
          title: b.product, subtitle: `${b.manufacturer} motherboard`,
          rows: [
            ["Form factor", b.form_factor.replace("_", "-").toUpperCase()],
            ["Revision", b.version || "—"],
            ["BIOS", `${b.bios_vendor} ${b.bios_version}`],
            ["BIOS date", b.bios_date || "—"],
            ["Memory slots", `${usedDimms} of ${hw.memory.total_slots || usedDimms} used`],
            ["Expansion slots", hw.slots.map((s) => `${s.name}${s.in_use ? " (in use)" : ""}`).join(", ") || "—"],
            ["System", `${hw.system.manufacturer} ${hw.system.model} · ${hw.system.chassis}`],
            ["Windows", hw.system.os],
          ],
          tips: hw.notes,
          sensorList: [...sensorsAt(sensors, { at: "board" }), ...sensorsAt(sensors, { at: "vrm" }), ...sensorsAt(sensors, { at: "chipset" }), ...sensors.filter((s) => s.site.at === "fan_header")],
        };
      }
      case "cpu": {
        const c = hw.cpus[0];
        if (!c) break;
        return {
          title: c.name, subtitle: `Processor in socket ${c.socket}`,
          rows: [
            ["Cores / threads", `${c.cores} / ${c.threads}`],
            ["Base clock", `${(c.max_mhz / 1000).toFixed(2)} GHz`],
            ["L2 cache", `${(c.l2_kb / 1024).toFixed(0)} MB`],
            ["L3 cache", `${(c.l3_kb / 1024).toFixed(0)} MB`],
            ["Socket", c.socket],
            ["Family", c.family || "—"],
            ["Voltage (BIOS)", c.bios_voltage != null ? `${c.bios_voltage.toFixed(2)} V · fixed value the BIOS reports, not live` : "—"],
            ["Virtualization", c.virtualization == null ? "—" : c.virtualization ? "Enabled" : "Disabled in BIOS (SVM/VT-x)"],
          ],
          connection: `Seated in the ${c.socket} socket in the middle of the board. Power comes from the 8-pin CPU_PWR connector through the VRM around the socket.`,
          tips: sensorsAt(sensors, { at: "cpu" }).some((x) => x.kind === "temperature") ? [] : ["Load and clock speed come straight from Windows. Windows only shares the CPU temperature and live voltage with programs that use a kernel driver."],
          sensorList: sensorsAt(sensors, { at: "cpu" }),
        };
      }
      case "dimm": {
        const name = String(p.ref);
        const s = hw.memory.sticks.find((m) => m.slot === name);
        const channel = name.match(/_?([A-H])(\d)$/i);
        const where = channel ? `Channel ${channel[1].toUpperCase()}, slot ${channel[2]}` : "Memory slot";
        if (!s) {
          return { title: name, subtitle: "Empty memory slot", rows: [["Position", where]], tips: [], sensorList: [] };
        }
        if (s.configured_mts && s.speed_mts && s.configured_mts < s.speed_mts) {
          tips.push(`This RAM is rated for ${s.speed_mts} MT/s but runs at ${s.configured_mts} MT/s. Enabling XMP/EXPO in the BIOS usually fixes that.`);
        }
        const used = hw.memory.sticks.map((m) => m.slot);
        if (used.length === 2 && hw.memory.total_slots === 4) {
          const good = used.every((u) => /2$/.test(u)) && new Set(used.map((u) => u.replace(/\d$/, ""))).size === 2;
          tips.push(good ? "Two sticks in the A2 + B2 slots: the recommended dual-channel setup. ✓" : "With two sticks, most boards want them in A2 and B2 for dual-channel speed.");
        }
        const dec = decodeRamPart(s.part_number);
        return {
          title: `${gb(s.capacity_bytes)} ${s.kind} · ${s.manufacturer}`, subtitle: `${name} — ${where}`,
          rows: [
            ["Part number", s.part_number],
            ["Capacity", gb(s.capacity_bytes)],
            ["Type", `${s.kind || "—"}${s.form ? " " + s.form : ""}`],
            ["Rated speed", `${s.speed_mts} MT/s`],
            ["Running at", `${s.configured_mts} MT/s`],
            ["Voltage (BIOS)", s.voltage != null ? `${s.voltage.toFixed(2)} V · as reported by the BIOS` : "—"],
            ["Ranks", s.ranks != null ? `${s.ranks === 1 ? "Single" : s.ranks === 2 ? "Dual" : s.ranks}-rank` : "—"],
            ["ECC", s.ecc ? "Yes" : "No"],
            ["Bank", s.bank],
            ...(dec ? (dec.map(([k, v]) => [`${k} (part no.)`, v]) as Row[]) : []),
          ],
          connection: `Plugged into ${name} (${where}), to the right of the CPU.`,
          tips,
          sensorList: sensorsAt(sensors, { at: "memory", id: name }),
        };
      }
      case "pcie": {
        const s = hw.slots.find((x) => x.name === p.ref);
        const card = layout.parts.find((x) => x.linkTo === p.id);
        return {
          title: String(p.ref), subtitle: `PCI Express x${s?.lanes ?? "?"} slot`,
          rows: [["Status", s?.in_use ? "In use" : s?.in_use === false ? "Empty" : "Unknown"], ["Lane width", s?.lanes ? `x${s.lanes}` : "—"], ["Card", card?.label ?? (s?.in_use ? "A card the system did not name" : "—")]],
          tips: [], sensorList: [],
        };
      }
      case "gpu": {
        const g = hw.gpus[Number(p.ref)];
        if (!g) break;
        const slot = p.linkTo?.replace("pcie:", "");
        return {
          title: g.name, subtitle: `${g.board_partner ? g.board_partner + " " : ""}graphics card`,
          rows: [
            ["Chip maker", g.vendor || "—"],
            ["Card maker", g.board_partner || "—"],
            ["Video memory", gb(g.vram_bytes)],
            ["Driver", `${g.driver_version} (${g.driver_date})`],
            ["VBIOS", g.vbios || "—"],
            ["PCIe link", g.pcie_link || "—"],
            ["Power limit", g.power_limit_w != null ? `${g.power_limit_w.toFixed(0)} W` : "—"],
            ["Display", g.resolution ? `${g.resolution} @ ${g.refresh_hz} Hz` : "—"],
          ],
          connection: p.slot
            ? `Installed in ${p.slot}. Proven from its PCIe connection path (${g.pcie_path}).`
            : slot ? `Installed in ${slot}.` : "Installed in a PCI Express slot.",
          tips: ["NVIDIA's driver does not share GPU voltage with other programs, so it is not shown."], sensorList: sensorsAt(sensors, { at: "gpu", id: Number(p.ref) }),
        };
      }
      case "m2":
      case "disk": {
        const d = hw.disks.find((x) => x.device_id === p.ref);
        if (!d) break;
        if (d.media === "HDD" && /EARX|EZRX|EADS/i.test(d.model)) tips.push("This is an older 5400-rpm WD Green drive. Fine for storage, but slow for games and Windows.");
        if (d.health !== "Healthy") tips.push(`Windows reports this drive as "${d.health}". Back up its data.`);
        if (d.wear_pct != null && d.wear_pct >= 80) tips.push(`This SSD has used ${d.wear_pct}% of its rated write life.`);
        if (!hw.elevated) tips.push("Temperature, wear and power-on hours appear when the Syscura service is running (it has the needed admin rights).");
        return {
          title: d.model, subtitle: `${driveSize(d.size_bytes)} ${d.media === "Unknown" ? "drive" : d.media} · ${d.bus}`,
          rows: [
            ["Type", d.media],
            ["Connection", d.bus],
            ["Capacity", driveSize(d.size_bytes)],
            ["Health", d.health],
            ["Firmware", d.firmware || "—"],
            ["Temperature", d.temperature_c != null ? `${d.temperature_c}°C` : "—"],
            ["Wear", d.wear_pct != null ? `${d.wear_pct}%` : "—"],
            ["Powered on", d.power_on_hours != null ? `${d.power_on_hours.toLocaleString()} hours` : "—"],
          ],
          connection: d.bus === "NVMe"
            ? p.slot ? `Plugged into ${p.slot}. Proven from its PCIe connection path (${d.pcie_path}).` : "Plugged straight into an M.2 socket on the board (which one is not reported for this board)."
            : d.bus === "USB" ? "Connected over USB." : "Cabled to one of the board's SATA ports (Windows does not report which port).",
          tips, sensorList: sensorsAt(sensors, { at: "disk", id: d.device_id }),
        };
      }
      case "usb": {
        const u = hw.usb.find((x) => x.instance_id === p.ref);
        if (!u) break;
        const panel = usbPanels[portKey(u.hub, u.port)] ?? u.panel;
        const place = panel === "back" ? "a rear port on the motherboard's I/O panel" : panel === "front" ? "a port on the case (front/top), wired to a USB header on the board" : panel === "internal" ? "an internal USB header" : "a USB port (the firmware does not say which panel; set it below)";
        return {
          title: u.name, subtitle: `${u.kind} · ${u.manufacturer.startsWith("(") ? "USB device" : u.manufacturer}`,
          rows: [
            ["Type", u.kind],
            ["Maker", u.manufacturer.startsWith("(") ? "—" : u.manufacturer],
            ["Plugged in at", panel === "back" ? "Rear panel" : panel === "front" ? "Case front/top" : panel === "internal" ? "Internal header" : "Unknown"],
            ["Port / hub", `Port ${u.port ?? "?"} on hub ${u.hub ?? "?"}`],
            ["Device ID", u.instance_id],
          ],
          connection: `Plugged into ${place}.`,
          tips: [], sensorList: [],
        };
      }
    }
    const generic: Record<string, string> = {
      io: "The ports you see at the back of the PC: USB, network, audio, display outputs of the CPU's graphics, Wi-Fi antennas.",
      vrm: "Voltage regulator module: turns the 12 V from the power supply into the ~1.2 V the CPU needs. It gets hot under load, hence the heatsinks.",
      chipset: "Connects the CPU to the extra USB ports, SATA ports and the lower PCIe slots.",
      sata: "Ports for SATA hard drives and SSDs (and optical drives).",
      atx24: "Main power from the power supply.",
      cpu_power: "Extra 12 V power for the CPU.",
      usb3_header: "Internal header that feeds the case's front USB 3 ports.",
      usb_header: "Internal headers that feed USB 2 ports on the case or internal devices (RGB controllers, Bluetooth).",
      front_panel: "Power button, reset button and case LEDs connect here.",
      audio_header: "Feeds the case's front headphone and microphone jacks.",
      cmos: "A coin battery that keeps the BIOS clock and settings when the PC is unplugged. If the clock resets, replace it.",
      superio: "Small chip that reads the board temperature, fan speeds and voltages.",
      fan_header: "Power and speed control for the CPU cooler fan.",
    };
    return { title: p.label, subtitle: "Part of the motherboard", rows: [], tips: [p.desc ?? generic[p.kind] ?? ""].filter(Boolean), sensorList: [] };
  }

  let busy = $state(false);
  let message = $state("");
  let linkInput = $state("");
  let showLink = $state(false);

  async function act(fn: () => Promise<unknown>, ok: string) {
    busy = true; message = "";
    try { await fn(); message = ok; onchanged(); }
    catch (e) { message = String(e); }
    finally { busy = false; }
  }
  function retry() { if (part?.imageKey) act(() => api.retryPartImage(part.imageKey!), "Searching again…"); }
  function useLink() { if (part?.imageKey && linkInput.trim()) act(() => api.setPartImageUrl(part.imageKey!, linkInput), "Picture saved."); }
  async function useFile(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file || !part?.imageKey) return;
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    act(() => api.setPartImageBytes(part.imageKey!, bytes, file.name), "Picture saved.");
  }

  const statusText: Record<string, string> = {
    queued: "Waiting to look up a picture…",
    searching: "Looking up a picture online…",
    waiting: "Search engines asked us to slow down. Syscura will try again later.",
    offline: "No internet right now. Syscura will try again.",
    not_found: "No picture found online. You can add your own.",
  };
</script>

{#if part}
  <aside class="detail card">
    <header>
      <div>
        <h2>{info.title}</h2>
        <p class="muted">{info.subtitle}</p>
      </div>
      <button class="x" onclick={onclose} aria-label="Close">✕</button>
    </header>

    {#if part.imageKey}
      <PartImage {image} kind={part.kind} size={190} cover />
      <div class="picbar">
        {#if image?.status === "ready" && image.source === "wikipedia"}
          <span class="generic">Generic example picture — not your exact model. Syscura keeps looking for the exact one.</span>
        {:else if image?.status === "ready"}
          <span class="faint">Picture: {image.source === "user" ? "added by you" : ""}{#if image.source !== "user"}<button class="linkbtn" onclick={() => api.open(image.page_url)} title={image.page_url}>{new URL(image.page_url).hostname}</button>{/if}</span>
        {:else}
          <span class="faint">{statusText[image?.status ?? "queued"]}</span>
        {/if}
        <span class="spacer"></span>
        <button disabled={busy} onclick={retry} title="Search for the picture again">Find again</button>
        <label class="filebtn">Use mine<input type="file" accept="image/png,image/jpeg,image/webp,image/gif,image/avif" onchange={useFile} /></label>
        <button onclick={() => (showLink = !showLink)} title="Use a picture from a link">Link</button>
      </div>
      {#if showLink}
        <div class="linkrow">
          <input type="text" placeholder="https://… picture link" bind:value={linkInput} />
          <button class="primary" disabled={busy} onclick={useLink}>Save</button>
        </div>
      {/if}
      {#if message}<p class="msg">{message}</p>{/if}
    {/if}

    {#if info.sensorList.length}
      <h3>Live sensors</h3>
      <div class="sensors">
        {#each info.sensorList as s}
          <div class="sensor t-{s.kind === 'temperature' ? tempTone(s.value) : 'none'}">
            <span class="v">{fmtSensor(s)}</span>
            <span class="l">{s.label}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if info.rows.length}
      <h3>Details</h3>
      <dl>
        {#each info.rows as [k, v]}<dt>{k}</dt><dd class:mono={k === "Device ID" || k === "Part number"}>{v}</dd>{/each}
      </dl>
    {/if}

    {#if info.connection}
      <h3>Where it's connected</h3>
      <p>{info.connection}</p>
    {/if}

    {#if usbDevice}
      <h3>Plugged in at</h3>
      <div class="panels">
        {#each [["back", "Rear panel"], ["front", "Case front/top"], ["internal", "Internal header"]] as [val, text]}
          <button class:on={(usbChoice ?? usbDevice.panel) === val} onclick={() => choosePanel(val as Panel)}>{text}</button>
        {/each}
      </div>
      <p class="faint small">{usbDevice.panel !== "unknown" && !usbChoice ? "Reported by the board's firmware." : usbChoice ? "Set by you; remembered for this USB port." : "The firmware does not say. Pick where this port is; Syscura remembers it for the port."}</p>
    {/if}

    {#if pins.length}
      <h3>Sensor location</h3>
      {#each pins as pin}<p><span class="pindot"></span><b>{pin.label}:</b> {pin.where}</p>{/each}
    {/if}

    {#each info.tips as tip}<p class="tip">{tip}</p>{/each}
  </aside>
{/if}

<style>
  .detail { width: 360px; flex: none; padding: 16px; overflow-y: auto; display: flex; flex-direction: column; gap: 8px; user-select: text; }
  header { display: flex; align-items: flex-start; gap: 8px; }
  header div { flex: 1; min-width: 0; }
  h2 { margin: 0; font-size: 17px; line-height: 1.25; }
  header p { margin: 2px 0 0; font-size: 12.5px; }
  .x { padding: 2px 8px; }
  h3 { margin: 10px 0 2px; font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--faint); }
  p { margin: 0; color: var(--muted); }
  .picbar { display: flex; align-items: center; gap: 6px; font-size: 12px; flex-wrap: wrap; }
  .picbar button, .filebtn { padding: 3px 9px; font-size: 12px; }
  .spacer { flex: 1; }
  .filebtn { border: 1px solid var(--line-2); border-radius: 8px; background: var(--panel-2); cursor: pointer; }
  .filebtn:hover { border-color: var(--accent); }
  .filebtn input { display: none; }
  .linkrow { display: flex; gap: 6px; }
  .picbar .linkbtn { background: none; border: none; padding: 0; color: var(--accent); font-size: 12px; }
  .picbar .linkbtn:hover { text-decoration: underline; }
  .msg { font-size: 12px; color: var(--accent); }
  dl { display: grid; grid-template-columns: 112px 1fr; gap: 5px 10px; margin: 0; font-size: 13px; }
  dt { color: var(--faint); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .sensors { display: grid; grid-template-columns: repeat(auto-fill, minmax(100px, 1fr)); gap: 6px; }
  .sensor { background: var(--bg-2); border: 1px solid var(--line); border-radius: 10px; padding: 8px; }
  .sensor .v { display: block; font-size: 18px; font-weight: 600; }
  .sensor .l { font-size: 11.5px; color: var(--faint); }
  .sensor.t-ok .v { color: var(--ok); }
  .sensor.t-warn .v { color: var(--warn); }
  .sensor.t-danger .v { color: var(--danger); }
  .tip { background: var(--accent-soft); border-left: 3px solid var(--accent); padding: 8px 10px; border-radius: 6px; color: var(--text); font-size: 13px; }
  .generic { color: var(--warn); font-size: 12px; }
  .panels { display: flex; gap: 6px; flex-wrap: wrap; }
  .panels button { font-size: 12.5px; padding: 4px 10px; }
  .panels button.on { background: var(--accent); color: #04211c; border-color: transparent; font-weight: 600; }
  .small { font-size: 12px; }
  .pindot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: var(--accent); margin-right: 6px; }
</style>
