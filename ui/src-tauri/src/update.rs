//! Updates from GitHub Releases: checks the latest release, downloads its
//! Setup, verifies it, and runs it.
//!
//! Verification has two steps. The release's SHA256SUMS.txt must carry a
//! valid signature (SHA256SUMS.txt.sig) from Syscura's release key, whose
//! public half is built into this app; then the Setup must match its
//! checksum in that list. Someone who took over the GitHub account could
//! replace both the Setup and the checksum list, but not sign them.
//! The Setup replaces the installed copy in place; history, verdicts,
//! settings and the AI key stay where they are (ProgramData, the app's
//! local data, Credential Manager), and the agent upgrades its database on
//! first start.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

const REPO: &str = "hiibrarahmad/syscura";
const MAX_SETUP: u64 = 200 * 1024 * 1024;
/// Public half of Syscura's release signing key (minisign). The private
/// half only exists in the release build's secrets and the author's backup.
const RELEASE_KEY: &str = "RWTWuJ/EqjDGLnxQR6YS2mZWGLOdYy6GBtLKw/lSelP0dnNqYs1z+2CS";

#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    /// Release notes (Markdown), shortened.
    pub notes: String,
    pub page: String,
    pub published: String,
    #[serde(skip)]
    setup_url: String,
    #[serde(skip)]
    setup_name: String,
    #[serde(skip)]
    sums_url: String,
    #[serde(skip)]
    sig_url: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn user_agent() -> String {
    format!("Syscura/{}", env!("CARGO_PKG_VERSION"))
}

/// Asks GitHub for the latest release.
pub fn check() -> Result<UpdateInfo, String> {
    let mut resp = agent()
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("User-Agent", &user_agent())
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().with_config().limit(4 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())?;
    if status == 403 || status == 429 {
        return Err("GitHub is busy right now. Syscura will check again later.".into());
    }
    if status != 200 {
        return Err(format!("GitHub answered {status}."));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let asset = |pred: &dyn Fn(&str) -> bool| {
        assets.iter().find(|a| a["name"].as_str().is_some_and(pred)).map(|a| {
            (a["name"].as_str().unwrap_or("").to_string(), a["browser_download_url"].as_str().unwrap_or("").to_string())
        })
    };
    let arm = native_arm64();
    let (setup_name, setup_url) = asset(&|n| is_setup_for(n, arm))
        // An ARM PC runs the x64 Setup too (emulated) if there is no ARM one.
        .or_else(|| if arm { asset(&|n| is_setup_for(n, false)) } else { None })
        .unwrap_or_default();
    let (_, sums_url) = asset(&|n| n == "SHA256SUMS.txt").unwrap_or_default();
    let (_, sig_url) = asset(&|n| n == "SHA256SUMS.txt.sig").unwrap_or_default();
    let current = env!("CARGO_PKG_VERSION").to_string();
    let notes: String = v["body"].as_str().unwrap_or("").chars().take(4000).collect();
    Ok(UpdateInfo {
        available: !setup_url.is_empty() && is_newer(&tag, &current),
        current,
        latest: tag,
        notes,
        page: v["html_url"].as_str().unwrap_or("").to_string(),
        published: v["published_at"].as_str().unwrap_or("").chars().take(10).collect(),
        setup_url,
        setup_name,
        sums_url,
        sig_url,
    })
}

/// Setup names: `Syscura-<v>-setup.exe` (x64) and `Syscura-<v>-setup-arm64.exe`.
/// The ARM one deliberately does not end in "-setup.exe", so versions
/// before 1.1 (which take the first such file) never pick it.
fn is_setup_for(name: &str, arm64: bool) -> bool {
    let n = name.to_ascii_lowercase();
    if arm64 { n.ends_with("-setup-arm64.exe") } else { n.ends_with("-setup.exe") }
}

/// True on an ARM64 PC, even when this copy is the x64 build running
/// under emulation (so the next update brings the native build).
pub fn native_arm64() -> bool {
    use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE;
    use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
    const ARM64: u16 = 0xAA64;
    let (mut process, mut native) = (IMAGE_FILE_MACHINE(0), IMAGE_FILE_MACHINE(0));
    match unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, Some(&mut native)) } {
        Ok(()) => native.0 == ARM64,
        Err(_) => cfg!(target_arch = "aarch64"),
    }
}

