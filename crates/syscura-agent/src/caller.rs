//! Who is talking to the agent, and where Syscura's own programs live.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

/// The programs Syscura ships. Only these, in the agent's own folder, may
/// change things through the pipe without being elevated.
pub const OWN_PROGRAMS: &[&str] = &["syscura.exe", "syscura-cli.exe", "syscura-agent.exe"];

pub struct Caller {
    pub path: PathBuf,
    pub elevated: bool,
}

/// Program file and elevation of a running process.
pub fn inspect(pid: u32) -> Option<Caller> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let path = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
            .ok()
            .map(|_| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
        let elevated = token_elevated(h);
        let _ = CloseHandle(h);
        Some(Caller { path: path?, elevated })
    }
}

unsafe fn token_elevated(process: HANDLE) -> bool {
    let mut token = HANDLE::default();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut size = 0u32;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
    }
    .is_ok();
    unsafe {
        let _ = CloseHandle(token);
    }
    ok && elevation.TokenIsElevated != 0
}

/// Lower-case long path without the `\\?\` prefix, for comparing folders.
fn normal(p: &Path) -> String {
    let full = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = full.to_string_lossy().to_ascii_lowercase();
    s.strip_prefix(r"\\?\").unwrap_or(&s).trim_end_matches('\\').to_string()
}

/// Is `program` one of Syscura's own programs, in the same folder as the
/// running agent?
pub fn is_own_program(program: &Path) -> bool {
    let Ok(me) = std::env::current_exe() else { return false };
    let name = program.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    OWN_PROGRAMS.contains(&name.as_str())
        && same_folder(program, &me)
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (a.parent(), b.parent()) {
        (Some(x), Some(y)) => normal(x) == normal(y),
        _ => false,
    }
}

/// Folders only administrators can change: Program Files and Windows.
pub fn admin_only_folder(dir: &Path) -> bool {
    let d = normal(dir);
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "SystemRoot"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|root| normal(Path::new(&root)))
        .any(|root| !root.is_empty() && (d == root || d.starts_with(&format!("{root}\\"))))
}

/// Where the installed copy lives: `%ProgramFiles%\Syscura`.
pub fn install_dir() -> PathBuf {
    let pf = std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .unwrap_or_else(|| r"C:\Program Files".into());
    PathBuf::from(pf).join("Syscura")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_files_is_admin_only_and_users_is_not() {
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        assert!(admin_only_folder(&Path::new(&pf).join("Syscura")));
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        assert!(admin_only_folder(&Path::new(&windir).join("System32")));
        assert!(!admin_only_folder(Path::new(r"C:\Users\someone\Downloads\Syscura")));
        // A look-alike folder name is not inside Program Files.
        assert!(!admin_only_folder(Path::new(&format!("{pf} Evil\\Syscura"))));
    }

    #[test]
    fn own_programs_must_sit_next_to_the_agent() {
        let me = std::env::current_exe().unwrap();
        assert!(!is_own_program(&me.with_file_name("notepad.exe")));
        assert!(!is_own_program(Path::new(r"C:\Users\x\Downloads\Syscura.exe")));
        assert!(is_own_program(&me.with_file_name("Syscura.exe")));
    }

    #[test]
    fn this_process_can_be_inspected() {
        let me = inspect(std::process::id()).expect("own process");
        assert!(me.path.is_absolute());
    }
}
