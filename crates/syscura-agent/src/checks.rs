//! More places malware hides or changes the PC, checked every 6 hours with
//! the startup scan. Everything here only reads; suspicious findings become
//! events on Syscura's own channels, and rules in kb/rules.toml turn them
//! into problems.
//!
//! - **Startup tricks** ("Syscura/Startup"): debugger hijacks (Image File
//!   Execution Options, silent-exit monitors), a replaced Windows shell or
//!   Userinit, AppInit DLLs, WMI event subscriptions that run commands, and
//!   unsigned services in user folders.
//! - **Network tampering** ("Syscura/Network"): hosts-file entries that
//!   block security sites or redirect popular ones, a proxy, unknown DNS
//!   servers, and browser extensions forced on by policy.
//! - **Drivers** ("Syscura/Drivers"): installed drivers on the LOLDrivers
//!   list of known vulnerable or malicious drivers.

use std::collections::{HashMap, HashSet};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use sha2::{Digest, Sha256};
use syscura_core::Level;

use crate::agent::Msg;
use crate::reg::{self, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_WOW64_32KEY, VIEW64};
use crate::trust::{self, Location, Signature};
use crate::watch::{emit, lock};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn scan_all(tx: &Sender<Msg>, drivers: &DriverCache) {
    let mut found = 0;
    found += debugger_hijacks(tx);
    found += winlogon(tx);
    found += appinit(tx);
    found += wmi_consumers(tx);
    found += user_folder_services(tx);
    found += hosts_file(tx);
    found += proxies(tx);
    found += dns_servers(tx);
    found += forced_extensions(tx);
    found += vulnerable_drivers(tx, drivers);
    crate::log::info(&format!("checked startup tricks, network settings and drivers: {found} things to look at"));
}

fn windir() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())
}

// ------------------------------------------------------------ startup tricks

/// Programs that are normally set as a "debugger" on purpose.
const KNOWN_DEBUGGERS: &[&str] = &["vsjitdebugger", "procexp", "systeminformer", "processhacker", "windbg", "devenv"];
/// Accessibility programs that run on the sign-in screen: a debugger on
/// them opens a SYSTEM command prompt without a password (a classic backdoor).
const SIGNIN_SCREEN_APPS: &[&str] = &["sethc.exe", "utilman.exe", "osk.exe", "magnify.exe", "narrator.exe", "displayswitch.exe", "atbroker.exe"];
/// Windows telemetry programs that privacy tweaks stop from running by
/// pointing their "debugger" at taskkill or similar. Harmless.
const TELEMETRY_PROGRAMS: &[&str] = &["compattelrunner.exe", "devicecensus.exe", "wsqmcons.exe", "aitagent.exe", "diagtrackrunner.exe", "inventory.exe"];

fn debugger_hijacks(tx: &Sender<Msg>) -> usize {
    let mut n = 0;
    let ifeo = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";
    // This key is shared by 64- and 32-bit programs: one view is enough.
    for exe in reg::subkeys(HKEY_LOCAL_MACHINE, ifeo) {
        let Some(debugger) = reg::string(HKEY_LOCAL_MACHINE, &format!(r"{ifeo}\{exe}"), "Debugger") else { continue };
        let lower = exe.to_ascii_lowercase();
        if debugger.trim().is_empty()
            || KNOWN_DEBUGGERS.iter().any(|k| debugger.to_ascii_lowercase().contains(k))
            || TELEMETRY_PROGRAMS.contains(&lower.as_str())
        {
            continue;
        }
        let backdoor = SIGNIN_SCREEN_APPS.contains(&lower.as_str());
        n += 1;
        emit(tx, "Syscura/Startup", 12, if backdoor { Level::Critical } else { Level::Error }, &format!("ifeo|{exe}|{debugger}"), [
            ("Program", exe.clone()),
            ("Runs", debugger.chars().take(300).collect()),
            ("How", "a \"Debugger\" setting (Image File Execution Options)".to_string()),
            ("_harmful", if backdoor { "yes" } else { "maybe" }.to_string()),
            ("_severity", if backdoor { "critical" } else { "error" }.to_string()),
        ]);
    }
    let spe = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SilentProcessExit";
    for exe in reg::subkeys(HKEY_LOCAL_MACHINE, spe) {
        let Some(monitor) = reg::string(HKEY_LOCAL_MACHINE, &format!(r"{spe}\{exe}"), "MonitorProcess") else { continue };
        if monitor.trim().is_empty() || monitor.to_ascii_lowercase().contains("werfault") {
            continue;
        }
        n += 1;
        emit(tx, "Syscura/Startup", 12, Level::Error, &format!("spe|{exe}|{monitor}"), [
            ("Program", exe.clone()),
            ("Runs", monitor.chars().take(300).collect()),
            ("How", "a \"silent process exit\" monitor".to_string()),
            ("_harmful", "maybe".to_string()),
            ("_severity", "error".to_string()),
        ]);
    }
    n
}

