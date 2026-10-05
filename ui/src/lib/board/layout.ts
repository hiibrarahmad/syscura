// Builds a top-down map of the motherboard from the detected hardware.
//
// Units are millimetres in the standard board orientation used by product
// photos: rear I/O on the left edge, CPU upper-middle, memory to its right,
// expansion slots across the lower half. Positions follow the ATX family
// geometry; exact positions differ per model, which is why the photo view
// lets the user drag pins.

import type { HardwareInfo, Panel, Sensor, SensorSite, UsbDevice } from "../types";
import { type BoardProfile, rootPort } from "./profiles";

export type PartKind =
  | "board" | "io" | "cpu" | "vrm" | "dimm" | "pcie" | "gpu" | "m2" | "chipset"
  | "sata" | "atx24" | "cpu_power" | "usb_header" | "usb3_header" | "front_panel"
  | "audio_header" | "cmos" | "superio" | "fan_header" | "disk" | "usb";

export interface Part {
  id: string;
  kind: PartKind;
  label: string;
  /** Short name printed on the map. */
  short?: string;
  x: number; y: number; w: number; h: number;
  /** Something is installed / connected here. */
  filled?: boolean;
  /** Picture key (matches the backend's part keys). */
  imageKey?: string;
  /** Index or id into the inventory for the detail panel. */
  ref?: string | number;
  /** Where this connects (id of a board part), for cable lines. */
  linkTo?: string;
  /** Plain-language explanation (from the board profile). */
  desc?: string;
  /** Slot the device is proven to sit in (e.g. "M.2_1"), when known. */
  slot?: string;
  /** Only exists on the photo view (labelled headers from the profile). */
  photoOnly?: boolean;
}

export interface SensorPin {
  id: string;
  site: SensorSite;
  label: string;
  x: number; y: number;
  /** Where the sensor physically is, in plain words. */
  where: string;
  /** The part this pin belongs to (for selection). */
  partId: string;
}

export interface BoardLayout {
  width: number;
  height: number;
  /** Extra space around the board for drives and USB devices. */
  view: { x: number; y: number; w: number; h: number };
  parts: Part[];
  pins: SensorPin[];
  formFactor: string;
}

interface Geometry {
  W: number; H: number;
  cpu: { cx: number; cy: number };
  dimmX: number; dimmPitch: number; dimmTop: number; dimmLen: number;
  slotTop: number; slotPitch: number; maxSlots: number;
  chipset: { x: number; y: number };
  rightEdge: number;
}

const GEOMETRY: Record<string, Geometry> = {
  atx: { W: 305, H: 244, cpu: { cx: 118, cy: 72 }, dimmX: 176, dimmPitch: 9.5, dimmTop: 8, dimmLen: 133, slotTop: 140, slotPitch: 22, maxSlots: 7, chipset: { x: 200, y: 172 }, rightEdge: 305 },
  eatx: { W: 305, H: 272, cpu: { cx: 118, cy: 76 }, dimmX: 176, dimmPitch: 9.5, dimmTop: 8, dimmLen: 133, slotTop: 150, slotPitch: 22, maxSlots: 7, chipset: { x: 200, y: 190 }, rightEdge: 305 },
  micro_atx: { W: 244, H: 244, cpu: { cx: 104, cy: 72 }, dimmX: 158, dimmPitch: 9, dimmTop: 8, dimmLen: 133, slotTop: 150, slotPitch: 22, maxSlots: 4, chipset: { x: 168, y: 176 }, rightEdge: 244 },
  mini_itx: { W: 170, H: 170, cpu: { cx: 78, cy: 62 }, dimmX: 128, dimmPitch: 9, dimmTop: 6, dimmLen: 133, slotTop: 146, slotPitch: 20, maxSlots: 1, chipset: { x: 120, y: 112 }, rightEdge: 170 },
};

function slotLength(lanes: number | null): number {
  if (lanes == null || lanes >= 8) return 89;
  if (lanes >= 4) return 39;
  return 25;
}

export interface LayoutOptions {
  profile?: BoardProfile;
  /** Where the user says a USB device is plugged in (overrides firmware). */
  usbPanel?: (u: UsbDevice) => Panel;
}

