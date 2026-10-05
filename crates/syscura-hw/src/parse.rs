//! Pure helpers that turn raw firmware/WMI values into readable facts.
//! No OS calls here, so everything is unit-tested on any platform.

use syscura_core::hw::{FormFactor, Panel};

/// SMBIOS chassis type -> short description.
pub fn chassis_name(t: u64) -> &'static str {
    match t {
        3..=7 | 15 | 16 | 24 => "desktop",
        8..=12 | 14 | 31 | 32 => "laptop",
        13 => "all-in-one",
        17 | 23 | 28 => "server",
        30 => "tablet",
        35 | 36 => "mini pc",
        _ => "unknown",
    }
}

pub fn is_laptop_chassis(t: u64) -> bool {
    chassis_name(t) == "laptop" || t == 30
}

/// SMBIOS memory type code -> "DDR4" etc.
pub fn memory_kind(code: u64) -> &'static str {
    match code {
        20 => "DDR",
        21 => "DDR2",
        24 => "DDR3",
        26 => "DDR4",
        27 => "LPDDR",
        28 => "LPDDR2",
        29 => "LPDDR3",
        30 => "LPDDR4",
        34 => "DDR5",
        35 => "LPDDR5",
        _ => "",
    }
}

/// Lane width from a slot name such as "PCIEX16_1" or "PCIe x4 Slot".
pub fn slot_lanes(name: &str) -> Option<u32> {
    let upper = name.to_ascii_uppercase();
    let after = &upper[upper.find("PCIE")? + 4..];
    let after = after.trim_start_matches([' ', '_', '-']);
    let digits: String = after.strip_prefix('X')?.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Best guess at the board's form factor from its product name, slot
/// counts and chassis.
pub fn guess_form_factor(product: &str, pcie_slots: usize, dimm_slots: u32, laptop: bool) -> FormFactor {
    if laptop {
        return FormFactor::Laptop;
    }
    let p = product.to_ascii_uppercase();
    if p.contains("ITX") || p.ends_with("-I") || p.contains("-I ") {
        return FormFactor::MiniItx;
    }
    if p.contains("E-ATX") || p.contains("EATX") {
        return FormFactor::Eatx;
    }
    if p.contains("MATX") || p.contains("M-ATX") || has_chipset_m_suffix(&p) {
        return FormFactor::MicroAtx;
    }
    match (pcie_slots, dimm_slots) {
        (0, 0) => FormFactor::Unknown,
        (0..=1, 0..=2) => FormFactor::MiniItx,
        _ => FormFactor::Atx,
    }
}

/// Chipset names like "B550M" or "H610M" mark micro-ATX boards.
fn has_chipset_m_suffix(p: &str) -> bool {
    let b = p.as_bytes();
    b.windows(5).any(|w| {
        matches!(w[0], b'A' | b'B' | b'H' | b'X' | b'Z' | b'Q')
            && w[1..4].iter().all(u8::is_ascii_digit)
            && w[4] == b'M'
    })
}

/// Decodes the panel from an ACPI `_PLD` buffer (byte 8: bit 0 = user
/// visible, bits 3-5 = panel). Firmware that never filled the buffer leaves
/// it zeroed, which would read as "top"; that is reported as unknown.
pub fn pld_panel(pld: &[u8]) -> Panel {
    let Some(&b) = pld.get(8) else { return Panel::Unknown };
    let visible = b & 1 == 1;
    let panel = match (b >> 3) & 7 {
        0 => Panel::Top,
        1 => Panel::Bottom,
        2 => Panel::Left,
        3 => Panel::Right,
        4 => Panel::Front,
        5 => Panel::Back,
        _ => Panel::Unknown,
    };
    match (visible, panel) {
        (false, Panel::Top) => Panel::Unknown,
        (false, Panel::Unknown) => Panel::Unknown,
        (false, _) => Panel::Internal,
        (true, p) => p,
    }
}

/// On a desktop only "back" and "front" mean something. Many boards leave
/// the panel field at its zero default ("top") even for rear ports, so the
/// other values are reported as unknown rather than guessed.
pub fn desktop_panel(p: Panel) -> Panel {
    match p {
        Panel::Back | Panel::Front | Panel::Internal => p,
        _ => Panel::Unknown,
    }
}

/// SMBIOS memory form factor code -> name.
pub fn memory_form(code: u64) -> &'static str {
    match code {
        8 => "DIMM",
        12 => "SODIMM",
        13 => "SRIMM",
        _ => "",
    }
}