/// Is a Winlogon value what Windows sets? `Shell` = explorer.exe and
/// `Userinit` = ...\system32\userinit.exe, (both may carry a full path).
fn winlogon_value_ok(name: &str, value: &str) -> bool {
    let windir = windir().to_ascii_lowercase();
    let parts: Vec<String> = value
        .split(',')
        .map(|p| p.trim().trim_matches('"').to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let allowed: Vec<String> = match name {
        "Shell" => vec!["explorer.exe".into(), format!(r"{windir}\explorer.exe")],
        _ => vec!["userinit.exe".into(), r"system32\userinit.exe".into(), format!(r"{windir}\system32\userinit.exe")],
    };
    parts.len() == 1 && allowed.contains(&parts[0])
}

fn winlogon(tx: &Sender<Msg>) -> usize {
    let mut n = 0;
    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon";
    let mut places: Vec<(String, String, String)> = Vec::new();
    for name in ["Shell", "Userinit"] {
        if let Some(v) = reg::string(HKEY_LOCAL_MACHINE, key, name) {
            places.push((name.into(), v, "all users".into()));
        }
    }
    // Windows never sets these per user; malware does.
    for sid in reg::user_sids() {
        for name in ["Shell", "Userinit"] {
            if let Some(v) = reg::string(HKEY_USERS, &format!(r"{sid}\{key}"), name) {
                places.push((name.into(), v, "one user".into()));
            }
        }
    }
    for (name, value, scope) in places {
        // Some kiosk and shell-replacement setups are deliberate.
        if winlogon_value_ok(&name, &value) {
            continue;
        }
        n += 1;
        emit(tx, "Syscura/Startup", 13, Level::Error, &format!("winlogon|{name}|{value}|{scope}"), [
            ("Setting", name),
            ("Value", value.chars().take(300).collect()),
            ("Scope", scope),
        ]);
    }
    n
}

fn appinit(tx: &Sender<Msg>) -> usize {
    let mut n = 0;
    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Windows";
    for (view, bits) in [(VIEW64, "64-bit"), (KEY_WOW64_32KEY, "32-bit")] {
        let dlls = reg::string_view(HKEY_LOCAL_MACHINE, key, "AppInit_DLLs", view).unwrap_or_default();
        let path = if view == KEY_WOW64_32KEY { r"SOFTWARE\WOW6432Node\Microsoft\Windows NT\CurrentVersion\Windows" } else { key };
        let on = reg::dword(HKEY_LOCAL_MACHINE, path, "LoadAppInit_DLLs").unwrap_or(0) == 1;
        if dlls.trim().is_empty() || !on {
            continue;
        }
        n += 1;
        emit(tx, "Syscura/Startup", 14, Level::Warning, &format!("appinit|{bits}|{dlls}"), [
            ("Dlls", dlls.chars().take(300).collect()),
            ("Bits", bits.to_string()),
        ]);
    }
    n
}

/// WMI event subscriptions that run a command or a script: a fileless way to
/// survive restarts. Windows itself only uses harmless log consumers.
fn wmi_consumers(tx: &Sender<Msg>) -> usize {
    let ps = PathBuf::from(windir()).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let script = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; \
        Get-CimInstance -Namespace root/subscription -ClassName CommandLineEventConsumer -ErrorAction SilentlyContinue | \
          ForEach-Object { \"cmd`t$($_.Name)`t$($_.CommandLineTemplate) $($_.ExecutablePath)\" }; \
        Get-CimInstance -Namespace root/subscription -ClassName ActiveScriptEventConsumer -ErrorAction SilentlyContinue | \
          ForEach-Object { \"script`t$($_.Name)`t$($_.ScriptFileName) $(([string]$_.ScriptText).Substring(0, [Math]::Min(200, ([string]$_.ScriptText).Length)))\" }";
    let Ok(out) = Command::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return 0;
    };
    let mut n = 0;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.trim_start_matches('\u{feff}').splitn(3, '\t');
        let (Some(kind), Some(name), Some(what)) = (parts.next(), parts.next(), parts.next()) else { continue };
        // Left by Windows 7 / Server 2008 setups; harmless.
        if name == "BVTConsumer" {
            continue;
        }
        n += 1;
        emit(tx, "Syscura/Startup", 15, Level::Error, &format!("wmi|{kind}|{name}"), [
            ("Name", name.to_string()),
            ("Kind", if kind == "cmd" { "runs a command" } else { "runs a script" }.to_string()),
            ("Runs", what.trim().chars().take(300).collect()),
        ]);
    }
    n
}

