//! Per-user registration (HKCU only — no admin rights, nothing
//! machine-wide), the Windows counterpart of installing the systemd
//! user unit (K7):
//!
//! - `Software\Classes\AppUserModelId\Newsflash.Kyu` — the identity
//!   toasts are shown under (name + icon in the toast header and in
//!   Settings → Notifications), plus `CustomActivator` so clicks are
//!   delivered to our COM class.
//! - `Software\Classes\CLSID\{…}\LocalServer32` — that COM class; lets
//!   Windows start `newsflashw.exe` for a click on an old toast in
//!   Notification Center when the daemon is not running.
//! - `Software\Microsoft\Windows\CurrentVersion\Run` — start at logon,
//!   stop at logoff: toasts only while someone is there to see them
//!   (the AR20 topology on Windows).

use crate::AUMID;
use std::path::Path;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};
use windows::core::{HSTRING, PCWSTR};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "newsflash";

fn aumid_key() -> String {
    format!(r"Software\Classes\AppUserModelId\{AUMID}")
}

fn clsid_key() -> String {
    format!(
        r"Software\Classes\CLSID\{}",
        crate::activator::clsid_string()
    )
}

fn check(err: WIN32_ERROR, what: &str) -> Result<(), String> {
    if err == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "{what}: {}",
            windows::core::Error::from(err.to_hresult()).message()
        ))
    }
}

fn set_string(subkey: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    set_typed(subkey, name, value, REG_SZ)
}

fn set_typed(
    subkey: &str,
    name: Option<&str>,
    value: &str,
    kind: REG_VALUE_TYPE,
) -> Result<(), String> {
    let mut key = HKEY::default();
    unsafe {
        check(
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(subkey),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            ),
            &format!("creating HKCU\\{subkey}"),
        )?;
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        let name = name.map(HSTRING::from);
        let result = RegSetValueExW(
            key,
            name.as_ref()
                .map(|n| PCWSTR(n.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            None,
            kind,
            Some(bytes),
        );
        let _ = RegCloseKey(key);
        check(result, &format!("writing HKCU\\{subkey}"))
    }
}

fn get_string(subkey: &str, name: Option<&str>) -> Option<String> {
    let name = name.map(HSTRING::from);
    let name_ptr = name
        .as_ref()
        .map(|n| PCWSTR(n.as_ptr()))
        .unwrap_or(PCWSTR::null());
    let subkey = HSTRING::from(subkey);
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            name_ptr,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

const ENV_KEY: &str = "Environment";

/// The user PATH exactly as stored (`%VARS%` unexpanded) and its type;
/// an absent value reads as empty REG_EXPAND_SZ, like a fresh profile.
fn user_path() -> Result<(String, REG_VALUE_TYPE), String> {
    let subkey = HSTRING::from(ENV_KEY);
    let name = HSTRING::from("Path");
    let mut kind = REG_VALUE_TYPE::default();
    let mut len = 0u32;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            &name,
            flags,
            Some(&mut kind),
            None,
            Some(&mut len),
        )
    };
    if err == ERROR_FILE_NOT_FOUND {
        return Ok((String::new(), REG_EXPAND_SZ));
    }
    check(err, "reading the user PATH")?;
    let mut buf = vec![0u16; len as usize / 2 + 1];
    let mut len = (buf.len() * 2) as u32;
    check(
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &subkey,
                &name,
                flags,
                Some(&mut kind),
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut len),
            )
        },
        "reading the user PATH",
    )?;
    let chars = (len as usize / 2).saturating_sub(1);
    Ok((String::from_utf16_lossy(&buf[..chars]), kind))
}

/// Tells Explorer (and so every window opened afterwards) that the
/// environment changed. Already-open consoles keep their old PATH.
fn broadcast_environment_change() {
    let area = HSTRING::from(ENV_KEY);
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(area.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            None,
        );
    }
}

/// Adds `dir` to the user PATH. `Ok(false)` = it was already there.
pub fn add_to_user_path(dir: &Path) -> Result<bool, String> {
    let (current, kind) = user_path()?;
    let Some(updated) = crate::paths::path_with(&current, &dir.display().to_string()) else {
        return Ok(false);
    };
    set_typed(ENV_KEY, Some("Path"), &updated, kind)?;
    broadcast_environment_change();
    Ok(true)
}

/// Removes `dir` from the user PATH. `Ok(false)` = it was not there.
pub fn remove_from_user_path(dir: &Path) -> Result<bool, String> {
    let (current, kind) = user_path()?;
    let Some(updated) = crate::paths::path_without(&current, &dir.display().to_string()) else {
        return Ok(false);
    };
    set_typed(ENV_KEY, Some("Path"), &updated, kind)?;
    broadcast_environment_change();
    Ok(true)
}

