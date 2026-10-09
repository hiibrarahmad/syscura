//! The Security page: is Windows' own protection switched on? Each check
//! reads a setting (registry, or one PowerShell call for BitLocker and
//! Windows Update) and says in plain words what it found and what to do.
//! Fixes, where there is a safe one, are catalog actions the person starts.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use syscura_core::{DefenderInfo, SecurityCheck, SecurityReport};

use crate::reg::{self, HKEY_LOCAL_MACHINE};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn check(id: &str, title: &str, weight: u8) -> SecurityCheck {
    SecurityCheck { id: id.into(), title: title.into(), weight, status: "unknown".into(), ..Default::default() }
}

trait Fill {
    fn set(self, status: &str, detail: impl Into<String>, advice: impl Into<String>) -> Self;
    fn fix(self, action: &str, label: &str) -> Self;
}

impl Fill for SecurityCheck {
    fn set(mut self, status: &str, detail: impl Into<String>, advice: impl Into<String>) -> Self {
        self.status = status.into();
        self.detail = detail.into();
        self.advice = advice.into();
        self
    }

    fn fix(mut self, action: &str, label: &str) -> Self {
        self.action = action.into();
        self.action_label = label.into();
        self
    }
}

pub fn report(defender: Option<DefenderInfo>) -> SecurityReport {
    let extra = powershell_facts();
    let mut checks = vec![
        firewall(),
        defender_realtime(defender.as_ref()),
        definitions(defender.as_ref()),
        tamper(defender.as_ref()),
        uac(),
        windows_updates(&extra),
        smb1(),
        remote_desktop(),
        secure_boot(),
        driver_blocklist(),
        bitlocker(&extra),
        lsa_protection(),
        controlled_folders(&extra),
    ];
    checks.retain(|c| !c.title.is_empty());
    SecurityReport::new(checks)
}

fn firewall() -> SecurityCheck {
    let c = check("firewall", "Windows Firewall", 3);
    let base = r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy";
    let policy = r"SOFTWARE\Policies\Microsoft\WindowsFirewall";
    let mut off = Vec::new();
    let mut known = false;
    for (profile, policy_profile, label) in [
        ("DomainProfile", "DomainProfile", "work (domain)"),
        ("StandardProfile", "PrivateProfile", "private"),
        ("PublicProfile", "PublicProfile", "public"),
    ] {
        // A Group Policy setting overrides the local one.
        let on = reg::dword(HKEY_LOCAL_MACHINE, &format!(r"{policy}\{policy_profile}"), "EnableFirewall")
            .or_else(|| reg::dword(HKEY_LOCAL_MACHINE, &format!(r"{base}\{profile}"), "EnableFirewall"));
        if let Some(v) = on {
            known = true;
            if v == 0 {
                off.push(label);
            }
        }
    }
    if !known {
        return c.set("unknown", "Windows did not say whether the firewall is on.", "Open Windows Security → Firewall & network protection.");
    }
    if off.is_empty() {
        c.set("good", "On for every network type.", "")
    } else {
        c.set(
            "bad",
            format!("Off for {} networks.", off.join(", ")),
            "Unless another security program runs its own firewall, turn Windows Firewall on.",
        )
        .fix("firewall.enable", "Turn the firewall on")
    }
}

fn defender_realtime(d: Option<&DefenderInfo>) -> SecurityCheck {
    let c = check("defender", "Microsoft Defender real-time protection", 3);
    match d {
        None => c.set(
            "unknown",
            "Defender's status is not available (another antivirus may have replaced it).",
            "If you use another antivirus, make sure it is up to date and running.",
        ),
        Some(d) if d.antivirus && d.realtime => c.set("good", "On: files are checked as they are opened.", ""),
        Some(d) if !d.antivirus => c.set(
            "warn",
            "Defender's antivirus is off. Usually another antivirus program has taken over.",
            "If you have no other antivirus, turn Defender back on in Windows Security.",
        ),
        Some(_) => c
            .set("bad", "Real-time protection is OFF: new files are not checked.", "Turn it back on. Malware often switches it off.")
            .fix("defender.enable_realtime", "Turn real-time protection on"),
    }
}

fn definitions(d: Option<&DefenderInfo>) -> SecurityCheck {
    let c = check("definitions", "Virus definitions up to date", 2);
    match d {
        Some(d) if d.antivirus && d.signature_age_days <= 3 => c.set("good", format!("Updated {} day(s) ago.", d.signature_age_days.max(0)), ""),
        Some(d) if d.antivirus => c
            .set("bad", format!("{} days old: new threats are not recognised.", d.signature_age_days), "Update them now and check that Windows Update works.")
            .fix("defender.update", "Update Defender"),
        _ => SecurityCheck { title: String::new(), ..c },
    }
}