/// Services (not drivers) whose program lives in a user, Temp or
/// ProgramData folder and is not validly signed.
fn user_folder_services(tx: &Sender<Msg>) -> usize {
    let base = r"SYSTEM\CurrentControlSet\Services";
    let mut candidates: Vec<(String, String)> = Vec::new();
    for name in reg::subkeys(HKEY_LOCAL_MACHINE, base) {
        let key = format!(r"{base}\{name}");
        // 0x10 / 0x20: own / shared process services (drivers are checked below).
        let kind = reg::dword(HKEY_LOCAL_MACHINE, &key, "Type").unwrap_or(0);
        if kind & 0x30 == 0 {
            continue;
        }
        let Some(image) = reg::string(HKEY_LOCAL_MACHINE, &key, "ImagePath") else { continue };
        let file = trust::image_file(&crate::watch::expand(&image));
        if trust::classify_location(&file) == Location::UserWritable && Path::new(&file).is_file() {
            candidates.push((name, file));
        }
    }
    let files: Vec<String> = candidates.iter().map(|c| c.1.clone()).collect::<HashSet<_>>().into_iter().collect();
    let mut sigs = HashMap::new();
    for chunk in files.chunks(40) {
        sigs.extend(trust::signatures(chunk));
    }
    let mut n = 0;
    for (name, file) in candidates {
        let why = match sigs.get(&file) {
            Some(Signature::Missing) => "not signed".to_string(),
            Some(Signature::Invalid(w)) => format!("signature NOT valid: {w}"),
            _ => continue,
        };
        n += 1;
        emit(tx, "Syscura/Startup", 16, Level::Error, &format!("svc|{name}|{file}"), [
            ("Service", name),
            ("Path", file),
            ("Signature", why),
        ]);
    }
    n
}

// ------------------------------------------------------------ network