/// Parses "Port_#0004.Hub_#0003" -> (port 4, hub 3).
pub fn usb_location(loc: &str) -> (Option<u32>, Option<u32>) {
    let num = |key: &str| {
        let rest = &loc[loc.find(key)? + key.len()..];
        rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
    };
    (num("Port_#"), num("Hub_#"))
}

/// PCI vendor ID (hex) -> company, for both chip makers and card makers.
pub fn pci_vendor(id: &str) -> &'static str {
    match id.to_ascii_uppercase().as_str() {
        "10DE" => "NVIDIA",
        "1002" => "AMD",
        "8086" => "Intel",
        "1458" => "Gigabyte",
        "1043" => "ASUS",
        "1462" => "MSI",
        "3842" => "EVGA",
        "19DA" => "Zotac",
        "1569" => "Palit",
        "10B0" => "Gainward",
        "196E" => "PNY",
        "1682" => "XFX",
        "1DA2" => "Sapphire",
        "1849" => "ASRock",
        "148C" => "PowerColor",
        "1B4C" => "Galax/KFA2",
        "7377" => "Colorful",
        "1028" => "Dell",
        "103C" => "HP",
        "17AA" => "Lenovo",
        "1025" => "Acer",
        _ => "",
    }
}

/// Extracts (chip vendor, card maker) from a PNP ID such as
/// `PCI\VEN_10DE&DEV_2484&SUBSYS_40691458&REV_A1\...`.
pub fn gpu_vendors(pnp_id: &str) -> (&'static str, &'static str) {
    let field = |key: &str| {
        let up = pnp_id.to_ascii_uppercase();
        let start = up.find(key)? + key.len();
        Some(up[start..].chars().take_while(char::is_ascii_hexdigit).collect::<String>())
    };
    let chip = field("VEN_").map(|v| pci_vendor(&v)).unwrap_or("");
    // SUBSYS_ssssvvvv: the card maker is the last four digits.
    let partner = field("SUBSYS_")
        .filter(|s| s.len() == 8)
        .map(|s| pci_vendor(&s[4..]))
        .unwrap_or("");
    (chip, partner)
}

/// WMI datetime "20230512000000.000000+000" -> "2023-05-12".
pub fn wmi_date(s: &str) -> String {
    let d: String = s.chars().take(8).collect();
    if d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()) {
        format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..])
    } else {
        String::new()
    }
}

/// MSFT_PhysicalDisk.BusType -> name.
pub fn bus_name(code: u64) -> &'static str {
    match code {
        1 => "SCSI",
        3 => "ATA",
        6 => "Fibre Channel",
        7 => "USB",
        8 => "RAID",
        9 => "iSCSI",
        10 => "SAS",
        11 => "SATA",
        12 => "SD",
        13 => "MMC",
        15 => "Virtual",
        16 => "Storage Spaces",
        17 => "NVMe",
        _ => "Other",
    }
}

/// Media type from MSFT_PhysicalDisk (3 = HDD, 4 = SSD), falling back to
/// the spindle speed (0 = no moving parts) and the bus.
pub fn media_name(media_type: u64, spindle_rpm: Option<u64>, bus: &str) -> &'static str {
    match media_type {
        3 => "HDD",
        4 => "SSD",
        // Unspecified media often comes with a bogus spindle speed of 0,
        // so only a real RPM value is trusted here.
        _ => match spindle_rpm {
            Some(rpm) if rpm > 0 && rpm < u32::MAX as u64 => "HDD",
            _ if bus == "NVMe" => "SSD",
            _ => "Unknown",
        },
    }
}

/// Last-resort media guess from well-known model-number formats, for drive
/// controllers that answer neither the media type nor the seek-penalty query.
pub fn media_from_model(model: &str) -> Option<&'static str> {
    let m = model.to_ascii_uppercase();
    let m = m.strip_prefix("WDC ").unwrap_or(&m);
    let digits_after = |prefix: &str| {
        m.strip_prefix(prefix)
            .is_some_and(|rest| rest.chars().take_while(char::is_ascii_digit).count() >= 2)
    };
    if m.contains("SSD") || m.contains("NVME") {
        Some("SSD")
    } else if digits_after("WD")
        || digits_after("ST")
        || digits_after("HD")
        || ["HGST H", "HITACHI H", "TOSHIBA DT", "TOSHIBA MQ", "TOSHIBA HDW", "SAMSUNG HD"]
            .iter()
            .any(|p| m.starts_with(p))
    {
        Some("HDD")
    } else {
        None
    }
}

pub fn health_name(code: u64) -> &'static str {
    match code {
        0 => "Healthy",
        1 => "Warning",
        2 => "Unhealthy",
        _ => "Unknown",
    }
}