export function buildLayout(hw: HardwareInfo, opts: LayoutOptions = {}): BoardLayout {
  const { profile } = opts;
  const panelOf = (u: UsbDevice) => opts.usbPanel?.(u) ?? u.panel;
  const ff = hw.board.form_factor;
  const key = ff === "micro_atx" || ff === "mini_itx" || ff === "eatx" ? ff : "atx";
  const g = GEOMETRY[key];
  const parts: Part[] = [];
  const pins: SensorPin[] = [];
  const push = (p: Part) => { parts.push(p); return p; };

  push({ id: "board", kind: "board", label: hw.board.product || "Motherboard", x: 0, y: 0, w: g.W, h: g.H, imageKey: "board" });

  // Rear I/O along the left edge.
  push({ id: "io", kind: "io", label: "Rear I/O panel", short: "REAR I/O", x: 0, y: 10, w: 28, h: Math.min(150, g.H * 0.62) });
  push({ id: "cpu_power", kind: "cpu_power", label: "CPU power (8-pin EPS)", short: "CPU_PWR", x: 32, y: 2, w: 24, h: 9 });

  // VRM heatsinks wrap the socket on the top and left.
  const { cx, cy } = g.cpu;
  push({ id: "vrm", kind: "vrm", label: "VRM (CPU power delivery)", short: "VRM", x: 34, y: 14, w: cx + 30 - 34, h: 16 });
  push({ id: "vrm_left", kind: "vrm", label: "VRM (CPU power delivery)", x: 34, y: 30, w: 18, h: cy + 34 - 30 });

  const cpu = hw.cpus[0];
  push({
    id: "cpu", kind: "cpu", label: cpu?.name ?? "CPU socket", short: cpu?.socket || "CPU",
    x: cx - 22, y: cy - 22, w: 44, h: 44, filled: !!cpu, imageKey: cpu ? "cpu:0" : undefined, ref: 0,
  });
  push({ id: "cpu_fan", kind: "fan_header", label: "CPU fan header", short: "CPU_FAN", x: cx + 32, y: 3, w: 12, h: 5 });

  // Memory slots, named as the board reports them (DIMM_A1 ...).
  const sticks = hw.memory.sticks;
  const slotNames = dimmSlotNames(hw.memory.total_slots, sticks.map((s) => s.slot));
  slotNames.forEach((name, i) => {
    const stick = sticks.find((s) => s.slot === name);
    push({
      id: `dimm:${name}`, kind: "dimm", label: name, short: name.replace(/^DIMM_?/i, ""),
      x: g.dimmX + i * g.dimmPitch, y: g.dimmTop, w: 6, h: g.dimmLen,
      filled: !!stick, imageKey: stick ? `ram:${stick.part_number}` : undefined, ref: name,
    });
  });

  // Power and front-panel headers along the right and bottom edges.
  const R = g.rightEdge;
  push({ id: "atx24", kind: "atx24", label: "24-pin ATX power", short: "ATX 24", x: R - 13, y: 60, w: 11, h: 52 });
  push({ id: "usb3_header", kind: "usb3_header", label: "USB 3 header (case front ports)", short: "USB3_F", x: R - 12, y: 118, w: 10, h: 20 });
  push({ id: "chipset", kind: "chipset", label: "Chipset", short: "CHIPSET", x: g.chipset.x, y: g.chipset.y, w: 40, h: 40 });
  push({ id: "sata", kind: "sata", label: "SATA ports", short: "SATA", x: R - 16, y: g.chipset.y - 6, w: 14, h: 44 });
  push({ id: "usb_header", kind: "usb_header", label: "USB 2 headers (case front ports)", short: "USB_F", x: g.W * 0.48, y: g.H - 9, w: 30, h: 7 });
  push({ id: "front_panel", kind: "front_panel", label: "Front panel header (power button, LEDs)", short: "F_PANEL", x: R - 44, y: g.H - 9, w: 26, h: 7 });
  push({ id: "audio_header", kind: "audio_header", label: "Front audio header", short: "AAFP", x: 6, y: g.H - 9, w: 18, h: 7 });
  push({ id: "superio", kind: "superio", label: "Super I/O chip (board temperature and fans)", short: "SIO", x: g.chipset.x + 52, y: g.chipset.y + 30, w: 13, h: 13 });
  push({ id: "cmos", kind: "cmos", label: "CMOS battery (keeps BIOS settings)", short: "BAT", x: g.chipset.x - 34, y: g.chipset.y + 18, w: 18, h: 18 });

  // Expansion slots and what sits in them.
  const pcie = hw.slots.filter((s) => /pci/i.test(s.name)).slice(0, g.maxSlots);
  let gpuIndex = 0;
  const realGpus = hw.gpus.map((gp, i) => ({ gp, i })).filter(({ gp }) => gp.vendor && !/basic display|remote/i.test(gp.name));
  // Slot each graphics card is proven to sit in (board profile + PCIe path).
  const provenSlot = new Map<number, string>();
  if (profile) for (const { gp, i } of realGpus) {
    const slot = profile.pcieSlots[rootPort(gp.pcie_path)];
    if (slot) provenSlot.set(i, slot);
  }
  const placed = new Set<number>();
  pcie.forEach((s, i) => {
    const y = g.slotTop + i * g.slotPitch;
    const len = slotLength(s.lanes);
    push({ id: `pcie:${s.name}`, kind: "pcie", label: s.name, short: s.name, x: 34, y, w: len, h: 6, filled: !!s.in_use, ref: s.name });
    // A graphics card sits in its proven slot, else in the first long slot in use.
    let hit = realGpus.find(({ i: idx }) => provenSlot.get(idx) === s.name);
    if (!hit && s.in_use && (s.lanes ?? 16) >= 8) hit = realGpus.find(({ i: idx }) => !placed.has(idx) && !provenSlot.has(idx));
    if (hit && !placed.has(hit.i)) {
      const { gp, i: idx } = hit;
      placed.add(idx);
      gpuIndex++;
      push({
        id: `gpu:${idx}`, kind: "gpu", label: gp.name, short: gp.name.replace(/^(NVIDIA|AMD|Intel\(R\))\s+/i, ""),
        x: 30, y: y - 9, w: 128, h: 24, filled: true, imageKey: `gpu:${idx}`, ref: idx, linkTo: `pcie:${s.name}`,
        slot: provenSlot.get(idx),
      });
    }
  });
  // Graphics cards we could not tie to a slot still get a card.
  for (const { gp, i: idx } of realGpus) {
    if (placed.has(idx)) continue;
    const y = g.slotTop + Math.max(pcie.length, 1) * g.slotPitch;
    push({ id: `gpu:${idx}`, kind: "gpu", label: gp.name, short: gp.name, x: 30, y: Math.min(y, g.H - 30), w: 128, h: 24, filled: true, imageKey: `gpu:${idx}`, ref: idx });
  }

  // M.2 drives sit between the slots; the exact M.2 socket is not reported.
  const nvme = hw.disks.filter((d) => d.bus === "NVMe");
  nvme.forEach((d, i) => {
    const slot = profile?.m2Slots[rootPort(d.pcie_path)];
    push({
      id: `disk:${d.device_id}`, kind: "m2", label: d.model, short: `${slot ?? "M.2"} · ${shortModel(d.model)}`, slot,
      x: 64, y: g.slotTop - 16 + i * g.slotPitch * 2, w: 80, h: 10, filled: true, imageKey: `disk:${d.device_id}`, ref: d.device_id,
    });
  });

  // SATA / USB drives live off the board, cabled to the SATA ports.
  const offBoard = hw.disks.filter((d) => d.bus !== "NVMe");
  offBoard.forEach((d, i) => {
    push({
      id: `disk:${d.device_id}`, kind: "disk", label: d.model, short: shortModel(d.model),
      x: g.W + 22, y: 120 + i * 38, w: 70, h: 30, filled: true, imageKey: `disk:${d.device_id}`, ref: d.device_id,
      linkTo: d.bus === "USB" ? "io" : "sata",
    });
  });

  // USB devices: rear ones on the left of the I/O panel, case ones on the
  // right (wired through the front headers), unknown ones underneath.
  let rear = 0, front = 0, other = 0;
  hw.usb.forEach((u) => {
    const base = { id: `usb:${u.instance_id}`, kind: "usb" as const, label: u.name, short: u.name, w: 70, h: 20, filled: true, imageKey: `usb:${u.instance_id}`, ref: u.instance_id };
    const panel = panelOf(u);
    if (panel === "back") {
      push({ ...base, x: -98, y: 14 + rear++ * 26, linkTo: "io" });
    } else if (panel === "front") {
      push({ ...base, x: g.W + 22, y: 6 + front++ * 26, linkTo: "usb3_header" });
    } else {
      push({ ...base, x: -98, y: g.H - 24 - other++ * 26, linkTo: panel === "internal" ? "usb_header" : undefined });
    }
  });

  // Sensor pins: where each sensor physically sits.
  const at = (id: string) => parts.find((p) => p.id === id);
  const center = (p: Part) => ({ x: p.x + p.w / 2, y: p.y + p.h / 2 });
  const pin = (id: string, site: SensorSite, label: string, partId: string, where: string, dx = 0, dy = 0) => {
    const p = at(partId);
    if (!p) return;
    const c = center(p);
    pins.push({ id, site, label, x: c.x + dx, y: c.y + dy, where, partId });
  };
  pin("s:cpu", { at: "cpu" }, "CPU", "cpu", "Inside the processor itself (on-die thermal diode, read as Tctl/Tdie on AMD or Package on Intel).");
  pin("s:vrm", { at: "vrm" }, "VRM", "vrm", "A thermistor under the VRM heatsink, next to the power stages that feed the CPU.");
  pin("s:chipset", { at: "chipset" }, "Chipset", "chipset", "Inside the chipset under its heatsink.");
  pin("s:board", { at: "board" }, "Board", "superio", "The Super I/O chip near the bottom of the board; it also reads the fan headers and voltages.");
  realGpus.forEach(({ i }) => pin(`s:gpu:${i}`, { at: "gpu", id: i }, "GPU", `gpu:${i}`, "On the graphics chip (GPU core sensor), read by the graphics driver.", 30));
  hw.disks.forEach((d) => pin(`s:disk:${d.device_id}`, { at: "disk", id: d.device_id }, "Drive", `disk:${d.device_id}`,
    d.bus === "NVMe" ? "On the SSD's controller chip; the drive reports it itself (SMART)." : "Inside the drive; the drive reports it itself (SMART)."));
  sticks.forEach((s) => {
    if (s.kind === "DDR5") pin(`s:mem:${s.slot}`, { at: "memory", id: s.slot }, "RAM", `dimm:${s.slot}`, "On the memory module (DDR5 modules have their own sensor).");
  });

  // Profile regions: add the labelled headers the generic map does not
  // draw, and attach the board's own descriptions to the parts it does.
  if (profile) {
    for (const r of profile.regions) {
      const existing = parts.find((p) => p.id === r.id);
      if (existing) existing.desc = r.desc;
      else parts.push({ id: r.id, kind: r.kind as PartKind, label: r.label, x: 0, y: 0, w: 0, h: 0, desc: r.desc, photoOnly: true });
    }
  }

  const minX = hw.usb.some((u) => panelOf(u) !== "front") ? -104 : -12;
  const maxX = g.W + (offBoard.length || hw.usb.some((u) => panelOf(u) === "front") ? 100 : 12);
  return { width: g.W, height: g.H, view: { x: minX, y: -8, w: maxX - minX, h: g.H + 16 }, parts, pins, formFactor: key };
}