/// Sites of security software and Windows Update. Malware blocks them in
/// the hosts file so the PC cannot update or clean itself.
const SECURITY_SITES: &[&str] = &[
    "windowsupdate", "update.microsoft", "download.microsoft", "wdcp.microsoft", "smartscreen", "defender", "mpsigs",
    "virustotal", "malwarebytes", "kaspersky", "eset.com", "bitdefender", "avast", "avg.com", "norton", "mcafee", "sophos",
    "trendmicro", "f-secure", "symantec", "clamav", "emsisoft", "hitmanpro", "safebrowsing",
];
/// Privacy tools block Microsoft's telemetry in the hosts file; that is fine.
const TELEMETRY_WORDS: &[&str] = &["telemetry", "vortex", "watson", "diagnostic", "data.microsoft", "settings-win", "ads", "analytics", "metrics", "oca."];
/// Sites people sign in to: a hosts entry sending them elsewhere is phishing.
const POPULAR_SITES: &[&str] = &[
    "google.", "gmail.", "youtube.", "facebook.", "instagram.", "whatsapp.", "paypal.", "apple.com", "icloud.", "amazon.",
    "microsoft.com", "live.com", "outlook.", "office.com", "yahoo.", "twitter.", "x.com", "netflix.", "binance.", "coinbase.",
    "bank", "steamcommunity.", "steampowered.", "discord.", "github.com", "linkedin.",
];

fn is_blackhole(ip: &str) -> bool {
    matches!(ip, "0.0.0.0" | "127.0.0.1" | "::1" | "::" | "0:0:0:0:0:0:0:0" | "::0") || ip.starts_with("127.")
}

fn is_private(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<std::net::IpAddr>() else { return false };
    match addr {
        std::net::IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified(),
        std::net::IpAddr::V6(v) => v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80,
    }
}

/// (event id, ip, host, why) for each worrying hosts line.
pub fn judge_hosts(text: &str) -> Vec<(u32, String, String, &'static str)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        let Some(ip) = parts.next() else { continue };
        for host in parts {
            let h = host.to_ascii_lowercase();
            let security = SECURITY_SITES.iter().any(|s| h.contains(s)) && !TELEMETRY_WORDS.iter().any(|w| h.contains(w));
            if security {
                out.push((20, ip.to_string(), h.clone(), "blocks or redirects a security or update site"));
                continue;
            }
            let popular = POPULAR_SITES.iter().any(|s| h.contains(s)) && !TELEMETRY_WORDS.iter().any(|w| h.contains(w));
            if popular && !is_blackhole(ip) && !is_private(ip) {
                out.push((21, ip.to_string(), h.clone(), "sends a popular site to another server"));
            }
        }
    }
    out
}

fn hosts_file(tx: &Sender<Msg>) -> usize {
    let path = PathBuf::from(windir()).join(r"System32\drivers\etc\hosts");
    let Ok(bytes) = std::fs::read(&path) else { return 0 };
    let found = judge_hosts(&String::from_utf8_lossy(&bytes));
    for (id, ip, host, why) in &found {
        emit(tx, "Syscura/Network", *id, Level::Error, &format!("hosts|{host}|{ip}"), [
            ("Host", host.clone()),
            ("Address", ip.clone()),
            ("Why", why.to_string()),
            ("Path", path.display().to_string()),
        ]);
    }
    found.len()
}

/// The proxy in a WinHTTP / Internet Settings "Connections" blob, if on.
fn proxy_from_blob(b: &[u8]) -> Option<String> {
    // version (4), counter (4), flags (4): 0x2 = a proxy server is set.
    let flags = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    if flags & 0x2 == 0 {
        return None;
    }
    let len = u32::from_le_bytes(b.get(12..16)?.try_into().ok()?) as usize;
    let proxy = String::from_utf8_lossy(b.get(16..16 + len.min(1024))?).trim().to_string();
    (!proxy.is_empty()).then_some(proxy)
}

