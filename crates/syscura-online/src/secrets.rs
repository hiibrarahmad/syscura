//! Keeps the AI key in Windows Credential Manager (encrypted by Windows
//! for this user), never in a plain file.

use windows::Win32::Security::Credentials::{
    CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};
use windows::core::{HSTRING, PWSTR};

const TARGET: &str = "Syscura/Gemini";

pub fn save_key(key: &str) -> Result<(), String> {
    let mut target: Vec<u16> = TARGET.encode_utf16().chain(Some(0)).collect();
    let mut blob = key.trim().as_bytes().to_vec();
    let cred = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    unsafe { CredWriteW(&cred, 0) }.map_err(|e| format!("Could not save the key: {e}"))
}

pub fn load_key() -> Option<String> {
    let mut p: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        CredReadW(&HSTRING::from(TARGET), CRED_TYPE_GENERIC, None, &mut p).ok()?;
        let c = &*p;
        let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
        CredFree(p as *const _);
        String::from_utf8(bytes).ok().filter(|k| !k.is_empty())
    }
}

pub fn delete_key() {
    let _ = unsafe { CredDeleteW(&HSTRING::from(TARGET), CRED_TYPE_GENERIC, None) };
}