fn tamper(d: Option<&DefenderInfo>) -> SecurityCheck {
    let c = check("tamper", "Tamper protection", 1);
    match d {
        Some(d) if d.antivirus && d.tamper_protected => c.set("good", "On: other programs cannot switch Defender off.", ""),
        Some(d) if d.antivirus => c.set(
            "warn",
            "Off: a program with admin rights could switch Defender off quietly.",
            "Windows Security → Virus & threat protection → Manage settings → Tamper Protection: On.",
        ),
        _ => SecurityCheck { title: String::new(), ..c },
    }
}

fn uac() -> SecurityCheck {
    let c = check("uac", "User Account Control (admin prompts)", 3);
    let key = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
    let lua = reg::dword(HKEY_LOCAL_MACHINE, key, "EnableLUA").unwrap_or(1);
    let prompt = reg::dword(HKEY_LOCAL_MACHINE, key, "ConsentPromptBehaviorAdmin").unwrap_or(5);
    let secure = reg::dword(HKEY_LOCAL_MACHINE, key, "PromptOnSecureDesktop").unwrap_or(1);
    if lua == 0 {
        c.set("bad", "Turned OFF: every program runs with full admin rights.", "Turn User Account Control back on (Control Panel → User Accounts → Change User Account Control settings) and restart.")
    } else if prompt == 0 {
        c.set("bad", "Set to never notify: programs get admin rights without asking.", "Move the User Account Control slider up to the default level.")
    } else if secure == 0 {
        c.set("warn", "On, but prompts do not dim the screen, so other programs can interfere with them.", "Use the default User Account Control level.")
    } else {
        c.set("good", "On: programs must ask before changing Windows.", "")
    }
}

