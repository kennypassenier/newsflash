//! `newsflash.exe` — the CLI and foreground courier (logs echo to the
//! console). See `newsflash_win::app`.

fn main() {
    #[cfg(windows)]
    std::process::exit(newsflash_win::app::main(false));
    #[cfg(not(windows))]
    {
        eprintln!("newsflash-win is the Windows build; on Linux run `newsflash`.");
        std::process::exit(2);
    }
}
