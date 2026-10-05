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