fn smb1() -> SecurityCheck {
    let c = check("smb1", "Old file sharing (SMBv1) switched off", 2);
    let on = |svc: &str| {
        let key = format!(r"SYSTEM\CurrentControlSet\Services\{svc}");
        reg::exists(HKEY_LOCAL_MACHINE, &key) && reg::dword(HKEY_LOCAL_MACHINE, &key, "Start").unwrap_or(4) != 4
    };
    let server_param = reg::dword(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Services\LanmanServer\Parameters", "SMB1");
    let client = on("mrxsmb10");
    let server = on("srv") || (server_param == Some(1));
    if client || server {
        c.set(
            "bad",
            "SMBv1 is installed. It is the 1980s protocol WannaCry spread through.",
            "Remove it unless a very old NAS or printer needs it.",
        )
        .fix("smb1.disable", "Remove SMBv1")
    } else {
        c.set("good", "Not installed.", "")
    }
}

fn remote_desktop() -> SecurityCheck {
    let c = check("rdp", "Remote Desktop", 2);
    let key = r"SYSTEM\CurrentControlSet\Control\Terminal Server";
    if reg::dword(HKEY_LOCAL_MACHINE, key, "fDenyTSConnections").unwrap_or(1) != 0 {
        return c.set("good", "Off: nobody can sign in to this PC over the network.", "");
    }
    let nla = reg::dword(HKEY_LOCAL_MACHINE, &format!(r"{key}\WinStations\RDP-Tcp"), "UserAuthentication").unwrap_or(1);
    if nla == 0 {
        c.set("bad", "On, without Network Level Authentication: anyone who can reach the PC sees the sign-in screen.", "Turn Remote Desktop off if you do not use it.")
            .fix("rdp.disable", "Turn Remote Desktop off")
    } else {
        c.set("warn", "On: this PC accepts remote sign-ins.", "Turn it off if you do not use it, and use a strong password if you do.")
            .fix("rdp.disable", "Turn Remote Desktop off")
    }
}

fn secure_boot() -> SecurityCheck {
    let c = check("secureboot", "Secure Boot", 2);
    match reg::dword(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\SecureBoot\State", "UEFISecureBootEnabled") {
        Some(1) => c.set("good", "On: only trusted code can start before Windows.", ""),
        Some(_) => c.set("warn", "Off. Boot-level malware (bootkits) could load before Windows.", "Turn on Secure Boot in the PC's UEFI/BIOS settings (see the PC maker's guide)."),
        None => c.set("warn", "Not available: the PC starts in legacy (BIOS) mode.", "Newer PCs can switch to UEFI mode; follow the PC maker's guide."),
    }
}

/// Windows build number (22621 = Windows 11 22H2).
fn build() -> u32 {
    reg::string(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber")
        .and_then(|b| b.parse().ok())
        .unwrap_or(0)
}

fn driver_blocklist() -> SecurityCheck {
    let c = check("blocklist", "Microsoft's vulnerable driver blocklist", 2);
    let v = reg::dword(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\CI\Config", "VulnerableDriverBlocklistEnable");
    // On by default since Windows 11 22H2; off by default before.
    let on = v.map(|v| v == 1).unwrap_or(build() >= 22621);
    if on {
        c.set("good", "On: drivers with known security holes cannot load.", "")
    } else {
        c.set(
            "bad",
            "Off: malware can load an old, signed driver with a known hole to take over Windows.",
            "Turn it on (needs a restart). A very old device driver might stop loading; Syscura's undo turns it off again.",
        )
        .fix("driverblocklist.enable", "Turn the blocklist on")
    }
}

fn lsa_protection() -> SecurityCheck {
    let c = check("lsa", "Password protection (LSA protection)", 1);
    match reg::dword(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Lsa", "RunAsPPL") {
        Some(1 | 2) => c.set("good", "On: password-stealing tools cannot read Windows' memory of sign-ins.", ""),
        _ => c.set(
            "warn",
            "Off. Tools such as Mimikatz could read saved sign-in secrets.",
            "Windows Security → Device security → Core isolation → Local Security Authority protection: On (needs a restart).",
        ),
    }
}

/// BitLocker, Windows Update and Controlled Folder Access in one
/// PowerShell call (one short-lived process every few hours).
struct Facts {
    bitlocker: Option<String>,
    last_update_days: Option<i64>,
    cfa: Option<String>,
}

fn powershell_facts() -> Facts {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = PathBuf::from(windir).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let script = "$ErrorActionPreference = 'SilentlyContinue'; \
        $v = Get-CimInstance -Namespace root/cimv2/Security/MicrosoftVolumeEncryption -ClassName Win32_EncryptableVolume -Filter \"DriveLetter='$env:SystemDrive'\"; \
        $b = if ($v) { [string]$v.ProtectionStatus } else { 'none' }; \
        $s = (New-Object -ComObject Microsoft.Update.Session).CreateUpdateSearcher(); \
        $n = $s.GetTotalHistoryCount(); $d = ''; \
        if ($n -gt 0) { $h = $s.QueryHistory(0, [Math]::Min($n, 50)) | Where-Object { $_.ResultCode -eq 2 } | Sort-Object Date -Descending | Select-Object -First 1; if ($h) { $d = [int]((Get-Date) - $h.Date).TotalDays } }; \
        $c = [string](Get-MpPreference).EnableControlledFolderAccess; \
        \"$b|$d|$c\"";
    let out = Command::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let mut p = out.lines().last().unwrap_or("").split('|');
    let nonempty = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Facts {
        bitlocker: nonempty(p.next()),
        last_update_days: nonempty(p.next()).and_then(|d| d.parse().ok()),
        cfa: nonempty(p.next()),
    }
}

fn bitlocker(f: &Facts) -> SecurityCheck {
    let c = check("bitlocker", "Drive encryption (BitLocker)", 2);
    match f.bitlocker.as_deref() {
        Some("1") => c.set("good", "The Windows drive is encrypted: a stolen PC's files cannot be read.", ""),
        Some("0") => c.set(
            "warn",
            "The Windows drive is not encrypted (or encryption is paused).",
            "For laptops especially: turn on BitLocker or Device Encryption in Settings → Privacy & security, and save the recovery key somewhere safe.",
        ),
        Some("none") => c.set(
            "info",
            "BitLocker is not available on this edition of Windows.",
            "Some PCs offer Device Encryption in Settings → Privacy & security.",
        ),
        _ => c.set("unknown", "Could not read the encryption state.", ""),
    }
}

fn windows_updates(f: &Facts) -> SecurityCheck {
    let c = check("updates", "Windows updates installed recently", 3);
    match f.last_update_days {
        Some(d) if d <= 40 => c.set("good", format!("Last update installed {d} day(s) ago."), ""),
        Some(d) => c.set(
            "bad",
            format!("The last successful update was {d} days ago. Security fixes come every month."),
            "Open Settings → Windows Update and install updates. If they keep failing, Syscura's Windows Update fixes can help.",
        ),
        None => c.set("unknown", "Windows did not report its update history.", "Check Settings → Windows Update."),
    }
}

fn controlled_folders(f: &Facts) -> SecurityCheck {
    let c = check("cfa", "Ransomware protection (Controlled folder access)", 0);
    match f.cfa.as_deref() {
        Some("1") => c.set("info", "On: only trusted programs can change your documents.", ""),
        Some(_) => c
            .set(
                "info",
                "Off. Optional: it stops unknown programs from changing Documents, Pictures and Desktop, but can block some legitimate apps.",
                "Turn it on if you want extra ransomware protection; allow apps in Windows Security if they get blocked.",
            )
            .fix("defender.enable_cfa", "Turn it on"),
        None => SecurityCheck { title: String::new(), ..c },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn report_runs_and_scores() {
        let r = super::report(None);
        assert!(r.checks.len() >= 8, "{:?}", r.checks.iter().map(|c| &c.id).collect::<Vec<_>>());
        assert!(r.score <= 100);
        for c in &r.checks {
            assert!(matches!(c.status.as_str(), "good" | "bad" | "warn" | "info" | "unknown"), "{}: {}", c.id, c.status);
            assert!(!c.detail.is_empty() || c.status == "unknown", "{} explains itself", c.id);
        }
    }
}
