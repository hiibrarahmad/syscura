//! Extra checks on evidence before it becomes a finding, e.g. "is the
//! program behind this new service signed, and where does it live?"
//!
//! Results are added to the evidence. `_harmful` and `_severity` override
//! the rule's defaults when the checks make the finding more (or less) worrying.

use std::collections::BTreeMap;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn enrich(kind: &str, evidence: &mut BTreeMap<String, String>) {
    if kind == "bugcheck" {
        let raw = evidence.get("param1").cloned().unwrap_or_default();
        let code = raw.split_whitespace().next().unwrap_or("");
        let value = u64::from_str_radix(code.trim_start_matches("0x").trim_start_matches("0X"), 16).ok();
        let (name, hint) = value.map(bugcheck).unwrap_or(("an unknown stop code", "Search the web for the stop code."));
        evidence.insert("StopCode".into(), value.map(|v| format!("0x{v:X}")).unwrap_or_else(|| code.to_string()));
        evidence.insert("StopName".into(), name.into());
        evidence.insert("StopHint".into(), hint.into());
        return;
    }
    if kind == "service_image" {
        let Some(raw) = evidence.get("ImagePath").cloned() else { return };
        // Kernel drivers have no account; say what they are instead.
        if evidence.get("AccountName").is_none_or(|a| a.trim().is_empty()) {
            evidence.insert("AccountName".into(), "a kernel driver".into());
        }
        let path = image_file(&raw);
        evidence.insert("File".into(), path.clone());
        let place = classify_location(&path);
        evidence.insert("Location".into(), place.describe().into());
        let sig = signature(&path);
        evidence.insert("Signature".into(), sig.describe());
        let (harmful, severity) = match (&sig, place) {
            (Signature::Valid(signer), Location::System | Location::ProgramFiles) if is_well_known(signer) => ("no", "info"),
            (Signature::Valid(_), Location::UserWritable) => ("maybe", "warning"),
            (Signature::Valid(_), _) => ("no", "info"),
            // The file is gone (tools such as CPU-Z unpack a driver and delete
            // it later), so it cannot be judged: worth a look, not an alarm.
            (Signature::Unknown, Location::UserWritable) => ("maybe", "warning"),
            (_, Location::UserWritable) => ("yes", "error"),
            (Signature::Missing, _) => ("maybe", "warning"),
            (Signature::Invalid(_), _) => ("yes", "error"),
            (Signature::Unknown, _) => ("maybe", "warning"),
        };
        evidence.insert("_harmful".into(), harmful.into());
        evidence.insert("_severity".into(), severity.into());
    }
}

/// Official names of common Windows stop codes, with the usual cause.
pub fn bugcheck(code: u64) -> (&'static str, &'static str) {
    const DRIVER: &str = "This is usually caused by a faulty or outdated driver.";
    const MEMORY: &str = "This is often faulty memory or unstable memory settings (XMP/EXPO), sometimes a driver.";
    const DISK: &str = "This usually points to the drive, its cable or its storage driver.";
    const GPU: &str = "This comes from the graphics driver or the graphics card.";
    const HARDWARE: &str = "This points to hardware: CPU, memory, overclocking or overheating.";
    const SYSTEM: &str = "A critical part of Windows stopped; check the drive and repair system files.";
    match code {
        0x0A => ("IRQL_NOT_LESS_OR_EQUAL", DRIVER),
        0x19 => ("BAD_POOL_HEADER", MEMORY),
        0x1A => ("MEMORY_MANAGEMENT", MEMORY),
        0x1E | 0x1000_001E => ("KMODE_EXCEPTION_NOT_HANDLED", DRIVER),
        0x24 => ("NTFS_FILE_SYSTEM", DISK),
        0x3B => ("SYSTEM_SERVICE_EXCEPTION", DRIVER),
        0x3D => ("INTERRUPT_EXCEPTION_NOT_HANDLED", DRIVER),
        0x4E => ("PFN_LIST_CORRUPT", MEMORY),
        0x50 => ("PAGE_FAULT_IN_NONPAGED_AREA", MEMORY),
        0x7A => ("KERNEL_DATA_INPAGE_ERROR", DISK),
        0x7B => ("INACCESSIBLE_BOOT_DEVICE", DISK),
        0x7E | 0x1000_007E => ("SYSTEM_THREAD_EXCEPTION_NOT_HANDLED", DRIVER),
        0x7F => ("UNEXPECTED_KERNEL_MODE_TRAP", HARDWARE),
        0x8E | 0x1000_008E => ("KERNEL_MODE_EXCEPTION_NOT_HANDLED", DRIVER),
        0x9F => ("DRIVER_POWER_STATE_FAILURE", DRIVER),
        0xA0 => ("INTERNAL_POWER_ERROR", DRIVER),
        0xBE => ("ATTEMPTED_WRITE_TO_READONLY_MEMORY", DRIVER),
        0xC2 => ("BAD_POOL_CALLER", DRIVER),
        0xC4 => ("DRIVER_VERIFIER_DETECTED_VIOLATION", DRIVER),
        0xC5 => ("DRIVER_CORRUPTED_EXPOOL", DRIVER),
        0xD1 => ("DRIVER_IRQL_NOT_LESS_OR_EQUAL", DRIVER),
        0xED => ("UNMOUNTABLE_BOOT_VOLUME", DISK),
        0xEF => ("CRITICAL_PROCESS_DIED", SYSTEM),
        0xF4 => ("CRITICAL_OBJECT_TERMINATION", SYSTEM),
        0xFC => ("ATTEMPTED_EXECUTE_OF_NOEXECUTE_MEMORY", DRIVER),
        0x101 => ("CLOCK_WATCHDOG_TIMEOUT", HARDWARE),
        0x109 => ("CRITICAL_STRUCTURE_CORRUPTION", MEMORY),
        0x116 => ("VIDEO_TDR_FAILURE", GPU),
        0x117 => ("VIDEO_TDR_TIMEOUT_DETECTED", GPU),
        0x119 => ("VIDEO_SCHEDULER_INTERNAL_ERROR", GPU),
        0x124 => ("WHEA_UNCORRECTABLE_ERROR", HARDWARE),
        0x133 => ("DPC_WATCHDOG_VIOLATION", "A driver took too long; often storage or chipset drivers, or SSD firmware."),
        0x139 => ("KERNEL_SECURITY_CHECK_FAILURE", DRIVER),
        0x13A => ("KERNEL_MODE_HEAP_CORRUPTION", DRIVER),
        0x154 => ("UNEXPECTED_STORE_EXCEPTION", DISK),
        0xC000_021A => ("STATUS_SYSTEM_PROCESS_TERMINATED", SYSTEM),
        _ => ("a stop code without a common name", "Search the web for the stop code."),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    System,
    ProgramFiles,
    UserWritable,
    Other,
}

impl Location {
    fn describe(self) -> &'static str {
        match self {
            Location::System => "Windows folder",
            Location::ProgramFiles => "Program Files",
            Location::UserWritable => "a user, Temp, Downloads or ProgramData folder (unusual for a service)",
            Location::Other => "another folder",
        }
    }
}