fn proxies(tx: &Sender<Msg>) -> usize {
    let mut found: Vec<(String, String)> = Vec::new();
    let winhttp = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Internet Settings\Connections";
    if let Some(p) = reg::binary(HKEY_LOCAL_MACHINE, winhttp, "WinHttpSettings").and_then(|b| proxy_from_blob(&b)) {
        found.push(("Windows services (WinHTTP)".into(), p));
    }
    let inet = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    for sid in reg::user_sids() {
        let key = format!(r"{sid}\{inet}");
        if reg::dword(HKEY_USERS, &key, "ProxyEnable") == Some(1)
            && let Some(p) = reg::string(HKEY_USERS, &key, "ProxyServer")
            && !p.trim().is_empty()
        {
            found.push(("browsers and apps (one user)".into(), p));
        }
        if let Some(url) = reg::string(HKEY_USERS, &key, "AutoConfigURL").filter(|u| !u.trim().is_empty()) {
            found.push(("browsers and apps, from a setup script (one user)".into(), url));
        }
    }
    for (scope, proxy) in &found {
        // A proxy on this PC itself is usually a local tool (Fiddler, a VPN, an ad blocker).
        let local = ["127.0.0.1", "localhost", "[::1]"].iter().any(|l| proxy.to_ascii_lowercase().contains(l));
        emit(tx, "Syscura/Network", 22, Level::Warning, &format!("proxy|{scope}|{proxy}"), [
            ("Proxy", proxy.chars().take(300).collect()),
            ("Scope", scope.clone()),
            ("_harmful", if local { "no" } else { "maybe" }.to_string()),
            ("_severity", if local { "info" } else { "warning" }.to_string()),
        ]);
    }
    found.len()
}

/// Well-known public DNS services. Anything else typed in by hand is worth
/// a look: changing DNS is how some malware sends you to fake sites.
const KNOWN_DNS: &[&str] = &[
    "1.1.1.", "1.0.0.", "8.8.8.8", "8.8.4.4", "9.9.9.", "149.112.112.", "208.67.222.", "208.67.220.", "94.140.14.", "94.140.15.",
    "76.76.2.", "76.76.10.", "45.90.28.", "45.90.30.", "185.228.168.", "185.228.169.", "4.2.2.", "64.6.64.6", "64.6.65.6",
    "77.88.8.", "156.154.70.", "156.154.71.", "8.26.56.26", "8.20.247.20", "84.200.69.80", "84.200.70.40", "194.242.2.",
    "2606:4700:4700::", "2001:4860:4860::", "2620:fe::", "2620:119:35::", "2620:119:53::", "2a10:50c0::", "2a07:a8c0::",
];

