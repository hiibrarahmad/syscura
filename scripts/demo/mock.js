// Fake Syscura backend for the demo video and README screenshots.
// Replaces Tauri's IPC in the browser; data is invented and realistic.
(() => {
  const now = Date.now();
  const min = 60_000, hour = 3_600_000, day = 86_400_000;

  const fix = (index, label, action, risk, extra = {}) => ({ index, label, action, risk, needs_admin: true, undoable: false, ...extra });
  const attempt = (id, label, automatic, outcome, msg, ago) => ({
    id, ts: now - ago, fix_index: 0, label, automatic, ok: true, verified: true, message: msg, undo: null, undone: false, outcome,
  });

  const findings = [
    {
      id: 1, rule_id: "svc.crash", group: "Print Spooler", title: "A Windows service crashed", category: "software",
      severity: "warning", harmful: "maybe",
      explanation: "The \"Print Spooler\" service stopped unexpectedly (3 times so far). Windows usually restarts it, but repeated crashes can break the feature it provides.",
      advice: "If it keeps happening, update or reinstall the program that owns the service. Repeated crashes of Windows' own services can mean damaged system files.",
      message: "The Print Spooler service terminated unexpectedly. It has done this 3 time(s).",
      first_ts: now - 3 * hour, last_ts: now - 4 * min, count: 3, status: "open",
      evidence: { param1: "Print Spooler", provider: "Service Control Manager", event_id: "7031" },
      fixes: [fix(0, "Make sure the service is running again", "service.ensure_running", "safe"), fix(1, "Check and repair Windows system files (SFC)", "sfc.scan", "caution")],
      attempts: [], search: "Windows 11 \"Print Spooler\" service terminated unexpectedly fix", verdict_by: "",
    },
    {
      id: 2, rule_id: "net.hosts_redirect", group: "www.paypal.com", title: "The hosts file sends a popular site somewhere else", category: "security",
      severity: "error", harmful: "yes",
      explanation: "Windows' hosts file sends www.paypal.com to 203.0.113.9 instead of the real site. This is how fake sign-in pages steal passwords.",
      advice: "Do not sign in to www.paypal.com until this is fixed. Remove the line from the hosts file, run a Defender full scan, and change your password from another device.",
      message: "", first_ts: now - 40 * min, last_ts: now - 40 * min, count: 1, status: "open",
      evidence: { Host: "www.paypal.com", Address: "203.0.113.9", Path: "C:\\Windows\\System32\\drivers\\etc\\hosts" },
      fixes: [fix(0, "Run a Defender full scan (takes a while)", "defender.full_scan", "caution"), fix(1, "Clear the DNS cache (after fixing the hosts file)", "dns.flush", "safe")],
      attempts: [], search: "hosts file redirect www.paypal.com phishing", verdict_by: "",
    },
    {
      id: 3, rule_id: "drv.vulnerable", group: "C:\\Windows\\System32\\drivers\\RTCore64.sys", title: "A driver with a known security hole is installed", category: "security",
      severity: "warning", harmful: "maybe",
      explanation: "RTCore64.sys (service \"RTCore64\", loads when a program asks for it) is on the LOLDrivers list of drivers with known security holes. Malware can load it to switch off antivirus. An old overclocking tool installed it.",
      advice: "Update or uninstall the program that installed it. Microsoft's vulnerable driver blocklist stops most of these drivers from loading.",
      message: "", first_ts: now - 2 * hour, last_ts: now - 2 * hour, count: 1, status: "open",
      evidence: { Service: "RTCore64", KnownAs: "RTCore64.sys", Path: "C:\\Windows\\System32\\drivers\\RTCore64.sys", Loads: "when a program asks for it" },
      fixes: [fix(0, "Turn on Microsoft's vulnerable driver blocklist", "driverblocklist.enable", "caution")],
      attempts: [], search: "RTCore64.sys vulnerable driver", verdict_by: "",
    },
    {
      id: 4, rule_id: "sys.bsod", group: "", title: "Windows crashed (blue screen)", category: "software", severity: "error", harmful: "maybe",
      explanation: "Windows stopped with SYSTEM_SERVICE_EXCEPTION (0x3B). This is usually caused by a faulty or outdated driver.",
      advice: "Update graphics and chipset drivers.", message: "", first_ts: now - 2 * day, last_ts: now - 2 * day, count: 1, status: "fixed",
      evidence: {}, fixes: [], attempts: [attempt(41, "Repair system files (SFC)", false, "fixed", "SFC found damaged system files and repaired them.", 2 * day - hour)],
      search: "", verdict_by: "",
    },
    {
      id: 5, rule_id: "time.sync", group: "", title: "The clock could not sync", category: "network", severity: "warning", harmful: "no",
      explanation: "Windows could not reach the time server.", advice: "", message: "", first_ts: now - day, last_ts: now - day, count: 2, status: "fixed",
      evidence: {}, fixes: [], attempts: [attempt(51, "Sync the clock", true, "fixed", "Synced the clock.", day - 5 * min)], search: "", verdict_by: "",
    },
    {
      id: 6, rule_id: "noise.dcom", group: "", title: "DCOM permission message (10016)", category: "noise", severity: "warning", harmful: "no",
      explanation: "Logged on every Windows PC. Microsoft says to ignore it.", advice: "", message: "", first_ts: now - 5 * day, last_ts: now - hour, count: 41, status: "open",
      evidence: {}, fixes: [], attempts: [], search: "", verdict_by: "",
    },
  ];

  const checks = [
    ["firewall", "Windows Firewall", 3, "bad", "Off for public networks.", "Unless another security program runs its own firewall, turn Windows Firewall on.", "firewall.enable", "Turn the firewall on"],
    ["defender", "Microsoft Defender real-time protection", 3, "good", "On: files are checked as they are opened.", "", "", ""],
    ["definitions", "Virus definitions up to date", 2, "good", "Updated today.", "", "", ""],
    ["tamper", "Tamper protection", 1, "good", "On: other programs cannot switch Defender off.", "", "", ""],
    ["uac", "User Account Control (admin prompts)", 3, "good", "On: programs must ask before changing Windows.", "", "", ""],
    ["updates", "Windows updates installed recently", 3, "good", "Last update installed 4 day(s) ago.", "", "", ""],
    ["smb1", "Old file sharing (SMBv1) switched off", 2, "bad", "SMBv1 is installed. It is the 1980s protocol WannaCry spread through.", "Remove it unless a very old NAS or printer needs it.", "smb1.disable", "Remove SMBv1"],
    ["rdp", "Remote Desktop", 2, "warn", "On: this PC accepts remote sign-ins.", "Turn it off if you do not use it.", "rdp.disable", "Turn Remote Desktop off"],
    ["secureboot", "Secure Boot", 2, "good", "On: only trusted code can start before Windows.", "", "", ""],
    ["blocklist", "Microsoft's vulnerable driver blocklist", 2, "good", "On: drivers with known security holes cannot load.", "", "", ""],
    ["bitlocker", "Drive encryption (BitLocker)", 2, "good", "The Windows drive is encrypted: a stolen PC's files cannot be read.", "", "", ""],
    ["lsa", "Password protection (LSA protection)", 1, "warn", "Off. Tools such as Mimikatz could read saved sign-in secrets.", "Windows Security → Device security → Core isolation → Local Security Authority protection: On.", "", ""],
    ["cfa", "Ransomware protection (Controlled folder access)", 0, "info", "Off. Optional: stops unknown programs from changing your documents.", "", "defender.enable_cfa", "Turn it on"],
  ].map(([id, title, weight, status, detail, advice, action, action_label]) => ({ id, title, weight, status, detail, advice, action, action_label }));

  const score = () => {
    let total = 0, got = 0;
    for (const c of checks) {
      if (!c.weight || !["good", "warn", "bad"].includes(c.status)) continue;
      total += c.weight;
      got += c.weight * (c.status === "good" ? 1 : c.status === "warn" ? 0.5 : 0);
    }
    return Math.round((got / total) * 100);
  };

  const sensors = [
    { label: "CPU load", kind: "load", value: 7, unit: "%", site: { at: "cpu" }, source: "windows" },
    { label: "CPU clock (average)", kind: "clock", value: 4120, unit: "MHz", site: { at: "cpu" }, source: "windows" },
    { label: "GPU temperature", kind: "temperature", value: 41, unit: "°C", site: { at: "gpu", id: 0 }, source: "nvml" },
    { label: "GPU load", kind: "load", value: 3, unit: "%", site: { at: "gpu", id: 0 }, source: "nvml" },
    { label: "Temperature", kind: "temperature", value: 38, unit: "°C", site: { at: "disk", id: "0" }, source: "windows" },
  ];

  const stick = (slot, bank) => ({ slot, bank, capacity_bytes: 16 * 2 ** 30, speed_mts: 3600, configured_mts: 3600, manufacturer: "G.Skill", part_number: "F4-3600C16-16GVKC", kind: "DDR4", voltage: 1.35, ranks: 1, ecc: false, form: "DIMM" });
  const hw = {
    collected_ms: now - 2 * min, elevated: true,
    system: { manufacturer: "ASUS", model: "TUF Gaming Desktop", family: "TUF Gaming", sku: "TUF-X570", chassis: "Desktop", is_laptop: false, virtual_machine: null, firmware: "UEFI", secure_boot: true },
    os: { name: "Windows 11 Pro", edition: "Professional", version: "25H2", build: "26200.6899", architecture: "64-bit", installed: "2025-03-14", last_boot_ms: now - 5 * hour - 12 * min },
    board: { manufacturer: "ASUSTeK COMPUTER INC.", product: "TUF GAMING X570-PRO WIFI II", version: "", bios_vendor: "American Megatrends Inc.", bios_version: "5021", bios_date: "2025-08-20" },
    cpus: [{ name: "AMD Ryzen 7 5800X3D 8-Core Processor", manufacturer: "AuthenticAMD", socket: "AM4", cores: 8, threads: 16, base_mhz: 3400, l2_kb: 4096, l3_kb: 98304, family: "AMD64 Family 25", bios_voltage: 1.1, virtualization: true }],
    memory: { usable_bytes: 32 * 2 ** 30, total_slots: 4, max_bytes: 128 * 2 ** 30, sticks: [stick("DIMM_A2", "BANK 1"), stick("DIMM_B2", "BANK 3")] },
    slots: [{ name: "PCIEX16_1", in_use: true, lanes: 16 }, { name: "PCIEX16_2", in_use: false, lanes: 4 }],
    gpus: [{ name: "NVIDIA GeForce RTX 4070", vendor: "NVIDIA", board_partner: "ASUS", vram_bytes: 12 * 2 ** 30, driver_version: "32.0.15.8157", driver_date: "2025-09-30", resolution: "2560x1440", refresh_hz: 165, pnp_id: "PCI\\VEN_10DE", pcie_path: "", vbios: "95.04.41.00.5B", pcie_link: "PCIe 4.0 x16", power_limit_w: 200 }],
    displays: [{ manufacturer: "Dell", model: "S2721DGF", year: 2023, active: true }],
    disks: [
      { model: "Samsung SSD 980 PRO 1TB", media: "SSD", bus: "NVMe", size_bytes: 1_000_204_886_016, health: "Healthy", firmware: "5B2QGXA7", temperature_c: 38, wear_pct: 4, power_on_hours: 6120, device_id: "0", pcie_path: "" },
      { model: "WDC WD20EZBX-00AYRA0", media: "HDD", bus: "SATA", size_bytes: 2_000_398_934_016, health: "Healthy", firmware: "01.01A01", temperature_c: 33, wear_pct: null, power_on_hours: 11240, device_id: "1", pcie_path: "" },
    ],
    volumes: [
      { letter: "C:", label: "Windows", file_system: "NTFS", size_bytes: 999_000_000_000, free_bytes: 412_000_000_000, removable: false },
      { letter: "D:", label: "Data", file_system: "NTFS", size_bytes: 2_000_000_000_000, free_bytes: 1_180_000_000_000, removable: false },
      { letter: "E:", label: "Backup USB", file_system: "exFAT", size_bytes: 256_000_000_000, free_bytes: 190_000_000_000, removable: true },
    ],
    network: [{ name: "Intel(R) Wi-Fi 6 AX200 160MHz", connection: "Wi-Fi", kind: "Wi-Fi", manufacturer: "Intel", connected: true, speed_mbps: 1201 }, { name: "Realtek Gaming 2.5GbE", connection: "Ethernet", kind: "Ethernet", manufacturer: "Realtek", connected: false, speed_mbps: null }],
    audio: [{ name: "Realtek High Definition Audio", manufacturer: "Realtek" }],
    battery: [],
    usb: [{ name: "Logitech G502 HERO", kind: "Mouse", manufacturer: "Logitech", instance_id: "", location: "", port: 3, hub: 1, panel: "back" }],
    sensors,
    notes: [],
  };

  const disk = (name, wear0, temp) => Array.from({ length: 30 }, (_, i) => {
    const d = new Date(now - (29 - i) * day).toISOString().slice(0, 10);
    return { day: d, disk: name, health: "Healthy", temperature_c: temp + Math.round(Math.sin(i / 3) * 3 + (i % 5 === 0 ? 2 : 0)), wear_pct: wear0 == null ? null : wear0 + (i > 18 ? 1 : 0), power_on_hours: 6000 + i * 4 };
  });
  const diskHistory = [...disk("Samsung SSD 980 PRO 1TB (disk 0)", 3, 38), ...disk("WDC WD20EZBX-00AYRA0 (disk 1)", null, 33)];

  const kinds = [
    ["System", "Service Control Manager", 7031, "error", "The Print Spooler service terminated unexpectedly. It has done this 3 time(s)."],
    ["System", "Microsoft-Windows-DistributedCOM", 10016, "warning", "The application-specific permission settings do not grant Local Activation permission for the COM Server application."],
    ["Application", "Application Error", 1000, "error", "Faulting application name: Discord.exe, version 1.0.9165, faulting module name: ntdll.dll."],
    ["System", "Microsoft-Windows-Time-Service", 129, "warning", "NtpClient was unable to set a domain peer to use as a time source."],
    ["System", "Microsoft-Windows-WindowsUpdateClient", 20, "error", "Installation Failure: Windows failed to install the following update with error 0x800f0922."],
    ["Microsoft-Windows-Windows Defender/Operational", "Microsoft-Windows-Windows Defender", 1116, "warning", "Microsoft Defender Antivirus has detected malware or other potentially unwanted software: PUA:Win32/Presenoker."],
    ["System", "disk", 153, "warning", "The IO operation at logical block address 0x2f0a1 for Disk 1 was retried."],
    ["Application", "Microsoft-Windows-Security-SPP", 16384, "info", "Successfully scheduled Software Protection service for re-start."],
  ];
  const events = Array.from({ length: 40 }, (_, i) => {
    const [channel, provider, event_id, level, message] = kinds[(i * 5) % kinds.length];
    return { id: 900 - i, ts: now - i * 23 * min, source: "eventlog", channel, provider, event_id, level, record_id: 5000 - i, data: { _message: message } };
  });

  const state = { canary: "on", findings, checks };
  const later = (ms, f) => setTimeout(f, ms);

  const handlers = {
    agent_status: () => ({
      version: "1.1.0", pid: 4120, service: true, uptime_secs: 5 * 3600 + 11 * 60, working_set_bytes: 13.2 * 2 ** 20, private_bytes: 6.1 * 2 ** 20,
      sensors: ["eventlog/System", "eventlog/Application", "eventlog/Security", "eventlog/Microsoft-Windows-Windows Defender/Operational", "eventlog/Microsoft-Windows-PowerShell/Operational"],
      events_total: 18342, last_24h: { critical: 0, error: 14, warning: 61, other: 0 },
      defender: { antivirus: true, realtime: true, tamper_protected: true, signature_age_days: 0, signatures_updated: new Date(now - 3 * hour).toISOString().slice(0, 19), last_quick_scan: new Date(now - 5 * hour).toISOString().slice(0, 19), last_full_scan: new Date(now - 6 * day).toISOString().slice(0, 19), checked_ms: now },
    }),
    events: () => events,
    hardware: () => ({ hw, from_agent: true }),
    live_sensors: () => sensors,
    findings: ({ includeClosed }) => state.findings.filter((f) => includeClosed || f.status !== "ignored"),
    run_fix: ({ finding, fix: index }) => {
      const f = state.findings.find((x) => x.id === finding);
      const fx = f.fixes[index];
      f.status = "fixing";
      later(window.__demoFixMs ?? 1600, () => {
        f.status = "fixed";
        f.attempts = [attempt(100 + f.id, fx.label, false, "fixed", f.id === 1 ? "Started \"Print Spooler\". Checked: the service is running." : "Done.", 0)];
      });
      return `Started: ${fx.label}`;
    },
    apply_action: ({ action, label }) => {
      const c = state.checks.find((x) => x.action === action);
      if (c) later(window.__demoFixMs ?? 1600, () => { c.status = "good"; c.detail = action === "firewall.enable" ? "On for every network type." : action === "smb1.disable" ? "Not installed." : "Off: nobody can sign in to this PC over the network."; });
      return `Started: ${label}`;
    },
    security: () => ({ checked_ms: now - 3 * min, score: score(), checks: state.checks, refreshing: false }),
    agent_options: () => ({ ransomware_canary: state.canary }),
    set_agent_option: ({ value }) => { state.canary = value; return "Saved."; },
    disk_history: () => diskHistory,
    summary: () => ({ days: 7, events: { critical: 0, error: 62, warning: 318, other: 0 }, new_problems: 6, fixed_automatically: 3, fixed_by_you: 2, open_problems: 3, security_problems: 2, highlights: [] }),
    outdated_apps: () => [
      { name: "Mozilla Firefox (x64 en-US)", id: "Mozilla.Firefox", version: "130.0", available: "131.0.2" },
      { name: "Adobe Acrobat Reader (64-bit)", id: "Adobe.Acrobat.Reader.64-bit", version: "24.002.21005", available: "24.003.20112" },
      { name: "7-Zip 23.01 (x64)", id: "7zip.7zip", version: "23.01", available: "24.08" },
    ],
    app_prefs: () => ({ auto_update_check: true, weekly_summary: true, backup_schedule: { every_days: 7, destination: "E:\\", folders: ["desktop", "documents", "pictures"], last_ms: now - 2 * day } }),
    update_check: () => ({ current: "1.1.0", latest: "1.1.0", available: false, notes: "", page: "", published: "" }),
    ai_status: () => ({ configured: true, model: "gemini-2.5-flash", auto_fix: true }),
    initial_view: () => null,
    paths_exist: ({ paths }) => paths.map(() => true),
    actions: () => [],
    backup_info: () => ({
      folders: [{ id: "desktop", name: "Desktop", path: "C:\\Users\\Alex\\Desktop" }, { id: "documents", name: "Documents", path: "C:\\Users\\Alex\\Documents" }, { id: "pictures", name: "Pictures", path: "C:\\Users\\Alex\\Pictures" }],
      targets: [{ root: "E:\\", label: "Backup USB", free_bytes: 190e9, size_bytes: 256e9, system_drive: false, removable: true }, { root: "D:\\", label: "Data", free_bytes: 1180e9, size_bytes: 2000e9, system_drive: false, removable: false }],
      status: { running: false, destination: "", current: "", done: [], finished_ms: null, error: null },
    }),
    processes: () => [],
    web_prompt: () => "",
    ai_ask: () => new Promise((r) => setTimeout(() => r({
      summary: "Fixed. The Print Spooler service is running again and Windows confirms it started cleanly.",
      likely_cause: "A printer driver crashed the spooler.", harmful: "no", fixed: "yes",
      steps: ["If it crashes again, update the printer driver from the maker's site."], actions: [],
      sources: [{ title: "Microsoft Learn: Print Spooler", url: "https://learn.microsoft.com" }], model: "gemini-2.5-flash",
    }), 500)),
  };

  let cb = 1;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback: () => cb++,
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args = {}) => {
      const h = handlers[cmd];
      if (!h) return null; // plugins (opener, notification) and anything else
      return JSON.parse(JSON.stringify(await h(args)));
    },
  };
  window.__demo = state;
})();
