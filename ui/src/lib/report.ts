// Turns the inventory into report sections. The Hardware page renders them
// and "Copy report" turns the very same data into text, so both always agree.
// Accuracy rule: a value Windows did not report is null and shows as
// "Not reported" — nothing is guessed or filled in.

import { driveSize, fmtSensor, gb } from "./api";
import { decodeRamPart } from "./ramparts";
import type { HardwareInfo, Sensor } from "./types";

export type Value = string | null;
export interface Group { heading?: string; rows: [string, Value][]; badge?: string }
export interface Section { id: string; title: string; icon: string; groups: Group[]; note?: string }

const v = (s: string | null | undefined): Value => (s && s.trim() ? s.trim() : null);
const num = (n: number | null | undefined, f: (n: number) => string): Value => (n == null || n === 0 ? null : f(n));
const yesNo = (b: boolean | null | undefined, yes = "Yes", no = "No"): Value => (b == null ? null : b ? yes : no);

export function buildReport(hw: HardwareInfo, live: Sensor[]): Section[] {
  const out: Section[] = [];
  const sensorRows = (pred: (s: Sensor) => boolean): [string, Value][] =>
    live.filter(pred).map((s) => [s.label, fmtSensor(s)]);

  const os = hw.os;
  out.push({
    id: "windows", title: "Windows", icon: "os",
    groups: [{ rows: [
      ["Edition", v(os.name)],
      ["Version", v(os.version)],
      ["Build", v(os.build)],
      ["Architecture", v(os.architecture)],
      ["Installed", v(os.installed)],
      ["Last start", os.last_boot_ms ? new Date(os.last_boot_ms).toLocaleString() : null],
      ["Firmware", v(hw.system.firmware)],
      ["Secure Boot", yesNo(hw.system.secure_boot, "On", "Off")],
    ] }],
  });

  const sys = hw.system;
  out.push({
    id: "system", title: sys.is_laptop ? "Laptop" : "Computer", icon: "system",
    groups: [{ rows: [
      ["Manufacturer", v(sys.manufacturer)],
      ["Model", v(sys.model)],
      ["Family", v(sys.family)],
      ["SKU", v(sys.sku)],
      ["Type", v(sys.chassis)],
      ...(sys.virtual_machine ? ([["Virtual machine", sys.virtual_machine]] as [string, Value][]) : []),
    ] }],
  });

  const b = hw.board;
  out.push({
    id: "board", title: "Motherboard & BIOS", icon: "board",
    groups: [{ rows: [
      ["Manufacturer", v(b.manufacturer)],
      ["Model", v(b.product)],
      ["Revision", v(b.version)],
      ["BIOS / UEFI maker", v(b.bios_vendor)],
      ["BIOS version", v(b.bios_version)],
      ["BIOS date", v(b.bios_date)],
    ] }],
  });

  out.push({
    id: "cpu", title: hw.cpus.length > 1 ? "Processors" : "Processor", icon: "cpu",
    groups: [
      ...hw.cpus.map((c): Group => ({
        heading: hw.cpus.length > 1 ? c.name.trim() : undefined,
        rows: [
          ["Name", v(c.name)],
          ["Manufacturer", v(c.manufacturer.replace("AuthenticAMD", "AMD").replace("GenuineIntel", "Intel"))],
          ["Cores / threads", c.cores ? `${c.cores} / ${c.threads}` : null],
          ["Base clock", num(c.base_mhz, (m) => `${(m / 1000).toFixed(2)} GHz`)],
          ["L2 cache", num(c.l2_kb, (k) => (k >= 1024 ? `${k / 1024} MB` : `${k} KB`))],
          ["L3 cache", num(c.l3_kb, (k) => (k >= 1024 ? `${k / 1024} MB` : `${k} KB`))],
          ["Socket", v(c.socket)],
          ["Family / model", v(c.family)],
          ["Virtualization", yesNo(c.virtualization, "Enabled", "Disabled in BIOS")],
          ["Voltage (BIOS value)", num(c.bios_voltage, (x) => `${x.toFixed(2)} V`)],
        ],
      })),
      { heading: "Live", rows: sensorRows((s) => s.site.at === "cpu") },
    ],
  });

  const m = hw.memory;
  const installed = m.sticks.reduce((a, s) => a + s.capacity_bytes, 0);
  const xmpOff = m.sticks.some((s) => s.speed_mts && s.configured_mts && s.configured_mts < s.speed_mts);
  out.push({
    id: "memory", title: "Memory", icon: "ram",
    note: xmpOff ? "Some memory runs below its rated speed. Enabling XMP/EXPO in the BIOS usually fixes that." : undefined,
    groups: [
      { rows: [
        ["Installed", installed ? gb(installed) : null],
        ["Usable by Windows", num(m.usable_bytes, (x) => gb(x, 1))],
        ["Slots used", m.total_slots ? `${m.sticks.length} of ${m.total_slots}` : m.sticks.length ? `${m.sticks.length}` : null],
        ["Most the board supports", num(m.max_bytes, (x) => gb(x))],
      ] },
      ...m.sticks.map((s): Group => ({
        heading: `${s.slot || "Slot"} — ${gb(s.capacity_bytes)} ${s.kind}`,
        rows: [
          ["Manufacturer", v(s.manufacturer)],
          ["Part number", v(s.part_number)],
          ...((decodeRamPart(s.part_number) ?? []).map(([k, val]) => [`${k} (from part no.)`, val]) as [string, Value][]),
          ["Type", v([s.kind, s.form].filter(Boolean).join(" "))],
          ["Rated speed", num(s.speed_mts, (x) => `${x} MT/s`)],
          ["Running at", num(s.configured_mts, (x) => `${x} MT/s`)],
          ["Voltage (BIOS value)", num(s.voltage, (x) => `${x.toFixed(2)} V`)],
          ["Ranks", s.ranks == null ? null : s.ranks === 1 ? "Single rank" : s.ranks === 2 ? "Dual rank" : `${s.ranks} ranks`],
          ["ECC", s.ecc ? "Yes" : "No"],
          ["Bank", v(s.bank)],
        ],
      })),
    ],
  });

  out.push({
    id: "gpu", title: "Graphics", icon: "gpu",
    groups: hw.gpus.map((g, i): Group => ({
      heading: g.name,
      rows: [
        ["Chip maker", v(g.vendor)],
        ["Card / PC maker", v(g.board_partner)],
        ["Video memory", num(g.vram_bytes, (x) => gb(x))],
        ["Driver", v(g.driver_version ? `${g.driver_version}${g.driver_date ? ` (${g.driver_date})` : ""}` : "")],
        ["VBIOS", v(g.vbios)],
        ["PCIe link", v(g.pcie_link)],
        ["Power limit", num(g.power_limit_w, (x) => `${x.toFixed(0)} W`)],
        ["Display mode", v(g.resolution ? `${g.resolution} @ ${g.refresh_hz} Hz` : "")],
        ...sensorRows((s) => s.site.at === "gpu" && s.site.id === i),
      ],
    })),
  });

  if (hw.displays.length) {
    out.push({
      id: "displays", title: "Monitors", icon: "display",
      groups: hw.displays.map((d): Group => ({
        heading: d.model || "Monitor",
        rows: [["Manufacturer", v(d.manufacturer)], ["Model", v(d.model)], ["Made in", d.year ? String(d.year) : null]],
      })),
    });
  }

  out.push({
    id: "storage", title: "Storage", icon: "disk",
    groups: [
      ...hw.disks.map((d): Group => ({
        heading: d.model,
        badge: d.health !== "Healthy" ? d.health : undefined,
        rows: [
          ["Type", v(d.media === "Unknown" ? "" : d.media)],
          ["Connection", v(d.bus)],
          ["Capacity", num(d.size_bytes, driveSize)],
          ["Health (Windows)", v(d.health)],
          ["Firmware", v(d.firmware)],
          ["Temperature", d.temperature_c != null ? `${d.temperature_c}°C` : null],
          ["Wear", d.wear_pct != null ? `${d.wear_pct}%` : null],
          ["Powered on", d.power_on_hours != null ? `${d.power_on_hours.toLocaleString()} hours` : null],
        ],
      })),
      ...(hw.volumes.length
        ? [{
            heading: "Drives (letters)",
            rows: hw.volumes.map((x): [string, Value] => [
              `${x.letter}${x.label ? ` ${x.label}` : ""}${x.removable ? " (removable)" : ""}`,
              `${driveSize(x.free_bytes)} free of ${driveSize(x.size_bytes)} · ${x.file_system}`,
            ]),
          }]
        : []),
    ],
  });

  if (hw.slots.length) {
    out.push({
      id: "slots", title: "Expansion slots", icon: "slot",
      groups: [{ rows: hw.slots.map((s): [string, Value] => [
        s.name,
        `${s.in_use == null ? "Use not reported" : s.in_use ? "In use" : "Empty"}${s.lanes ? ` · x${s.lanes}` : ""}`,
      ]) }],
    });
  }

  if (hw.network.length) {
    out.push({
      id: "network", title: "Network", icon: "network",
      groups: hw.network.map((n): Group => ({
        heading: n.name,
        rows: [
          ["Type", v(n.kind)],
          ["Connection name", v(n.connection)],
          ["Status", n.connected ? `Connected${n.speed_mbps ? ` at ${n.speed_mbps >= 1000 ? `${n.speed_mbps / 1000} Gbps` : `${n.speed_mbps} Mbps`}` : ""}` : "Not connected"],
          ["Maker", v(n.manufacturer)],
        ],
      })),
    });
  }

  if (hw.battery.length) {
    out.push({
      id: "battery", title: "Battery", icon: "battery",
      groups: hw.battery.map((x): Group => ({
        heading: x.name || "Battery",
        rows: [
          ["Maker", v(x.manufacturer)],
          ["Chemistry", v(x.chemistry)],
          ["Charge", x.charge_pct != null ? `${x.charge_pct}%` : null],
          ["Status", v(x.status)],
          ["Original capacity", num(x.design_mwh, (w) => `${(w / 1000).toFixed(1)} Wh`)],
          ["Capacity now", num(x.full_mwh, (w) => `${(w / 1000).toFixed(1)} Wh`)],
          ["Wear", x.wear_pct != null ? `${x.wear_pct}% capacity lost` : null],
          ["Charge cycles", num(x.cycle_count, String)],
        ],
      })),
    });
  }

  if (hw.audio.length) {
    out.push({
      id: "audio", title: "Audio", icon: "audio",
      groups: [{ rows: hw.audio.map((a): [string, Value] => [a.name, v(a.manufacturer)]) }],
    });
  }

  if (hw.usb.length) {
    out.push({
      id: "usb", title: "USB devices", icon: "usb",
      groups: [{ rows: hw.usb.map((u): [string, Value] => [
        u.name,
        [u.kind, u.manufacturer && !u.manufacturer.startsWith("(") ? u.manufacturer : "", u.port ? `port ${u.port}, hub ${u.hub}` : "", u.panel === "back" ? "rear panel" : u.panel === "front" ? "front panel" : ""]
          .filter(Boolean).join(" · "),
      ]) }],
    });
  }
  return out;
}

/** Plain-text version for forums, support chats and bug reports. */
export function reportText(sections: Section[], hw: HardwareInfo): string {
  const lines = [`Syscura hardware report — ${new Date(hw.collected_ms).toLocaleString()}`, ""];
  for (const s of sections) {
    lines.push(`== ${s.title} ==`);
    for (const g of s.groups) {
      if (!g.rows.length) continue;
      if (g.heading) lines.push(`-- ${g.heading}${g.badge ? ` [${g.badge}]` : ""}`);
      for (const [k, val] of g.rows) lines.push(`${k}: ${val ?? "Not reported"}`);
    }
    if (s.note) lines.push(`Note: ${s.note}`);
    lines.push("");
  }
  for (const n of hw.notes) lines.push(`Note: ${n}`);
  return lines.join("\n");
}