/// Classifies a USB device for the UI from its PnP class and name.
pub fn usb_kind(pnp_class: &str, name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    match pnp_class {
        _ if n.contains("hub") => "Hub",
        "Mouse" => "Mouse",
        "Keyboard" => "Keyboard",
        "DiskDrive" | "WPD" | "USBSTOR" => "Storage",
        "Image" | "Camera" => "Camera",
        "Media" | "AudioEndpoint" => "Audio",
        "Bluetooth" => "Bluetooth",
        "Net" => "Network",
        _ if n.contains("mouse") => "Mouse",
        _ if n.contains("keyboard") => "Keyboard",
        _ if n.contains("headset") || n.contains("audio") || n.contains("microphone") => "Audio",
        _ if n.contains("webcam") || n.contains("camera") => "Camera",
        _ if n.contains("bluetooth") => "Bluetooth",
        _ if n.contains("receiver") => "Wireless receiver",
        _ if n.contains("led") || n.contains("aura") || n.contains("rgb") => "RGB controller",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_names() {
        assert_eq!(slot_lanes("PCIEX16_1"), Some(16));
        assert_eq!(slot_lanes("PCIEX1_2"), Some(1));
        assert_eq!(slot_lanes("PCIe x4 Slot 3"), Some(4));
        assert_eq!(slot_lanes("M.2_1"), None);
    }

    #[test]
    fn form_factors() {
        assert_eq!(guess_form_factor("TUF GAMING X570-PRO WIFI II", 4, 4, false), FormFactor::Atx);
        assert_eq!(guess_form_factor("TUF GAMING B550M-PLUS", 2, 4, false), FormFactor::MicroAtx);
        assert_eq!(guess_form_factor("ROG STRIX B550-I GAMING", 1, 2, false), FormFactor::MiniItx);
        assert_eq!(guess_form_factor("B450 I AORUS PRO WIFI-ITX", 1, 2, false), FormFactor::MiniItx);
        assert_eq!(guess_form_factor("anything", 4, 4, true), FormFactor::Laptop);
    }

    #[test]
    fn pld_panels() {
        let mut pld = [0u8; 20];
        assert_eq!(pld_panel(&pld), Panel::Unknown, "zeroed buffer");
        pld[8] = 1 | (5 << 3);
        assert_eq!(pld_panel(&pld), Panel::Back);
        pld[8] = 1 | (4 << 3);
        assert_eq!(pld_panel(&pld), Panel::Front);
        pld[8] = 4 << 3;
        assert_eq!(pld_panel(&pld), Panel::Internal, "not user visible");
        assert_eq!(pld_panel(&[1, 2]), Panel::Unknown, "short buffer");
    }

    #[test]
    fn usb_locations() {
        assert_eq!(usb_location("Port_#0004.Hub_#0003"), (Some(4), Some(3)));
        assert_eq!(usb_location("0006.0000.0003.005"), (None, None));
    }

    #[test]
    fn gpu_ids() {
        let id = r"PCI\VEN_10DE&DEV_2484&SUBSYS_40691458&REV_A1\4&1D81E16&0&0019";
        assert_eq!(gpu_vendors(id), ("NVIDIA", "Gigabyte"));
        assert_eq!(gpu_vendors("ROOT\\BasicDisplay"), ("", ""));
    }

    #[test]
    fn misc() {
        assert_eq!(wmi_date("20230512000000.000000+000"), "2023-05-12");
        assert_eq!(wmi_date(""), "");
        assert_eq!(media_name(0, None, "NVMe"), "SSD");
        assert_eq!(media_name(0, Some(5400), "SATA"), "HDD");
        assert_eq!(media_name(0, Some(u32::MAX as u64), "SATA"), "Unknown");
        assert_eq!(media_name(0, Some(0), "SATA"), "Unknown");
        assert_eq!(media_from_model("WDC WD20EARX-00PASB0"), Some("HDD"));
        assert_eq!(media_from_model("ST2000DM008-2FR102"), Some("HDD"));
        assert_eq!(media_from_model("Samsung SSD 870 EVO 1TB"), Some("SSD"));
        assert_eq!(media_from_model("WDS500G2B0A"), None);
        assert_eq!(desktop_panel(Panel::Top), Panel::Unknown);
        assert_eq!(desktop_panel(Panel::Front), Panel::Front);
        assert_eq!(desktop_panel(Panel::Back), Panel::Back);
        assert_eq!(usb_kind("USB", "AURA LED Controller"), "RGB controller");
        assert_eq!(memory_kind(26), "DDR4");
        assert_eq!(usb_kind("HIDClass", "G403 Prodigy Gaming Mouse"), "Mouse");
        assert_eq!(usb_kind("USB", "Generic USB Hub"), "Hub");
    }
}