pub fn unknown_dns(servers: &str) -> Vec<String> {
    servers
        .split([',', ' ', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.parse::<std::net::IpAddr>().is_ok())
        .filter(|s| !is_private(s) && !KNOWN_DNS.iter().any(|k| s.to_ascii_lowercase().starts_with(k)))
        .map(str::to_string)
        .collect()
}

fn dns_servers(tx: &Sender<Msg>) -> usize {
    let mut n = 0;
    for (base, family) in [
        (r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces", "IPv4"),
        (r"SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters\Interfaces", "IPv6"),
    ] {
        for iface in reg::subkeys(HKEY_LOCAL_MACHINE, base) {
            // Only addresses typed in (NameServer); the router's come as DhcpNameServer.
            let Some(servers) = reg::string(HKEY_LOCAL_MACHINE, &format!(r"{base}\{iface}"), "NameServer") else { continue };
            for server in unknown_dns(&servers) {
                n += 1;
                emit(tx, "Syscura/Network", 23, Level::Warning, &format!("dns|{server}"), [
                    ("Server", server),
                    ("Family", family.to_string()),
                ]);
            }
        }
    }
    n
}

fn forced_extensions(tx: &Sender<Msg>) -> usize {
    let browsers = [
        (r"SOFTWARE\Policies\Google\Chrome\ExtensionInstallForcelist", "Chrome"),
        (r"SOFTWARE\Policies\Microsoft\Edge\ExtensionInstallForcelist", "Edge"),
        (r"SOFTWARE\Policies\BraveSoftware\Brave\ExtensionInstallForcelist", "Brave"),
    ];
    let mut found: Vec<(String, String, String)> = Vec::new();
    for (key, browser) in browsers {
        for (_, v) in reg::values(HKEY_LOCAL_MACHINE, key, VIEW64) {
            found.push((browser.into(), v, "all users".into()));
        }
        for sid in reg::user_sids() {
            for (_, v) in reg::values(HKEY_USERS, &format!(r"{sid}\{key}"), VIEW64) {
                found.push((browser.into(), v, "one user".into()));
            }
        }
    }
    for (browser, value, scope) in &found {
        let id = value.split(';').next().unwrap_or(value).trim().to_string();
        let source = value.split_once(';').map(|x| x.1.trim().to_string()).unwrap_or_default();
        emit(tx, "Syscura/Network", 24, Level::Warning, &format!("ext|{browser}|{value}|{scope}"), [
            ("Browser", browser.clone()),
            ("Extension", id),
            ("Source", if source.is_empty() { "the browser's web store".to_string() } else { source }),
            ("Scope", scope.clone()),
        ]);
    }
    found.len()
}

// ------------------------------------------------------------ drivers

/// The LOLDrivers list, shipped with Syscura (no download needed).
const DRIVER_LIST: &str = include_str!("../../../kb/vulnerable-drivers.txt");

/// SHA-256 prefix -> (malicious, known file name).
fn driver_list() -> HashMap<&'static str, (bool, &'static str)> {
    DRIVER_LIST
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut p = l.split(' ');
            Some((p.next()?, (p.next()? == "m", p.next().unwrap_or("?"))))
        })
        .collect()
}

/// Hash of every driver file, keyed by path, remembered with its size and
/// change time so unchanged drivers are not read again.
#[derive(Default)]
pub struct DriverCache(Mutex<HashMap<String, (u64, u128, String)>>);