/// Checks the release key's signature on the checksum list. `sig_file_b64`
/// is the .sig file as `tauri signer sign` writes it (base64 of a minisign
/// signature file).
fn verify_signed(sums: &str, sig_file_b64: &str, key: &str) -> Result<(), String> {
    use base64::Engine;
    let refused = "The update's checksum list is not signed by Syscura's release key, so it was not installed.";
    let sig_text = base64::engine::general_purpose::STANDARD
        .decode(sig_file_b64.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or(refused)?;
    let pk = minisign_verify::PublicKey::from_base64(key).map_err(|_| refused)?;
    let sig = minisign_verify::Signature::decode(&sig_text).map_err(|_| refused)?;
    pk.verify(sums.as_bytes(), &sig, false).map_err(|_| refused.to_string())
}

/// Downloads and verifies the Setup, then starts it (one admin prompt).
/// The installer shows its progress, replaces this copy and opens
/// Syscura again; the caller should exit right after.
pub fn install(info: &UpdateInfo) -> Result<(), String> {
    if !info.available {
        return Err("Syscura is already up to date.".into());
    }
    let file = download_verified(info)?;
    launch(&file)
}

/// Downloads the release's Setup to the temp folder and checks it against
/// the release's SHA256SUMS.txt. Nothing is written unless it matches.
fn download_verified(info: &UpdateInfo) -> Result<PathBuf, String> {
    if !info.setup_url.starts_with(&format!("https://github.com/{REPO}/releases/download/")) {
        return Err("The update does not come from Syscura's GitHub releases; not installing it.".into());
    }
    if info.sig_url.is_empty() {
        return Err("This release has no signature for its checksums, so Syscura did not install it. Download it from the release page instead.".into());
    }
    let sums = download_text(&info.sums_url)?;
    let sig = download_text(&info.sig_url)?;
    verify_signed(&sums, &sig, RELEASE_KEY)?;
    let expected = sums
        .lines()
        .find_map(|l| {
            let (hash, name) = l.trim().split_once(char::is_whitespace)?;
            (name.trim().trim_start_matches('*') == info.setup_name).then(|| hash.to_ascii_lowercase())
        })
        .ok_or("The release has no checksum for its Setup; not installing it.")?;

    let dir = std::env::temp_dir().join("Syscura-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file: PathBuf = dir.join(&info.setup_name);
    let mut resp = agent()
        .get(&info.setup_url)
        .header("User-Agent", &user_agent())
        .call()
        .map_err(|e| format!("Could not download the update: {e}"))?;
    if resp.status().as_u16() != 200 {
        return Err(format!("Could not download the update (GitHub answered {}).", resp.status().as_u16()));
    }
    let mut bytes = Vec::new();
    resp.body_mut()
        .with_config()
        .limit(MAX_SETUP)
        .reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("The download was interrupted: {e}"))?;
    let actual = hex(&Sha256::digest(&bytes));
    if actual != expected {
        return Err("The downloaded update does not match its published checksum, so it was not installed. Try again later.".into());
    }
    std::fs::write(&file, &bytes).map_err(|e| e.to_string())?;
    Ok(file)
}

fn launch(file: &std::path::Path) -> Result<(), String> {
    // /P: passive install with a progress bar. /SYSCURA_RELAUNCH: our
    // installer hook opens Syscura again when it is done.
    use std::os::windows::process::CommandExt;
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = PathBuf::from(windir).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    std::process::Command::new(ps)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Process -FilePath $env:SYSCURA_SETUP -ArgumentList '/P','/SYSCURA_RELAUNCH' -Verb RunAs",
        ])
        .env("SYSCURA_SETUP", file)
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|e| format!("Could not start the installer: {e}"))?;
    Ok(())
}