pub fn is_on_user_path(dir: &Path) -> bool {
    user_path().is_ok_and(|(current, _)| {
        crate::paths::path_with(&current, &dir.display().to_string()).is_none()
    })
}

fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

pub fn register(windowless_exe: &Path, icon: &Path) -> Result<(), String> {
    let clsid = crate::activator::clsid_string();
    set_string(&aumid_key(), Some("DisplayName"), "newsflash")?;
    set_string(&aumid_key(), Some("IconUri"), &icon.display().to_string())?;
    set_string(&aumid_key(), Some("CustomActivator"), &clsid)?;
    set_string(&clsid_key(), None, "newsflash toast activator")?;
    set_string(
        &format!(r"{}\LocalServer32", clsid_key()),
        None,
        &quoted(windowless_exe),
    )?;
    Ok(())
}

/// Start at logon (the AR20 topology on Windows), or not.
pub fn set_autostart(windowless_exe: Option<&Path>) -> Result<(), String> {
    match windowless_exe {
        Some(exe) => set_string(RUN_KEY, Some(RUN_VALUE), &quoted(exe)),
        None => {
            let err = unsafe {
                RegDeleteKeyValueW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(RUN_KEY),
                    &HSTRING::from(RUN_VALUE),
                )
            };
            if err == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                check(err, "removing the logon autostart")
            }
        }
    }
}

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\newsflash";

fn set_dword(subkey: &str, name: &str, value: u32) -> Result<(), String> {
    let mut key = HKEY::default();
    unsafe {
        check(
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(subkey),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            ),
            &format!("creating HKCU\\{subkey}"),
        )?;
        let result = RegSetValueExW(
            key,
            &HSTRING::from(name),
            None,
            REG_DWORD,
            Some(&value.to_le_bytes()),
        );
        let _ = RegCloseKey(key);
        check(result, &format!("writing HKCU\\{subkey}"))
    }
}

/// Settings → Apps → Installed apps: "Modify" and "Uninstall" both open
/// the setup wizard (on the uninstall confirmation for the latter).
pub fn register_uninstall_entry(
    windowless_exe: &Path,
    install_dir: &Path,
    version: &str,
) -> Result<(), String> {
    let exe = quoted(windowless_exe);
    set_string(UNINSTALL_KEY, Some("DisplayName"), "newsflash")?;
    set_string(UNINSTALL_KEY, Some("DisplayVersion"), version)?;
    set_string(UNINSTALL_KEY, Some("Publisher"), "Kenny Passenier")?;
    set_string(
        UNINSTALL_KEY,
        Some("InstallLocation"),
        &install_dir.display().to_string(),
    )?;
    set_string(
        UNINSTALL_KEY,
        Some("DisplayIcon"),
        &windowless_exe.display().to_string(),
    )?;
    set_string(UNINSTALL_KEY, Some("ModifyPath"), &format!("{exe} setup"))?;
    set_string(
        UNINSTALL_KEY,
        Some("UninstallString"),
        &format!("{exe} setup uninstall"),
    )?;
    set_dword(UNINSTALL_KEY, "NoRepair", 1)
}

pub fn remove_uninstall_entry() -> Result<(), String> {
    let err = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY)) };
    if err == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        check(err, "removing the Installed apps entry")
    }
}

/// Removes every key `register` wrote; absent keys are fine.
pub fn unregister() -> Result<(), String> {
    let gone = |err: WIN32_ERROR, what: &str| {
        if err == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(err, what)
        }
    };
    unsafe {
        gone(
            RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(aumid_key())),
            "removing the AppUserModelId key",
        )?;
        gone(
            RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(clsid_key())),
            "removing the COM activator key",
        )?;
        gone(
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(RUN_KEY),
                &HSTRING::from(RUN_VALUE),
            ),
            "removing the logon autostart",
        )
    }
}

#[derive(Debug)]
pub struct Registration {
    pub display_name: Option<String>,
    pub activator_exe: Option<String>,
    pub autostart: Option<String>,
}

pub fn read() -> Registration {
    Registration {
        display_name: get_string(&aumid_key(), Some("DisplayName")),
        activator_exe: get_string(&format!(r"{}\LocalServer32", clsid_key()), None),
        autostart: get_string(RUN_KEY, Some(RUN_VALUE)),
    }
}

/// Without the AppUserModelId key Windows silently drops our toasts.
pub fn is_registered() -> bool {
    get_string(&aumid_key(), Some("DisplayName")).is_some()
}