impl DriverCache {
    fn sha256(&self, path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
        let key = path.to_string_lossy().to_ascii_lowercase();
        if let Some((size, when, hash)) = lock(&self.0).get(&key)
            && *size == meta.len()
            && *when == stamp
        {
            return Some(hash.clone());
        }
        if meta.len() > 64 * 1024 * 1024 {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        let hash: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        lock(&self.0).insert(key, (meta.len(), stamp, hash.clone()));
        Some(hash)
    }
}

fn vulnerable_drivers(tx: &Sender<Msg>, cache: &DriverCache) -> usize {
    let list = driver_list();
    let base = r"SYSTEM\CurrentControlSet\Services";
    let windir = windir();
    let mut n = 0;
    let mut seen = HashSet::new();
    for name in reg::subkeys(HKEY_LOCAL_MACHINE, base) {
        let key = format!(r"{base}\{name}");
        // 0x1 kernel driver, 0x2 file system driver.
        if reg::dword(HKEY_LOCAL_MACHINE, &key, "Type").unwrap_or(0) & 0x3 == 0 {
            continue;
        }
        let file = match reg::string(HKEY_LOCAL_MACHINE, &key, "ImagePath") {
            Some(image) => trust::image_file(&crate::watch::expand(&image)),
            None => format!(r"{windir}\System32\drivers\{name}.sys"),
        };
        if !seen.insert(file.to_ascii_lowercase()) {
            continue;
        }
        let Some(hash) = cache.sha256(Path::new(&file)) else { continue };
        let Some(&(malicious, known_as)) = list.get(&hash[..16]) else { continue };
        let start = reg::dword(HKEY_LOCAL_MACHINE, &key, "Start").unwrap_or(3);
        n += 1;
        emit(tx, "Syscura/Drivers", if malicious { 31 } else { 30 }, if malicious { Level::Critical } else { Level::Warning }, &format!("drv|{file}|{hash}"), [
            ("Service", name.clone()),
            ("Path", file.clone()),
            ("KnownAs", known_as.to_string()),
            ("Sha256", hash.clone()),
            ("Loads", match start {
                0 | 1 => "when Windows starts",
                2 => "automatically",
                4 => "never (disabled)",
                _ => "when a program asks for it",
            }
            .to_string()),
        ]);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_entries_are_judged() {
        let hosts = "\
# comment 1.2.3.4 google.com
127.0.0.1 localhost
0.0.0.0 vortex.data.microsoft.com telemetry.microsoft.com
0.0.0.0 doubleclick.net
127.0.0.1 www.malwarebytes.com
192.168.1.20 nas.local
203.0.113.9 www.paypal.com
0.0.0.0 www.facebook.com
";
        let found = judge_hosts(hosts);
        let hosts: Vec<&str> = found.iter().map(|f| f.2.as_str()).collect();
        assert_eq!(hosts, vec!["www.malwarebytes.com", "www.paypal.com"], "{found:?}");
        assert_eq!(found[0].0, 20);
        assert_eq!(found[1].0, 21);
    }

    #[test]
    fn dns_servers_are_judged() {
        assert!(unknown_dns("1.1.1.1,8.8.8.8 192.168.1.1").is_empty());
        assert_eq!(unknown_dns("203.0.113.53"), vec!["203.0.113.53"]);
        assert!(unknown_dns("2606:4700:4700::1111 fe80::1").is_empty());
        assert!(unknown_dns("not-an-ip").is_empty());
    }

    #[test]
    fn winlogon_defaults_are_accepted() {
        let windir = windir();
        assert!(winlogon_value_ok("Shell", "explorer.exe"));
        assert!(winlogon_value_ok("Userinit", &format!(r"{windir}\system32\userinit.exe,")));
        assert!(!winlogon_value_ok("Userinit", &format!(r"{windir}\system32\userinit.exe,C:\Users\x\evil.exe")));
        assert!(!winlogon_value_ok("Shell", r"explorer.exe, C:\ProgramData\x.exe"));
    }

    #[test]
    fn proxy_blob() {
        let mut b = vec![0x46, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0];
        let p = b"10.0.0.1:8080";
        b.extend((p.len() as u32).to_le_bytes());
        b.extend(p);
        assert_eq!(proxy_from_blob(&b).as_deref(), Some("10.0.0.1:8080"));
        b[8] = 1; // direct connection
        assert_eq!(proxy_from_blob(&b), None);
        assert_eq!(proxy_from_blob(&[1, 2]), None);
    }

    #[test]
    fn driver_list_is_loaded() {
        let list = driver_list();
        assert!(list.len() > 1000, "the LOLDrivers list ships with Syscura");
        assert!(list.values().any(|(m, _)| *m), "it includes malicious drivers");
        assert!(list.keys().all(|k| k.len() == 16));
    }

    #[test]
    fn driver_hashes_are_cached() {
        let cache = DriverCache::default();
        let me = std::env::current_exe().unwrap();
        let a = cache.sha256(&me).unwrap();
        assert_eq!(a.len(), 64);
        assert_eq!(cache.sha256(&me).unwrap(), a);
    }
}

/// Property tests for the parsers that read files and settings other
/// programs (and malware) control.
#[cfg(test)]
mod fuzz {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn hosts_and_dns_never_panic(text in ".{0,500}") {
            let _ = judge_hosts(&text);
            let _ = unknown_dns(&text);
        }

        #[test]
        fn proxy_blobs_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
            let _ = proxy_from_blob(&bytes);
        }

        #[test]
        fn paths_never_panic(s in ".{0,300}") {
            let _ = crate::trust::image_file(&s);
            let _ = crate::trust::classify_location(&s);
            let _ = crate::heal::affected_files(&s);
            let _ = winlogon_value_ok("Shell", &s);
            let _ = winlogon_value_ok("Userinit", &s);
        }
    }
}