fn download_text(url: &str) -> Result<String, String> {
    let mut resp = agent()
        .get(url)
        .header("User-Agent", &user_agent())
        .call()
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    if resp.status().as_u16() != 200 {
        return Err(format!("GitHub answered {}.", resp.status().as_u16()));
    }
    resp.body_mut().with_config().limit(1024 * 1024).read_to_string().map_err(|e| e.to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// (major, minor, patch, pre-release) of "1.2.3" or "1.2.3-beta.4".
fn parse(v: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (v, None),
    };
    let mut n = core.split('.').map(|x| x.parse::<u64>());
    Some((n.next()?.ok()?, n.next().unwrap_or(Ok(0)).ok()?, n.next().unwrap_or(Ok(0)).ok()?, pre))
}

/// Semantic-version comparison: a release is newer than its pre-releases,
/// and pre-release parts compare numerically when they are numbers.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (Some(a), Some(b)) = (parse(candidate), parse(current)) else { return false };
    if (a.0, a.1, a.2) != (b.0, b.1, b.2) {
        return (a.0, a.1, a.2) > (b.0, b.1, b.2);
    }
    match (&a.3, &b.3) {
        (None, Some(_)) => true,
        (Some(_), None) | (None, None) => false,
        (Some(x), Some(y)) => {
            let xs: Vec<&str> = x.split('.').collect();
            let ys: Vec<&str> = y.split('.').collect();
            for (p, q) in xs.iter().zip(ys.iter()) {
                let ord = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    _ => p.cmp(q),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord == std::cmp::Ordering::Greater;
                }
            }
            xs.len() > ys.len()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RELEASE_KEY, is_newer, is_setup_for, verify_signed};

    #[test]
    fn signatures_are_checked() {
        // "test\n" signed with the release key.
        let sig = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUV3VKL0VxakRHTG1GRlhhYjY4eU00NXFlOE5LWTJMNVJwYnd4ZWc0RmxwcHNtVlFSbjArTk5iaytCZURKWEl6Z2dkOFdhS3kyU1FKb2xRTEJBRy9ySkw3c0NHcnR0Vmc4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNDU1NzE3CWZpbGU6dC50eHQKWDNiTTZkTFBkQ2RjRUpBOHh6NExvNWVQTTRBZkpKaHJTMlUwK05oN3dYdnpmSzhyVGsyUWVMcklVSC9hNFg5YThoa05sd2ovYzRUOHlVY2g2eCs2Qnc9PQo=";
        assert!(verify_signed("test\n", sig, RELEASE_KEY).is_ok(), "the signed text verifies");
        assert!(verify_signed("tesT\n", sig, RELEASE_KEY).is_err(), "a changed list is refused");
        assert!(verify_signed("test\n", "bm90IGEgc2lnbmF0dXJl", RELEASE_KEY).is_err(), "garbage is refused");
        assert!(
            verify_signed("test\n", sig, "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3").is_err(),
            "another key is refused"
        );
    }

    /// Checks a locally built release before it is published:
    /// `cargo test -p syscura-ui -- --ignored verifies_dist`.
    #[test]
    #[ignore]
    fn verifies_dist() {
        let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist");
        let sums = std::fs::read_to_string(dist.join("SHA256SUMS.txt")).expect("dist/SHA256SUMS.txt");
        let sig = std::fs::read_to_string(dist.join("SHA256SUMS.txt.sig")).expect("dist/SHA256SUMS.txt.sig");
        verify_signed(&sums, &sig, RELEASE_KEY).expect("signed with the release key");
    }

    #[test]
    fn setup_names_per_architecture() {
        assert!(is_setup_for("Syscura-1.1.0-setup.exe", false));
        assert!(!is_setup_for("Syscura-1.1.0-setup-arm64.exe", false), "x64 PCs never get the ARM build");
        assert!(is_setup_for("Syscura-1.1.0-setup-arm64.exe", true));
        assert!(!is_setup_for("Syscura-1.1.0-windows-x64.zip", false));
    }

    /// Talks to GitHub: `cargo test -p syscura-ui -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_and_verifies_the_latest_release() {
        let mut info = super::check().expect("GitHub answers");
        println!("latest {} (current {}), setup {}", info.latest, info.current, info.setup_name);
        assert!(!info.setup_url.is_empty(), "the latest release has a Setup");
        info.available = true;
        let file = super::download_verified(&info).expect("download matches its checksum");
        assert!(std::fs::metadata(&file).unwrap().len() > 1_000_000);
        // A tampered checksum list must be refused.
        info.setup_name = "something-else-setup.exe".into();
        assert!(super::download_verified(&info).is_err());
    }

    #[test]
    fn versions_compare_like_semver() {
        assert!(is_newer("1.0.0", "0.1.0-beta.3"));
        assert!(is_newer("1.0.0", "1.0.0-beta.3"), "a release beats its betas");
        assert!(!is_newer("1.0.0-beta.3", "1.0.0"));
        assert!(is_newer("0.1.0-beta.10", "0.1.0-beta.9"), "numeric, not text, comparison");
        assert!(is_newer("v1.2.0", "1.1.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.1"));
        assert!(!is_newer("garbage", "1.0.0"));
    }
}
