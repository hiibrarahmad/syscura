//! Outdated programs, from Windows' own package manager (winget). Old
//! browsers, PDF readers and runtimes are how most malware gets in, so the
//! Security page lists what has a newer version and updates it on request.
//!
//! winget only prints a text table, so it is read by column position (the
//! header's words differ by language; their positions do not).

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use serde::Serialize;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppUpdate {
    pub name: String,
    pub id: String,
    pub version: String,
    pub available: String,
}

/// Programs with a newer version available.
pub fn outdated() -> Result<Vec<AppUpdate>, String> {
    let out = Command::new("winget")
        .args(["upgrade", "--include-unknown", "--accept-source-agreements", "--disable-interactivity"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|_| "Windows' package manager (winget) is not installed. Install \"App Installer\" from the Microsoft Store.".to_string())?;
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Reads winget's table. Lines may carry progress-spinner leftovers before
/// a carriage return; only the text after the last one counts.
pub fn parse(text: &str) -> Vec<AppUpdate> {
    let lines: Vec<String> = text.lines().map(|l| l.rsplit('\r').next().unwrap_or(l).trim_end().to_string()).collect();
    let Some(sep) = lines.iter().position(|l| l.len() > 20 && l.chars().all(|c| c == '-')) else { return Vec::new() };
    let Some(header) = sep.checked_sub(1).map(|i| lines[i].clone()) else { return Vec::new() };
    let hchars: Vec<char> = header.chars().collect();
    // Column starts: a non-space after a space (or the first character).
    let starts: Vec<usize> = (0..hchars.len()).filter(|&i| hchars[i] != ' ' && (i == 0 || hchars[i - 1] == ' ')).collect();
    if starts.len() < 4 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in &lines[sep + 1..] {
        let chars: Vec<char> = line.chars().collect();
        // The table ends with a blank line, then a summary.
        if chars.len() < starts[3] {
            break;
        }
        let col = |n: usize| -> String {
            let from = starts[n].min(chars.len());
            let to = starts.get(n + 1).copied().unwrap_or(chars.len()).min(chars.len());
            chars[from..to].iter().collect::<String>().trim().to_string()
        };
        let (name, id, version, available) = (col(0), col(1), col(2), col(3));
        if id.is_empty() || available.is_empty() || !valid_id(&id) {
            continue;
        }
        out.push(AppUpdate { name: name.trim_end_matches('…').to_string(), id, version, available });
    }
    out
}

/// Package ids are like "Mozilla.Firefox" or "Microsoft.VCRedist.2015+.x64".
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// Updates the programs in a visible window, so the person sees winget's
/// progress and any installer that asks a question.
pub fn update(ids: &[String]) -> Result<String, String> {
    if ids.is_empty() {
        return Err("Nothing to update.".into());
    }
    if let Some(bad) = ids.iter().find(|id| !valid_id(id)) {
        return Err(format!("\"{bad}\" is not a package id."));
    }
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let cmd = std::path::Path::new(&windir).join(r"System32\cmd.exe");
    // One winget call per program; ids were checked above, so they cannot
    // carry shell characters.
    let script = ids
        .iter()
        .map(|id| format!("winget upgrade --id {id} --exact --accept-source-agreements --accept-package-agreements"))
        .collect::<Vec<_>>()
        .join(" & ");
    Command::new(cmd)
        .args(["/c", "start", "Syscura: updating programs", "cmd", "/c", &format!("{script} & echo. & echo Done. You can close this window. & pause")])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Could not start winget: {e}"))?;
    Ok(if ids.len() == 1 { "Updating in a new window…".into() } else { format!("Updating {} programs in a new window…", ids.len()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_winget_table() {
        let row = |a: &str, b: &str, c: &str, d: &str| format!("{a:<35}{b:<30}{c:<15}{d:<15}winget\n");
        let header = format!("{:<35}{:<30}{:<15}{:<15}Source\n", "Name", "Id", "Version", "Available");
        let text = format!(
            "   - \r   \\ \r{header}{}\n{}{}{}\n3 upgrades available.\n",
            "-".repeat(101),
            row("Mozilla Firefox (x64 en-US)", "Mozilla.Firefox", "130.0", "131.0.2"),
            row("Microsoft Visual C++ 2015-2022 Re…", "Microsoft.VCRedist.2015+.x64", "14.38.33135.0", "14.40.33810.0"),
            row("7-Zip 23.01 (x64)", "7zip.7zip", "23.01", "24.08"),
        );
        let apps = parse(&text);
        assert_eq!(apps.len(), 3, "{apps:?}");
        assert_eq!(apps[0], AppUpdate { name: "Mozilla Firefox (x64 en-US)".into(), id: "Mozilla.Firefox".into(), version: "130.0".into(), available: "131.0.2".into() });
        assert_eq!(apps[2].id, "7zip.7zip");
    }

    #[test]
    fn localised_headers_and_empty_lists() {
        let de = "Name     Id          Version Verfügbar Quelle\n--------------------------------------------\nFoo App  Foo.App     1.0     2.0       winget\n";
        assert_eq!(parse(de)[0].available, "2.0");
        assert!(parse("No installed package found matching input criteria.").is_empty());
    }

    #[test]
    fn ids_are_checked() {
        assert!(valid_id("Microsoft.VCRedist.2015+.x64"));
        assert!(!valid_id("x & del C:"));
        assert!(!valid_id("a\"b"));
        assert!(update(&["ok.id".into(), "bad id".into()]).is_err());
    }
}
