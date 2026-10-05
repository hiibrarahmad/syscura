// Mirrors the Rust types in syscura-core (serde JSON shapes).

export type Level = "critical" | "error" | "warning" | "info" | "verbose";

export interface LevelCounts { critical: number; error: number; warning: number; other: number }

export interface StatusInfo {
  version: string;
  pid: number;
  uptime_secs: number;
  working_set_bytes: number;
  private_bytes: number;
  sensors: string[];
  events_total: number;
  last_24h: LevelCounts;
}

export interface StoredEvent {
  id: number;
  ts: number;
  source: string;
  channel: string;
  provider: string;
  event_id: number;
  level: Level;
  record_id: number;
  data: Record<string, string>;
}

export type FormFactor = "atx" | "micro_atx" | "mini_itx" | "eatx" | "laptop" | "unknown";
export type Panel = "front" | "back" | "top" | "bottom" | "left" | "right" | "internal" | "unknown";
export type SensorKind = "temperature" | "fan" | "voltage" | "load" | "power" | "clock";

export type SensorSite =
  | { at: "cpu" }
  | { at: "vrm" }
  | { at: "chipset" }
  | { at: "board" }
  | { at: "gpu"; id: number }
  | { at: "disk"; id: string }
  | { at: "memory"; id: string }
  | { at: "fan_header"; id: string };

export interface Sensor {
  label: string;
  kind: SensorKind;
  value: number | null;
  unit: string;
  site: SensorSite;
  source: string;
}

export interface CpuInfo { name: string; manufacturer: string; socket: string; cores: number; threads: number; max_mhz: number; l2_kb: number; l3_kb: number; family: string; bios_voltage: number | null; virtualization: boolean | null }
export interface MemoryStick { slot: string; bank: string; capacity_bytes: number; speed_mts: number; configured_mts: number; manufacturer: string; part_number: string; kind: string; voltage: number | null; ranks: number | null; ecc: boolean; form: string }
export interface ExpansionSlot { name: string; in_use: boolean | null; lanes: number | null }
export interface GpuInfo { name: string; vendor: string; board_partner: string; vram_bytes: number; driver_version: string; driver_date: string; resolution: string; refresh_hz: number; pnp_id: string; pcie_path: string; vbios: string; pcie_link: string; power_limit_w: number | null }
export interface DiskInfo { model: string; media: string; bus: string; size_bytes: number; health: string; firmware: string; temperature_c: number | null; wear_pct: number | null; power_on_hours: number | null; device_id: string; pcie_path: string }
export interface UsbDevice { name: string; kind: string; manufacturer: string; instance_id: string; location: string; port: number | null; hub: number | null; panel: Panel }

export interface HardwareInfo {
  collected_ms: number;
  elevated: boolean;
  system: { manufacturer: string; model: string; chassis: string; os: string };
  board: { manufacturer: string; product: string; version: string; bios_vendor: string; bios_version: string; bios_date: string; form_factor: FormFactor };
  cpus: CpuInfo[];
  memory: { total_slots: number; sticks: MemoryStick[] };
  slots: ExpansionSlot[];
  gpus: GpuInfo[];
  disks: DiskInfo[];
  usb: UsbDevice[];
  sensors: Sensor[];
  notes: string[];
}

export interface HardwareView { hw: HardwareInfo; from_agent: boolean }

export interface PartImage {
  key: string;
  kind: string;
  query: string;
  status: "ready" | "queued" | "searching" | "waiting" | "offline" | "not_found";
  version: number;
  page_url: string;
  source: string;
}

export type Risk = "safe" | "caution" | "risky";
export type Harm = "no" | "maybe" | "yes";
export type FindingStatus = "open" | "fixing" | "fixed" | "fix_failed" | "ignored";

export interface FixOption { index: number; label: string; action: string; risk: Risk; needs_admin: boolean; undoable: boolean }
export interface FixAttempt { id: number; ts: number; fix_index: number; label: string; automatic: boolean; ok: boolean; verified: boolean | null; message: string; undo: string | null; undone: boolean }
export interface Finding {
  id: number; rule_id: string; group: string; title: string; category: string; severity: Level; harmful: Harm;
  explanation: string; advice: string; first_ts: number; last_ts: number; count: number; status: FindingStatus;
  evidence: Record<string, string>; fixes: FixOption[]; attempts: FixAttempt[];
}
