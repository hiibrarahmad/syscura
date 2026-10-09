//! Small, read-mostly helpers for the Windows registry.

use windows::Win32::System::Registry::{
    HKEY, KEY_READ, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_SAM_FLAGS,
    REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};
use windows::core::{HSTRING, PWSTR};

pub use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_WOW64_32KEY};
pub const VIEW64: REG_SAM_FLAGS = KEY_WOW64_64KEY;

pub fn open(root: HKEY, path: &str, view: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    let r = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), Some(0), KEY_READ | view, &mut key) };
    r.is_ok().then_some(key)
}

fn close(key: HKEY) {
    unsafe {
        let _ = RegCloseKey(key);
    }
}

pub fn subkeys(root: HKEY, path: &str) -> Vec<String> {
    subkeys_view(root, path, VIEW64)
}

pub fn subkeys_view(root: HKEY, path: &str, view: REG_SAM_FLAGS) -> Vec<String> {
    let mut out = Vec::new();
    let key = if path.is_empty() {
        root
    } else {
        match open(root, path, view) {
            Some(k) => k,
            None => return out,
        }
    };
    for i in 0..10_000 {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let r = unsafe { RegEnumKeyExW(key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
        if r.is_err() {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    if !path.is_empty() {
        close(key);
    }
    out
}

fn words(data: &[u8]) -> String {
    let w: Vec<u16> = data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    String::from_utf16_lossy(&w)
}

/// String values of a key: (name, data). Multi-strings are joined with spaces.
pub fn values(root: HKEY, path: &str, view: REG_SAM_FLAGS) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(key) = open(root, path, view) else { return out };
    for i in 0..500 {
        let mut name = [0u16; 512];
        let mut name_len = name.len() as u32;
        let mut data = vec![0u8; 8192];
        let mut data_len = data.len() as u32;
        let mut kind = 0u32;
        let r = unsafe {
            RegEnumValueW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, Some(&mut kind as *mut u32), Some(data.as_mut_ptr()), Some(&mut data_len))
        };
        if r.is_err() {
            break;
        }
        let k = REG_VALUE_TYPE(kind);
        if k != REG_SZ && k != REG_EXPAND_SZ && k != REG_MULTI_SZ {
            continue;
        }
        let text = words(&data[..data_len as usize]);
        let text = text.split('\0').filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ").trim().to_string();
        if !text.is_empty() {
            out.push((String::from_utf16_lossy(&name[..name_len as usize]), text));
        }
    }
    close(key);
    out
}

fn raw(root: HKEY, path: &str, value: &str, view: REG_SAM_FLAGS) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
    let key = open(root, path, view)?;
    let name = HSTRING::from(value);
    let mut kind = REG_VALUE_TYPE(0);
    let mut len = 0u32;
    let first = unsafe { RegQueryValueExW(key, &name, None, Some(&mut kind), None, Some(&mut len)) };
    if first.is_err() || len > 1 << 20 {
        close(key);
        return None;
    }
    let mut data = vec![0u8; len as usize];
    let r = unsafe { RegQueryValueExW(key, &name, None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut len)) };
    close(key);
    r.is_ok().then(|| {
        data.truncate(len as usize);
        (kind, data)
    })
}

pub fn dword(root: HKEY, path: &str, value: &str) -> Option<u32> {
    match raw(root, path, value, VIEW64)? {
        (k, d) if k == REG_DWORD && d.len() >= 4 => Some(u32::from_le_bytes([d[0], d[1], d[2], d[3]])),
        _ => None,
    }
}

pub fn string(root: HKEY, path: &str, value: &str) -> Option<String> {
    string_view(root, path, value, VIEW64)
}

pub fn string_view(root: HKEY, path: &str, value: &str, view: REG_SAM_FLAGS) -> Option<String> {
    match raw(root, path, value, view)? {
        (k, d) if k == REG_SZ || k == REG_EXPAND_SZ || k == REG_MULTI_SZ => {
            Some(words(&d).split('\0').filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ").trim().to_string())
        }
        _ => None,
    }
}

pub fn binary(root: HKEY, path: &str, value: &str) -> Option<Vec<u8>> {
    match raw(root, path, value, VIEW64)? {
        (k, d) if k == REG_BINARY => Some(d),
        _ => None,
    }
}

pub fn exists(root: HKEY, path: &str) -> bool {
    match open(root, path, VIEW64) {
        Some(k) => {
            close(k);
            true
        }
        None => false,
    }
}

/// Sets (or, with `None`, removes) a DWORD under HKLM. Used by actions.
pub fn set_dword_hklm(path: &str, value: &str, data: Option<u32>) -> Result<(), String> {
    let mut key = HKEY::default();
    let r = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &HSTRING::from(path), Some(0), KEY_SET_VALUE | VIEW64, &mut key) };
    if r.is_err() {
        return Err(format!("cannot open HKLM\\{path}: {}", windows::core::Error::from(r.to_hresult())));
    }
    let name = HSTRING::from(value);
    let r = match data {
        Some(v) => unsafe { RegSetValueExW(key, &name, None, REG_DWORD, Some(&v.to_le_bytes())) },
        None => unsafe { RegDeleteValueW(key, &name) },
    };
    close(key);
    if r.is_err() {
        return Err(format!("cannot change {value}: {}", windows::core::Error::from(r.to_hresult())));
    }
    Ok(())
}

/// Signed-in users' registry hives (S-1-5-21-...), as HKEY_USERS sub-keys.
pub fn user_sids() -> Vec<String> {
    subkeys(HKEY_USERS, "")
        .into_iter()
        .filter(|s| s.starts_with("S-1-5-21-") && !s.ends_with("_Classes"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_well_known_values() {
        let cv = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        assert!(string(HKEY_LOCAL_MACHINE, cv, "ProductName").is_some_and(|s| s.contains("Windows")));
        assert!(dword(HKEY_LOCAL_MACHINE, cv, "CurrentMajorVersionNumber").is_some());
        assert!(exists(HKEY_LOCAL_MACHINE, cv));
        assert!(!exists(HKEY_LOCAL_MACHINE, r"SOFTWARE\Syscura-Does-Not-Exist"));
        assert!(!subkeys(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Services").is_empty());
    }
}
