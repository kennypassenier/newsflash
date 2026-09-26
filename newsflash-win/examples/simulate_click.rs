//! Drill tool: performs exactly the COM call Windows makes when a toast
//! button is clicked — `CoCreateInstance(<our CLSID>)` +
//! `INotificationActivationCallback::Activate` — so the click path is
//! testable without a mouse or a screen. With the daemon running, COM
//! routes the call into it; with no daemon, COM starts
//! `newsflashw.exe -Embedding` (the Notification Center case).
//!
//!   cargo xwin run --example simulate_click --target x86_64-pc-windows-msvc -- \
//!       <hub id> <envelope id> <action id> [key=value ...]
//!
//! Only meaningful against a scratch hub: a simulated click publishes a
//! real action_result to notify.actions.

#[cfg(windows)]
fn main() {
    use courier_core::wintoast::{Activation, encode_activation};
    use windows::Win32::System::Com::{
        CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Notifications::{
        INotificationActivationCallback, NOTIFICATION_USER_INPUT_DATA,
    };
    use windows::core::{HSTRING, PCWSTR};

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: simulate_click <hub id> <envelope id> <action id> [key=value ...]");
        std::process::exit(2);
    }
    let invoked = encode_activation(&Activation::Button {
        action_id: args[2].clone(),
        envelope_id: args[1].clone(),
        ack_id: None,
        hub_id: args[0].clone(),
        demo: false,
    });
    let pairs: Vec<(HSTRING, HSTRING)> = args[3..]
        .iter()
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (HSTRING::from(k), HSTRING::from(v)))
        .collect();
    let data: Vec<NOTIFICATION_USER_INPUT_DATA> = pairs
        .iter()
        .map(|(k, v)| NOTIFICATION_USER_INPUT_DATA {
            Key: PCWSTR(k.as_ptr()),
            Value: PCWSTR(v.as_ptr()),
        })
        .collect();
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        let callback: INotificationActivationCallback =
            CoCreateInstance(&newsflash_win::activator::CLSID, None, CLSCTX_LOCAL_SERVER)
                .expect("the activator class is not available — is newsflash installed?");
        callback
            .Activate(
                &HSTRING::from(newsflash_win::AUMID),
                &HSTRING::from(invoked.as_str()),
                &data,
            )
            .expect("Activate failed");
    }
    println!(
        "clicked {:?} on {} with {} input(s)",
        args[2],
        args[0],
        data.len()
    );
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows only");
}
