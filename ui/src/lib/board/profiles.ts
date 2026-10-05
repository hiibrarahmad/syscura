// Verified board profiles: the maker's own top-down photo plus the exact
// position of every labelled part on it. Positions are percentages of the
// photo and were measured against the white silkscreen labels printed on
// the board, so they are exact for that photo.
//
// Boards without a profile get the generic layout and movable pins.

export interface Region {
  /** Matches the ids used by the board map ("dimm:DIMM_A2", "pcie:PCIEX16_1"). */
  id: string;
  kind: string;
  /** Name printed on the board. */
  label: string;
  /** [x, y, width, height] in % of the photo. */
  rect: [number, number, number, number];
  /** Shown by default; minor headers appear with "All labels". */
  major?: boolean;
  desc: string;
}

export interface SensorSpot {
  /** Which reading to show (matches SensorSite.at). */
  at: "cpu" | "board" | "chipset" | "vrm" | "disk";
  /** Region the sensor sits in. */
  region: string;
  x: number;
  y: number;
  where: string;
}

export interface BoardProfile {
  match: string[];
  photo: { url: string; page: string };
  /** PCIe root port (from the device's location path) -> slot name. */
  pcieSlots: Record<string, string>;
  /** NVMe controller root port -> M.2 socket name. */
  m2Slots: Record<string, string>;
  regions: Region[];
  sensors: SensorSpot[];
  notes: string[];
}

