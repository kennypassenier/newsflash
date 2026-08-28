//! desk-courier — renders messages from the mailbox hub's
//! `notify.kenny` topic as desktop toasts. See docs/SCOPE.md.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("desk-courier {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    eprintln!("desk-courier: not implemented yet (L0 walking skeleton)");
    std::process::exit(2);
}
