//! Turns detected hardware into good search phrases for product pictures.

use serde::{Deserialize, Serialize};
use syscura_core::hw::HardwareInfo;

/// One part of the PC that can have a picture.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartQuery {
    /// Stable key used by the UI and the cache, e.g. "board", "gpu:0".
    pub key: String,
    /// "board", "cpu", "gpu", "ram", "disk", "usb".
    pub kind: String,
    /// Search phrase, e.g. "ASUS TUF GAMING X570-PRO WIFI II motherboard".
    pub query: String,
    /// Article to try on Wikipedia when web search finds nothing.
    pub wiki_hint: Option<String>,
    /// Brand, used to prefer pictures from the brand's own website.
    pub brand: Option<String>,
    /// An exact picture known in advance (e.g. from a board profile).
    pub direct_url: Option<String>,
    pub direct_page: Option<String>,
}

/// Boards with a verified profile: (product name, maker's photo, product page).
/// Keep in sync with `ui/src/lib/board/profiles.ts`.
const BOARD_PHOTOS: &[(&str, &str, &str)] = &[(
    "TUF GAMING X570-PRO WIFI II",
    "https://dlcdnwebimgs.asus.com/gain/3e75d067-659d-43dc-8388-8b1159535cfa/",
    "https://www.asus.com/motherboards-components/motherboards/tuf-gaming/tuf-gaming-x570-pro-wifi-ii/",
)];

/// The maker's own top-down photo for a board with a verified profile.
pub fn board_photo(product: &str) -> Option<(&'static str, &'static str)> {
    BOARD_PHOTOS
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(product.trim()))
        .map(|(_, url, page)| (*url, *page))
}

/// Short brand names for the long legal names firmware reports.
pub fn brand(raw: &str) -> String {
    let r = raw.trim();
    let u = r.to_ascii_uppercase();
    let known = [
        ("ASUSTEK", "ASUS"),
        ("MICRO-STAR", "MSI"),
        ("GIGABYTE", "Gigabyte"),
        ("ASROCK", "ASRock"),
        ("G SKILL", "G.Skill"),
        ("G.SKILL", "G.Skill"),
        ("CORSAIR", "Corsair"),
        ("KINGSTON", "Kingston"),
        ("CRUCIAL", "Crucial"),
        ("MICRON", "Micron"),
        ("SAMSUNG", "Samsung"),
        ("SK HYNIX", "SK hynix"),
        ("HYNIX", "SK hynix"),
        ("TEAM", "TeamGroup"),
        ("ADATA", "ADATA"),
        ("PATRIOT", "Patriot"),
        ("LOGITECH", "Logitech"),
        ("RAZER", "Razer"),
        ("BIOSTAR", "Biostar"),
        ("EVGA", "EVGA"),
    ];
    for (needle, name) in known {
        if u.contains(needle) {
            return name.to_string();
        }
    }
    // Drop legal suffixes such as "Inc." or "Co., Ltd.".
    r.split([',', '('])
        .next()
        .unwrap_or(r)
        .trim_end_matches(" Inc.")
        .trim_end_matches(" INC.")
        .trim()
        .to_string()
}

/// "AMD Ryzen 5 5600X 6-Core Processor" -> "AMD Ryzen 5 5600X".
pub fn cpu_model(name: &str) -> String {
    let words: Vec<&str> = name
        .split_whitespace()
        .filter(|w| {
            let l = w.to_ascii_lowercase();
            !(l.ends_with("-core") || l == "processor" || l == "cpu" || l.contains("@") || l.ends_with("ghz") || l == "with")
        })
        .take_while(|w| !w.eq_ignore_ascii_case("radeon"))
        .collect();
    words.join(" ").replace("(R)", "").replace("(TM)", "").trim().to_string()
}