/** For a photo region, the installed device to open when it is clicked. */
export function installedIn(layout: BoardLayout, regionId: string): string | undefined {
  if (regionId.startsWith("pcie:")) return layout.parts.find((p) => p.kind === "gpu" && p.linkTo === regionId)?.id;
  if (regionId.startsWith("m2:")) {
    const slot = regionId.slice(3);
    return layout.parts.find((p) => p.kind === "m2" && p.slot === slot)?.id;
  }
  return undefined;
}

function shortModel(m: string): string {
  return m.replace(/^WDC\s+/i, "").replace(/-[0-9A-Z]{5,}$/i, "").slice(0, 22);
}

/** Slot names in physical order. Uses the names the board reports and
 * fills in the empty ones with the usual A1/A2/B1/B2 naming. */
export function dimmSlotNames(total: number, used: string[]): string[] {
  const n = Math.max(total, used.length);
  if (n === 0) return [];
  const prefix = used[0]?.match(/^([A-Za-z]+_?)/)?.[1] ?? "DIMM_";
  const usual = n <= 2 ? ["A1", "B1"].map((s) => prefix + s) : n === 4 ? ["A1", "A2", "B1", "B2"].map((s) => prefix + s)
    : Array.from({ length: n }, (_, i) => `${prefix}${String.fromCharCode(65 + Math.floor(i / 2))}${(i % 2) + 1}`);
  if (used.every((u) => usual.includes(u))) return usual;
  // Unusual naming: keep the reported names and add placeholders.
  const empty = Array.from({ length: Math.max(0, n - used.length) }, (_, i) => `Empty slot ${i + 1}`);
  return [...[...used].sort(), ...empty];
}

/** Sensors reported for a given site. */
export function sensorsAt(sensors: Sensor[], site: SensorSite): Sensor[] {
  return sensors.filter((s) => s.site.at === site.at && ("id" in site ? "id" in s.site && s.site.id === site.id : true));
}
