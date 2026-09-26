//! The Start menu entry "newsflash setup" (reopens the wizard), via the
//! shell's own IShellLink — a real .lnk, like any installer writes.

use std::path::{Path, PathBuf};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{HSTRING, Interface};

fn lnk_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(r"Microsoft\Windows\Start Menu\Programs\newsflash setup.lnk")
}

pub fn create(windowless_exe: &Path) -> Result<PathBuf, String> {
    let path = lnk_path();
    unsafe {
        // Already initialised in another mode on this thread is fine.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| e.message().to_string())?;
        link.SetPath(&HSTRING::from(windowless_exe.as_os_str()))
            .and_then(|_| link.SetArguments(&HSTRING::from("setup")))
            .and_then(|_| {
                link.SetDescription(&HSTRING::from(
                    "Install, reconfigure or remove the kyu desktop notifications",
                ))
            })
            .map_err(|e| e.message().to_string())?;
        let file: IPersistFile = link.cast().map_err(|e| e.message().to_string())?;
        file.Save(&HSTRING::from(path.as_os_str()), true)
            .map_err(|e| e.message().to_string())?;
    }
    Ok(path)
}

pub fn remove() -> bool {
    std::fs::remove_file(lnk_path()).is_ok()
}
