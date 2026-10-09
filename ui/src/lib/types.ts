// Mirrors the Rust types in syscura-core and syscura-online (serde JSON shapes).

export type Level = "critical" | "error" | "warning" | "info" | "verbose";

export interface LevelCounts { critical: number; error: number; warning: number; other: number }

export interface DefenderInfo {
  antivirus: boolean; realtime: boolean; tamper_protected: boolean; signature_age_days: number;
  signatures_updated: string; last_quick_scan: string; last_full_scan: string; checked_ms: number;
}
export interface ProcessInfo {
  pid: number; parent: number; parent_name: string; name: string; path: string; started_ms: number;
  memory_bytes: number; cpu_pct: number; threads: number;
  signature: "valid" | "unsigned" | "invalid" | "unknown" | ""; signer: string;
  location: "system" | "program_files" | "user" | "other"; warning: string;
}

export interface StatusInfo {
  version: string;
  pid: number;
  service: boolean;
  defender: DefenderInfo | null;
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

export interface Sensor { label: string; kind: SensorKind; value: number | null; unit: string; site: SensorSite; source: string }

export interface SystemInfo { manufacturer: string; model: string; family: string; sku: string; chassis: string; is_laptop: boolean; virtual_machine: string | null; firmware: string; secure_boot: boolean | null }
export interface OsInfo { name: string; edition: string; version: string; build: string; architecture: string; installed: string; last_boot_ms: number | null }
export interface BoardInfo { manufacturer: string; product: string; version: string; bios_vendor: string; bios_version: string; bios_date: string }
export interface CpuInfo { name: string; manufacturer: string; socket: string; cores: number; threads: number; base_mhz: number; l2_kb: number; l3_kb: number; family: string; bios_voltage: number | null; virtualization: boolean | null }
export interface MemoryStick { slot: string; bank: string; capacity_bytes: number; speed_mts: number; configured_mts: number; manufacturer: string; part_number: string; kind: string; voltage: number | null; ranks: number | null; ecc: boolean; form: string }
export interface ExpansionSlot { name: string; in_use: boolean | null; lanes: number | null }
export interface GpuInfo { name: string; vendor: string; board_partner: string; vram_bytes: number; driver_version: string; driver_date: string; resolution: string; refresh_hz: number; pnp_id: string; pcie_path: string; vbios: string; pcie_link: string; power_limit_w: number | null }
export interface DisplayInfo { manufacturer: string; model: string; year: number | null; active: boolean }
export interface DiskInfo { model: string; media: string; bus: string; size_bytes: number; health: string; firmware: string; temperature_c: number | null; wear_pct: number | null; power_on_hours: number | null; device_id: string; pcie_path: string }
export interface VolumeInfo { letter: string; label: string; file_system: string; size_bytes: number; free_bytes: number; removable: boolean }
export interface NetworkAdapter { name: string; connection: string; kind: string; manufacturer: string; connected: boolean; speed_mbps: number | null }
export interface AudioDevice { name: string; manufacturer: string }
export interface BatteryInfo { name: string; manufacturer: string; chemistry: string; charge_pct: number | null; status: string; design_mwh: number | null; full_mwh: number | null; wear_pct: number | null; cycle_count: number | null }
export interface UsbDevice { name: string; kind: string; manufacturer: string; instance_id: string; location: string; port: number | null; hub: number | null; panel: Panel }

export interface HardwareInfo {
  collected_ms: number;
  elevated: boolean;
  system: SystemInfo;
  os: OsInfo;
  board: BoardInfo;
  cpus: CpuInfo[];
  memory: { usable_bytes: number; total_slots: number; max_bytes: number; sticks: MemoryStick[] };
  slots: ExpansionSlot[];
  gpus: GpuInfo[];
  displays: DisplayInfo[];
  disks: DiskInfo[];
  volumes: VolumeInfo[];
  network: NetworkAdapter[];
  audio: AudioDevice[];
  battery: BatteryInfo[];
  usb: UsbDevice[];
  sensors: Sensor[];
  notes: string[];
}

export interface HardwareView { hw: HardwareInfo; from_agent: boolean }

export type Risk = "safe" | "caution" | "risky";
export type Harm = "no" | "maybe" | "yes";
export type FindingStatus = "open" | "fixing" | "fixed" | "fix_failed" | "ignored";

export interface ActionInfo { id: string; label: string; description: string; risk: Risk; params: string[]; undoable: boolean; needs_admin: boolean }
export interface FixOption { index: number; label: string; action: string; risk: Risk; needs_admin: boolean; undoable: boolean }
export interface FixAttempt { id: number; ts: number; fix_index: number; label: string; automatic: boolean; ok: boolean; verified: boolean | null; message: string; undo: string | null; undone: boolean; outcome: string }
export interface Finding {
  id: number; rule_id: string; group: string; title: string; category: string; severity: Level; harmful: Harm;
  explanation: string; advice: string; message: string; first_ts: number; last_ts: number; count: number; status: FindingStatus;
  evidence: Record<string, string>; fixes: FixOption[]; attempts: FixAttempt[]; search: string;
  /** Who set `harmful` when it is not the rule's default: "you" or "ai". */
  verdict_by: string;
}

export interface BackupFolder { id: string; name: string; path: string }
export interface BackupTarget { root: string; label: string; free_bytes: number; size_bytes: number; system_drive: boolean; removable: boolean }
export interface BackupStatus { running: boolean; destination: string; current: string; done: { name: string; ok: boolean; detail: string }[]; finished_ms: number | null; error: string | null }
export interface BackupInfo { folders: BackupFolder[]; targets: BackupTarget[]; status: BackupStatus }

export interface Question { title: string; explanation: string; details: Record<string, string>; system: string }
export interface ProposedAction { action: string; params: Record<string, string>; why: string }
export interface Analysis {
  summary: string; likely_cause: string; harmful: string; fixed: string; steps: string[];
  actions: ProposedAction[]; sources: { title: string; url: string }[]; model: string;
}
export interface AiStatus { configured: boolean; model: string | null; auto_fix: boolean }

export interface SecurityCheck {
  id: string; title: string; status: "good" | "bad" | "warn" | "info" | "unknown"; detail: string; advice: string;
  action: string; action_label: string; weight: number;
}
export interface SecurityReport { checked_ms: number; score: number; checks: SecurityCheck[]; refreshing: boolean }
export interface DiskPoint { day: string; disk: string; health: string; temperature_c: number | null; wear_pct: number | null; power_on_hours: number | null }
export interface Summary {
  days: number; events: LevelCounts; new_problems: number; fixed_automatically: number; fixed_by_you: number;
  open_problems: number; security_problems: number; highlights: string[];
}
export interface AppUpdate { name: string; id: string; version: string; available: string }
export interface BackupSchedule { every_days: number; destination: string; folders: string[]; last_ms: number }
export interface Prefs { auto_update_check: boolean; weekly_summary: boolean; backup_schedule: BackupSchedule | null }
