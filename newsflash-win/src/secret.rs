//! The token on Windows (K9, AR10). There is no latch here, so
//! `newsflash set-token` encrypts the app token with DPAPI — bound to
//! this Windows user account, unreadable to anyone else (and to this
//! same account on another machine). Resolution order at startup:
//! `KYU_TOKEN` env → `token.dpapi` → the config's `token_file`.

use std::io::{BufRead, Write};
use std::path::Path;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, SetConsoleMode,
};
use windows::core::w;

fn blob_to_vec(blob: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(Some(HLOCAL(blob.pbData as _)));
    }
    bytes
}

pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            w!("newsflash kyu app token"),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    }
    .map_err(|e| format!("DPAPI encrypt failed: {}", e.message()))?;
    Ok(blob_to_vec(&out))
}

pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    }
    .map_err(|e| format!("DPAPI decrypt failed: {}", e.message()))?;
    Ok(blob_to_vec(&out))
}

/// `Ok(None)` = no token file; `Err` = present but unreadable (a
/// profile moved to another machine, say) — the caller names the remedy.
pub fn load(path: &Path) -> Result<Option<String>, String> {
    let cipher = match std::fs::read(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let plain = unprotect(&cipher).map_err(|e| {
        format!(
            "{} cannot be decrypted ({e}). It only opens for the Windows account that wrote \
             it — run `newsflash set-token` again.",
            path.display()
        )
    })?;
    let token =
        String::from_utf8(plain).map_err(|_| format!("{} does not hold text", path.display()))?;
    let token = token.trim().to_string();
    Ok((!token.is_empty()).then_some(token))
}

pub fn store(path: &Path, token: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let cipher = protect(token.trim().as_bytes())?;
    std::fs::write(path, cipher).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Reads one line from stdin without echoing it when stdin is a
/// console (so the token never lands on screen or in scrollback);
/// piped input (`Get-Content token.txt | newsflash set-token`) works too.
pub fn read_secret_line(prompt: &str) -> String {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let restore = unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE).ok();
        let mut mode = CONSOLE_MODE::default();
        match handle {
            Some(h) if GetConsoleMode(h, &mut mode).is_ok() => {
                let _ = SetConsoleMode(h, mode & !ENABLE_ECHO_INPUT);
                Some((h, mode))
            }
            _ => None,
        }
    };
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    if let Some((h, mode)) = restore {
        unsafe {
            let _ = SetConsoleMode(h, mode);
        }
        eprintln!();
    }
    line.trim().to_string()
}
