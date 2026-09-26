//! `newsflashw.exe` — the same program without a console window, for
//! the logon autostart and COM click activation. Logs go to the file.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    std::process::exit(newsflash_win::app::main(true));
    #[cfg(not(windows))]
    {
        eprintln!("newsflash-win is the Windows build; on Linux run `newsflash`.");
        std::process::exit(2);
    }
}