pub fn classify_location(path: &str) -> Location {
    let p = path.to_ascii_lowercase().replace('/', "\\");
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    if ["\\users\\", "\\appdata\\", "\\temp\\", "\\downloads\\", "\\programdata\\", "\\$recycle.bin\\"]
        .iter()
        .any(|s| p.contains(s))
    {
        Location::UserWritable
    } else if p.starts_with(&format!("{windir}\\")) {
        Location::System
    } else if p.contains("\\program files\\") || p.contains("\\program files (x86)\\") {
        Location::ProgramFiles
    } else {
        Location::Other
    }
}

/// Turns a service ImagePath into a plain file path:
/// `"C:\x\a.exe" -k foo` -> `C:\x\a.exe`, `\SystemRoot\System32\drivers\a.sys`
/// -> `C:\Windows\System32\drivers\a.sys`, `system32\DRIVERS\a.sys` -> ...
pub fn image_file(raw: &str) -> String {
    let s = raw.trim();
    let mut path = if let Some(rest) = s.strip_prefix('"') {
        rest.split('"').next().unwrap_or(rest).to_string()
    } else {
        let lower = s.to_ascii_lowercase();
        let end = [".exe", ".sys", ".dll"]
            .iter()
            .filter_map(|ext| lower.find(ext).map(|i| i + ext.len()))
            .min()
            .unwrap_or(s.len());
        s[..end].to_string()
    };
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    if let Some(rest) = path.strip_prefix(r"\??\") {
        path = rest.to_string();
    }
    let lower = path.to_ascii_lowercase();
    if lower.starts_with(r"\systemroot\") {
        path = format!("{windir}{}", &path[r"\systemroot".len()..]);
    } else if lower.starts_with(r"system32\") {
        path = format!("{windir}\\{path}");
    } else if lower.starts_with("%systemroot%") {
        path = format!("{windir}{}", &path["%systemroot%".len()..]);
    }
    path
}

#[derive(Debug, Clone, PartialEq)]
pub enum Signature {
    Valid(String),
    Missing,
    Invalid(String),
    Unknown,
}

impl Signature {
    fn describe(&self) -> String {
        match self {
            Signature::Valid(s) => format!("Valid, signed by {s}"),
            Signature::Missing => "Not signed".into(),
            Signature::Invalid(why) => format!("Signature is NOT valid ({why})"),
            Signature::Unknown => "Could not be checked".into(),
        }
    }
}

fn is_well_known(signer: &str) -> bool {
    let s = signer.to_ascii_lowercase();
    ["microsoft", "nvidia", "advanced micro devices", "amd", "intel", "realtek", "asustek", "logitech", "google", "mozilla"]
        .iter()
        .any(|k| s.contains(k))
}

