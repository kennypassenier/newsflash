# Setup wizard (Windows and Linux)

One window, the same on both OSes (crate `newsflash-setup`, egui). It
makes every check before it writes anything:

1. **Welcome.** Shows whether newsflash is installed and running.
   Offers Install, Update / reconfigure, or Uninstall.
2. **Hub.** The address (default `http://10.10.10.9:8080`). Next tests
   that the hub answers. If it doesn't, "Continue anyway" is offered.
3. **Token.** Hidden input, with a link to the hub's `/apps` page. Next
   checks the token read-only (a policy GET, never a receive). A
   rejected token blocks. Leave the field empty to keep a stored token.
4. **Options.** Language; how critical notifications behave (Windows
   only: reminder / urgent / alarm); start at login; add to PATH.
5. **Install**, with a live log, then **Done**, with a "Send a test
   notification" button.

| | Windows | Linux (Garuda) |
|---|---|---|
| open it | double-click `newsflashw.exe` (before install), Start menu **newsflash setup**, Settings → Installed apps → newsflash → Modify, or `newsflash setup` | app menu **newsflash setup**, or `newsflash setup` |
| uninstall | Installed apps → Uninstall (opens the wizard's confirmation), or `newsflash setup uninstall` | `newsflash setup uninstall`, or `newsflash uninstall` |
| token | DPAPI store (this Windows account) | typed → `~/.config/newsflash/token` (0600) and the unit runs without latch; left empty → latch keeps providing it |
| installs | `%LOCALAPPDATA%\Programs\newsflash`, HKCU registration, Run key, Start menu `.lnk`, Installed apps entry, user PATH | `~/.local/bin/newsflash`, systemd user unit, PATH drop-ins, app-menu `.desktop` entry + icon |

The config is edited line by line (`newsflash::config_edit`): comments
and keys the wizard doesn't know about stay exactly as they were.

Tests: `newsflash-setup/src/tests.rs` drives the real pages headlessly
(egui_kittest) against a fake backend. It covers the full install, a
rejected token, an unreachable hub, keeping a stored token, uninstall,
and address validation. The Linux backend's hub checks and config/token
writing are tested in `newsflash/src/setup.rs`, and `install`/`uninstall`
end to end in `newsflash/tests/install_tests.rs`. The Windows crates'
tests also run natively on Windows from WSL:
`CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=env cargo xwin test -p newsflash-win -p newsflash-setup --target x86_64-pc-windows-msvc`.