/// Consumer brands by USB vendor ID. Chip makers (C-Media, Realtek,
/// Holtek, ...) are deliberately absent: their ID says nothing about whose
/// product it is, so a picture search would be a guess.
fn usb_brand(instance_id: &str) -> Option<&'static str> {
    let up = instance_id.to_ascii_uppercase();
    let vid = &up[up.find("VID_")? + 4..];
    Some(match vid.get(..4)? {
        "046D" => "Logitech",
        "1532" => "Razer",
        "1B1C" => "Corsair",
        "1038" => "SteelSeries",
        "0951" => "HyperX",
        "03F0" => "HP",
        "0B05" => "ASUS",
        "0DB0" => "MSI",
        "045E" => "Microsoft",
        "054C" => "Sony",
        "057E" => "Nintendo",
        "28DE" => "Valve",
        "0781" => "SanDisk",
        "1058" => "Western Digital",
        "0BC2" => "Seagate",
        "04E8" => "Samsung",
        "05AC" => "Apple",
        "2516" => "Cooler Master",
        "1E7D" => "ROCCAT",
        "3434" => "Keychron",
        "046A" => "Cherry",
        "0B0E" => "Jabra",
        _ => return None,
    })
}

/// Generic USB names that make pointless searches.
fn meaningful_usb_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !(n.is_empty() || n.contains("usb") && (n.contains("device") || n.contains("hub")) || n.contains("wireless_device") || n.contains("hid-compliant"))
}