/// Checks the file's Authenticode signature with Windows' own cmdlet. The
/// path is passed in an environment variable, never inside the script, so
/// a crafted path cannot become code.
pub fn signature(path: &str) -> Signature {
    if !PathBuf::from(path).exists() {
        return Signature::Unknown;
    }
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = PathBuf::from(windir).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let script = "$s = Get-AuthenticodeSignature -LiteralPath $env:SYSCURA_FILE; \
                  $n = if ($s.SignerCertificate) { $s.SignerCertificate.GetNameInfo('SimpleName', $false) } else { '' }; \
                  \"$($s.Status)|$n\"";
    let out = Command::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("SYSCURA_FILE", path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(out) = out else { return Signature::Unknown };
    let text = String::from_utf8_lossy(&out.stdout);
    let (status, signer) = text.trim().split_once('|').unwrap_or((text.trim(), ""));
    match status {
        "Valid" => Signature::Valid(if signer.is_empty() { "unknown publisher".into() } else { signer.to_string() }),
        "NotSigned" => Signature::Missing,
        "" | "UnknownError" => Signature::Unknown,
        other => Signature::Invalid(other.to_string()),
    }
}

/// Checks many files in one PowerShell call (the process watch would
/// otherwise start one PowerShell per program). Paths travel in an
/// environment variable, separated by `|`, which Windows paths cannot
/// contain. Files that do not exist come back as `Unknown`.
pub fn signatures(paths: &[String]) -> std::collections::HashMap<String, Signature> {
    let mut out: std::collections::HashMap<String, Signature> =
        paths.iter().map(|p| (p.clone(), Signature::Unknown)).collect();
    let existing: Vec<&String> = paths.iter().filter(|p| !p.contains('|') && PathBuf::from(p).is_file()).collect();
    if existing.is_empty() {
        return out;
    }
    let joined = existing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|");
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = PathBuf::from(windir).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let script = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; \
                  $env:SYSCURA_FILES -split '\\|' | ForEach-Object { \
                    $s = Get-AuthenticodeSignature -LiteralPath $_ -ErrorAction SilentlyContinue; \
                    $n = if ($s.SignerCertificate) { $s.SignerCertificate.GetNameInfo('SimpleName', $false) } else { '' }; \
                    \"$_|$($s.Status)|$n\" }";
    let Ok(res) = Command::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("SYSCURA_FILES", joined)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return out;
    };
    for line in String::from_utf8_lossy(&res.stdout).lines() {
        let mut parts = line.trim_start_matches('\u{feff}').splitn(3, '|');
        let (Some(path), Some(status)) = (parts.next(), parts.next()) else { continue };
        let signer = parts.next().unwrap_or("").trim();
        let sig = match status.trim() {
            "Valid" => Signature::Valid(if signer.is_empty() { "unknown publisher".into() } else { signer.to_string() }),
            "NotSigned" => Signature::Missing,
            "" | "UnknownError" => Signature::Unknown,
            other => Signature::Invalid(other.to_string()),
        };
        if let Some(slot) = out.get_mut(path) {
            *slot = sig;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_paths() {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        assert_eq!(image_file(r#""C:\Program Files\Acme\svc.exe" -k run"#), r"C:\Program Files\Acme\svc.exe");
        assert_eq!(image_file(r"C:\Windows\system32\svchost.exe -k netsvcs -p"), r"C:\Windows\system32\svchost.exe");
        assert_eq!(image_file(r"\SystemRoot\System32\drivers\acme.sys"), format!(r"{windir}\System32\drivers\acme.sys"));
        assert_eq!(image_file(r"system32\DRIVERS\x.sys"), format!(r"{windir}\system32\DRIVERS\x.sys"));
        assert_eq!(image_file(r"\??\C:\Tools\drv.sys"), r"C:\Tools\drv.sys");
    }

    #[test]
    fn bugchecks() {
        let mut ev = BTreeMap::from([(
            "param1".to_string(),
            "0x0000003b (0x0000000080000004, 0xfffff80113f1b1b6, 0xffffd00f671f5950, 0x0000000000000000)".to_string(),
        )]);
        enrich("bugcheck", &mut ev);
        assert_eq!(ev["StopCode"], "0x3B");
        assert_eq!(ev["StopName"], "SYSTEM_SERVICE_EXCEPTION");
        assert!(ev["StopHint"].contains("driver"));
        assert_eq!(bugcheck(0x124).0, "WHEA_UNCORRECTABLE_ERROR");
    }

    #[test]
    fn locations() {
        assert_eq!(classify_location(r"C:\Users\bob\AppData\Local\Temp\x.exe"), Location::UserWritable);
        assert_eq!(classify_location(r"C:\ProgramData\x\y.exe"), Location::UserWritable);
        assert_eq!(classify_location(r"C:\Program Files\Acme\a.exe"), Location::ProgramFiles);
        assert_eq!(classify_location(r"D:\Games\a.exe"), Location::Other);
    }

    #[test]
    fn unsigned_program_in_temp_is_flagged() {
        let mut ev = BTreeMap::from([("ImagePath".to_string(), r"C:\Users\x\AppData\Local\Temp\definitely-missing-123.exe".to_string())]);
        enrich("service_image", &mut ev);
        // The file does not exist, so it cannot be judged either way.
        assert_eq!(ev["_harmful"], "maybe");
        assert_eq!(ev["Signature"], "Could not be checked");
        assert_eq!(ev["AccountName"], "a kernel driver");
    }
}