const TUF_X570_PRO_WIFI_II: BoardProfile = {
  match: ["TUF GAMING X570-PRO WIFI II"],
  photo: {
    url: "https://dlcdnwebimgs.asus.com/gain/3e75d067-659d-43dc-8388-8b1159535cfa/",
    page: "https://www.asus.com/motherboards-components/motherboards/tuf-gaming/tuf-gaming-x570-pro-wifi-ii/",
  },
  // AM4: root port 03.1 feeds the top x16 slot, 01.1 feeds M.2_1; everything
  // behind 01.2 goes through the X570 chipset.
  pcieSlots: { "PCI(0301)": "PCIEX16_1", "PCI(0102)": "PCIEX16_2" },
  m2Slots: { "PCI(0101)": "M.2_1", "PCI(0102)": "M.2_2" },
  regions: [
    { id: "io", kind: "io", label: "Rear I/O", rect: [12, 9.5, 7.5, 41], major: true, desc: "The ports at the back of the PC: USB, 2.5G LAN, Wi-Fi antennas, HDMI/DisplayPort (for CPUs with graphics), audio, and the BIOS FlashBack button." },
    { id: "bios_flbk", kind: "button", label: "BIOS_FLBK", rect: [12, 7.5, 1.3, 2], desc: "BIOS FlashBack button: updates the BIOS from a USB stick, even without a CPU or memory installed." },
    { id: "cpu_power", kind: "cpu_power", label: "EATX12V_1 (8-pin)", rect: [29.2, 5.5, 5.2, 2.8], major: true, desc: "Main 8-pin CPU power from the power supply. Must be connected." },
    { id: "cpu_power_2", kind: "cpu_power", label: "EATX12V_2 (4-pin)", rect: [25.7, 5.5, 2.6, 2.8], desc: "Extra 4-pin CPU power. Optional; helps with heavy overclocking." },
    { id: "vrm", kind: "vrm", label: "VRM heatsinks", rect: [37.5, 5.5, 23.5, 9], major: true, desc: "Voltage regulators that turn 12 V into the ~1.1–1.4 V the CPU runs on. The heatsinks keep them cool." },
    { id: "vrm_left", kind: "vrm", label: "VRM heatsink", rect: [17, 8.5, 21.5, 36], desc: "The large TUF GAMING heatsink covers the rear I/O shroud and the left VRM bank." },
    { id: "cpu", kind: "cpu", label: "AM4 socket", rect: [47.5, 25, 16, 15.5], major: true, desc: "The CPU socket (AMD AM4)." },
    { id: "cpu_fan", kind: "fan_header", label: "CPU_FAN", rect: [64.7, 5.5, 3.1, 1.4], major: true, desc: "4-pin fan header for the CPU cooler. Speed is controlled from the BIOS (Q-Fan)." },
    { id: "cpu_opt", kind: "fan_header", label: "CPU_OPT", rect: [68.2, 5.5, 2.9, 1.4], desc: "Second CPU fan header (for a second cooler fan)." },
    { id: "rgb1", kind: "rgb_header", label: "RGB_HEADER1", rect: [72.2, 5.5, 3.1, 1.4], desc: "12 V RGB LED header (4-pin, 12V-G-R-B) for Aura Sync lighting." },
    { id: "dimm:DIMM_A1", kind: "dimm", label: "DIMM_A1", rect: [69.5, 10, 1.4, 44], major: true, desc: "Memory slot A1 (channel A). Use with 4 sticks." },
    { id: "dimm:DIMM_A2", kind: "dimm", label: "DIMM_A2", rect: [72.4, 10, 1.5, 44], major: true, desc: "Memory slot A2 (channel A). Recommended slot (marked * on the board)." },
    { id: "dimm:DIMM_B1", kind: "dimm", label: "DIMM_B1", rect: [75.3, 10, 1.45, 44], major: true, desc: "Memory slot B1 (channel B). Use with 4 sticks." },
    { id: "dimm:DIMM_B2", kind: "dimm", label: "DIMM_B2", rect: [78.1, 10, 1.5, 44], major: true, desc: "Memory slot B2 (channel B). Recommended slot (marked * on the board)." },
    { id: "atx24", kind: "atx24", label: "EATXPWR (24-pin)", rect: [82.8, 25, 2.6, 16.7], major: true, desc: "Main 24-pin power from the power supply." },
    { id: "usb3_header", kind: "usb3_header", label: "U32G2_C5", rect: [82.5, 41.4, 2.1, 3.7], major: true, desc: "Front USB-C header (USB 3.2 Gen 2, 10 Gbps) for the case's front Type-C port." },
    { id: "m2:M.2_1", kind: "m2", label: "M.2_1 (SOCKET3)", rect: [36, 50.3, 24.6, 5.6], major: true, desc: "M.2 slot wired straight to the CPU (PCIe 4.0 x4, also takes SATA M.2). Fastest slot. Takes 2242/2260/2280/22110 drives." },
    { id: "pcie:PCIEX16_1", kind: "pcie", label: "PCIEX16_1", rect: [26.5, 57.7, 26.1, 3.1], major: true, desc: "Main graphics slot: PCIe 4.0 x16, wired to the CPU." },
    { id: "chipset", kind: "chipset", label: "X570 chipset", rect: [46.5, 60.8, 29.5, 14.7], major: true, desc: "AMD X570 chipset under the TUF heatsink (fanless on this board). It runs the second M.2, the lower PCIe slots, SATA and many USB ports." },
    { id: "pcie:PCIEX1_1", kind: "pcie", label: "PCIEX1_1", rect: [26.6, 69.6, 7.2, 2.7], major: true, desc: "PCIe 4.0 x1 slot (via chipset) for small cards (Wi-Fi, sound, capture)." },
    { id: "cmos", kind: "cmos", label: "CMOS battery", rect: [35, 67.9, 5.8, 5.4], major: true, desc: "CR2032 coin battery: keeps the BIOS clock and settings. If the clock resets after unplugging, replace it." },
    { id: "superio", kind: "superio", label: "Nuvoton Super I/O", rect: [18, 70, 4.5, 4.6], major: true, desc: "Monitoring chip: reads the motherboard temperature, all fan speeds and the voltages (Vcore, 12 V, 5 V, 3.3 V)." },
    { id: "pcie:PCIEX16_2", kind: "pcie", label: "PCIEX16_2", rect: [26.5, 75.6, 26.8, 3.2], major: true, desc: "Second long slot: physically x16, electrically PCIe 4.0 x4 via the chipset." },
    { id: "m2:M.2_2", kind: "m2", label: "M.2_2 (SOCKET3)", rect: [35.8, 79.6, 39.6, 5.4], major: true, desc: "Second M.2 slot under the heatsink, via the chipset (PCIe 4.0 x4, also SATA M.2)." },
    { id: "audio", kind: "audio", label: "Audio (codec under cover)", rect: [15.3, 82.2, 6, 5.6], desc: "The onboard audio chip sits under this TUF cover, shielded from electrical noise." },
    { id: "pcie:PCIEX1_2", kind: "pcie", label: "PCIEX1_2", rect: [26.6, 87.6, 7.2, 2.6], major: true, desc: "PCIe 4.0 x1 slot (via chipset)." },
    { id: "audio_header", kind: "audio_header", label: "AAFP", rect: [25.4, 92.7, 5, 1.5], desc: "Front-panel audio header: the case's headphone and microphone jacks." },
    { id: "com", kind: "header", label: "COM", rect: [31.3, 92.5, 4.5, 2], desc: "Serial (COM) port header." },
    { id: "clrtc", kind: "header", label: "COM_DEBUG / CLRTC", rect: [38.5, 92.5, 4.5, 1.5], desc: "CLRTC: short these pins to reset the BIOS settings (clear CMOS). COM_DEBUG is a service header." },
    { id: "cha_fan3", kind: "fan_header", label: "CHA_FAN3", rect: [43.8, 92.5, 2.4, 1.5], desc: "4-pin case fan header." },
    { id: "usb78", kind: "usb_header", label: "USB78", rect: [47.4, 92.5, 4.5, 1.6], desc: "USB 2.0 header (two ports) for the case or internal devices." },
    { id: "usb910", kind: "usb_header", label: "USB910", rect: [52.5, 92.5, 4.1, 1.6], desc: "USB 2.0 header (two ports) for the case or internal devices such as RGB controllers and Bluetooth." },
    { id: "usb_header", kind: "usb_header", label: "U32G1_12", rect: [57.5, 92.5, 6.3, 2], major: true, desc: "USB 3.2 Gen 1 header (two 5 Gbps ports) for the case's front USB-A ports." },
    { id: "cha_fan2", kind: "fan_header", label: "CHA_FAN2", rect: [64.7, 92.5, 2.3, 1.5], desc: "4-pin case fan header." },
    { id: "sata:78", kind: "sata", label: "SATA6G_7 / 8", rect: [67.9, 87, 9.3, 2.4], desc: "SATA ports 7 and 8 (6 Gb/s)." },
    { id: "sata:56", kind: "sata", label: "SATA6G_5 / 6", rect: [67.9, 91.5, 9.3, 2.7], desc: "SATA ports 5 and 6 (6 Gb/s)." },
    { id: "rgb2", kind: "rgb_header", label: "RGB_HEADER2", rect: [78.6, 88.1, 3, 1.1], desc: "Second 12 V RGB LED header." },
    { id: "front_panel", kind: "front_panel", label: "PANEL", rect: [78, 92.4, 6.3, 2.1], major: true, desc: "Front panel header: power button, reset button, power LED, drive LED and speaker." },
    { id: "sata", kind: "sata", label: "SATA6G_12 / 34", rect: [81.6, 55.7, 4.6, 10], major: true, desc: "SATA ports 1–4 (6 Gb/s), angled for neat cabling." },
  ],
  sensors: [
    { at: "cpu", region: "cpu", x: 55.5, y: 32.7, where: "Inside the CPU itself (Tctl/Tdie, read from the processor)." },
    { at: "board", region: "superio", x: 20.2, y: 72.3, where: "Motherboard temperature, measured by the Nuvoton Super I/O chip." },
    { at: "chipset", region: "chipset", x: 61, y: 68, where: "Inside the X570 chipset under the heatsink." },
    { at: "disk", region: "m2:M.2_1", x: 52, y: 53.1, where: "On the SSD's controller chip; the drive reports it itself." },
  ],
  notes: [
    "Part positions are measured from ASUS's official photo of this board.",
  ],
};

export const PROFILES: BoardProfile[] = [TUF_X570_PRO_WIFI_II];

export function findProfile(product: string): BoardProfile | undefined {
  const p = product.trim().toUpperCase();
  return PROFILES.find((b) => b.match.some((m) => m.toUpperCase() === p));
}

/** Root-port part of a location path: "PCIROOT(0)#PCI(0301)#..." -> "PCI(0301)". */
export function rootPort(path: string): string {
  return path.split("#")[1] ?? "";
}