pub fn parts(hw: &HardwareInfo) -> Vec<PartQuery> {
    let mut out = Vec::new();
    let mut push = |key: String, kind: &str, query: String, wiki_hint: Option<String>| {
        if !query.trim().is_empty() {
            let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
            let brand = query.split_whitespace().next().map(str::to_string);
            out.push(PartQuery { key, kind: kind.into(), query, wiki_hint, brand, direct_url: None, direct_page: None });
        }
    };
    if !hw.board.product.is_empty() {
        push(
            "board".into(),
            "board",
            format!("{} {} motherboard", brand(&hw.board.manufacturer), hw.board.product),
            None,
        );
    }
    for (i, c) in hw.cpus.iter().enumerate() {
        let model = cpu_model(&c.name);
        let hint = if model.contains("Ryzen") { Some("Ryzen".into()) } else if model.contains("Core") { Some("Intel Core".into()) } else { None };
        push(format!("cpu:{i}"), "cpu", model, hint);
    }
    for (i, g) in hw.gpus.iter().enumerate() {
        if g.name.contains("Basic Display") || g.name.contains("Remote") {
            continue;
        }
        let chip = g.name.trim_start_matches("NVIDIA ").trim_start_matches("AMD ");
        let hint = if g.name.contains("RTX 30") { Some("GeForce 30 series".into()) }
            else if g.name.contains("RTX 40") { Some("GeForce 40 series".into()) }
            else if g.name.contains("RTX 50") { Some("GeForce 50 series".into()) }
            else if g.name.contains("RTX 20") { Some("GeForce 20 series".into()) }
            else { None };
        push(format!("gpu:{i}"), "gpu", format!("{} {chip} graphics card", g.board_partner), hint);
    }
    // Identical sticks share one picture.
    let mut seen = Vec::new();
    for m in &hw.memory.sticks {
        if seen.contains(&m.part_number) {
            continue;
        }
        seen.push(m.part_number.clone());
        push(format!("ram:{}", m.part_number), "ram", format!("{} {} {} memory", brand(&m.manufacturer), m.part_number, m.kind), None);
    }
    for d in &hw.disks {
        let model = d.model.trim_start_matches("WDC ").to_string();
        let hint = (d.media == "HDD").then(|| "Hard disk drive".to_string()).or_else(|| (d.media == "SSD").then(|| "Solid-state drive".to_string()));
        // "WDC" model prefixes mean Western Digital.
        let maker = if d.model.starts_with("WDC") || d.model.starts_with("WD") { "Western Digital " } else { "" };
        push(format!("disk:{}", d.device_id), "disk", format!("{maker}{model} {}", d.media.replace("Unknown", "drive")), hint);
    }
    for u in &hw.usb {
        if !meaningful_usb_name(&u.name) || matches!(u.kind.as_str(), "Hub" | "RGB controller" | "Bluetooth") {
            continue;
        }
        // Only search when the brand is known; otherwise a picture of some
        // other product with the same name could show up.
        let Some(maker) = usb_brand(&u.instance_id) else { continue };
        push(format!("usb:{}", u.instance_id), "usb", format!("{maker} {} {}", u.name, u.kind.to_ascii_lowercase()), None);
    }
    if let (Some((url, page)), Some(board)) = (board_photo(&hw.board.product), out.iter_mut().find(|p| p.key == "board")) {
        board.direct_url = Some(url.into());
        board.direct_page = Some(page.into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use syscura_core::hw::*;

    #[test]
    fn brands() {
        assert_eq!(brand("ASUSTeK COMPUTER INC."), "ASUS");
        assert_eq!(brand("Micro-Star International Co., Ltd."), "MSI");
        assert_eq!(brand("G Skill Intl"), "G.Skill");
        assert_eq!(brand("Acme Widgets Inc."), "Acme Widgets");
    }

    #[test]
    fn cpu_names() {
        assert_eq!(cpu_model("AMD Ryzen 5 5600X 6-Core Processor             "), "AMD Ryzen 5 5600X");
        assert_eq!(cpu_model("Intel(R) Core(TM) i7-12700K CPU @ 3.60GHz"), "Intel Core i7-12700K");
        assert_eq!(cpu_model("AMD Ryzen 7 5700G with Radeon Graphics"), "AMD Ryzen 7 5700G");
    }

    #[test]
    fn queries_for_this_pc() {
        let mut hw = HardwareInfo {
            board: BoardInfo { manufacturer: "ASUSTeK COMPUTER INC.".into(), product: "TUF GAMING X570-PRO WIFI II".into(), ..Default::default() },
            ..Default::default()
        };
        hw.gpus.push(GpuInfo { name: "NVIDIA GeForce RTX 3070".into(), board_partner: "Gigabyte".into(), ..Default::default() });
        for slot in ["DIMM_A2", "DIMM_B2"] {
            hw.memory.sticks.push(MemoryStick { slot: slot.into(), manufacturer: "G Skill Intl".into(), part_number: "F4-3600C16-8GVK".into(), kind: "DDR4".into(), ..Default::default() });
        }
        hw.disks.push(DiskInfo { model: "WDC WD20EARX-00PASB0".into(), media: "HDD".into(), device_id: "0".into(), ..Default::default() });
        hw.usb.push(UsbDevice { name: "G815".into(), kind: "Keyboard".into(), manufacturer: "Logitech".into(), instance_id: "USB\\VID_046D&PID_C33F\\1".into(), ..Default::default() });
        hw.usb.push(UsbDevice { name: "USB Composite Device".into(), kind: "Other".into(), ..Default::default() });
        // Unknown brand (0D8C is C-Media, only the audio chip maker): no search.
        hw.usb.push(UsbDevice { name: "G800".into(), kind: "Audio".into(), instance_id: "USB\\VID_0D8C&PID_036D\\4".into(), ..Default::default() });

        let q: Vec<(String, String)> = parts(&hw).into_iter().map(|p| (p.key, p.query)).collect();
        assert_eq!(
            q,
            vec![
                ("board".into(), "ASUS TUF GAMING X570-PRO WIFI II motherboard".into()),
                ("gpu:0".into(), "Gigabyte GeForce RTX 3070 graphics card".into()),
                ("ram:F4-3600C16-8GVK".into(), "G.Skill F4-3600C16-8GVK DDR4 memory".into()),
                ("disk:0".into(), "Western Digital WD20EARX-00PASB0 HDD".into()),
                ("usb:USB\\VID_046D&PID_C33F\\1".into(), "Logitech G815 keyboard".into()),
            ]
        );
    }
}
