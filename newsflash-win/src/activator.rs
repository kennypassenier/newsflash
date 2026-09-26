//! M10 on Windows: button clicks arrive through COM. Windows calls
//! `INotificationActivationCallback::Activate` on the class registered
//! as the AUMID's `CustomActivator` — in the running daemon if it has
//! registered the class object, otherwise by starting `newsflashw.exe
//! -Embedding`. That makes buttons work from the popup AND from
//! Notification Center, even for a toast shown before a restart.
//!
//! The callback runs on a COM thread and must return quickly, so it
//! only forwards the raw arguments and input values over a channel;
//! the receiver decodes and publishes (AR24: nothing here can delay
//! the loop).

use std::sync::mpsc::Sender;
use windows::Win32::Foundation::CLASS_E_NOAGGREGATION;
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoInitializeEx, CoRegisterClassObject,
    CoRevokeClassObject, IClassFactory, IClassFactory_Impl, REGCLS_MULTIPLEUSE,
};
use windows::Win32::UI::Notifications::{
    INotificationActivationCallback, INotificationActivationCallback_Impl,
    NOTIFICATION_USER_INPUT_DATA,
};
use windows::core::{BOOL, GUID, IUnknown, Interface, PCWSTR, Ref, implement};

/// Must match nothing else on the machine; fixed forever once
/// installed (the registry points at it).
pub const CLSID: GUID = GUID::from_u128(0x07db5a3e_0f0c_4d8d_885f_53ec854f29ef);

pub fn clsid_string() -> String {
    format!("{{{CLSID:?}}}")
}

/// One click: the `arguments` of the button (or the toast's `launch`
/// for a body click) and the values of any inputs, keyed by input id.
#[derive(Debug)]
pub struct Click {
    pub args: String,
    pub inputs: Vec<(String, String)>,
}

#[implement(INotificationActivationCallback)]
struct Activator {
    tx: Sender<Click>,
}

impl INotificationActivationCallback_Impl for Activator_Impl {
    fn Activate(
        &self,
        _appusermodelid: &PCWSTR,
        invokedargs: &PCWSTR,
        data: *const NOTIFICATION_USER_INPUT_DATA,
        count: u32,
    ) -> windows::core::Result<()> {
        let args = unsafe { invokedargs.to_string() }.unwrap_or_default();
        let mut inputs = Vec::new();
        if !data.is_null() {
            for i in 0..count as usize {
                let item = unsafe { &*data.add(i) };
                let key = unsafe { item.Key.to_string() }.unwrap_or_default();
                let value = unsafe { item.Value.to_string() }.unwrap_or_default();
                inputs.push((key, value));
            }
        }
        let _ = self.tx.send(Click { args, inputs });
        Ok(())
    }
}

#[implement(IClassFactory)]
struct Factory {
    tx: Sender<Click>,
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        riid: *const GUID,
        object: *mut *mut core::ffi::c_void,
    ) -> windows::core::Result<()> {
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let unknown: IUnknown = Activator {
            tx: self.tx.clone(),
        }
        .into();
        unsafe { unknown.query(riid, object).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Revokes the class object on drop.
pub struct Registered(u32);

impl Drop for Registered {
    fn drop(&mut self) {
        unsafe {
            let _ = CoRevokeClassObject(self.0);
        }
    }
}

/// Joins the multithreaded apartment (callbacks then arrive on COM's
/// own threads — no message pump needed) and publishes the class
/// object so Windows can hand us clicks.
pub fn register(tx: Sender<Click>) -> Result<Registered, String> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| format!("COM init failed: {}", e.message()))?;
        let factory: IClassFactory = Factory { tx }.into();
        let cookie =
            CoRegisterClassObject(&CLSID, &factory, CLSCTX_LOCAL_SERVER, REGCLS_MULTIPLEUSE)
                .map_err(|e| format!("registering the click activator failed: {}", e.message()))?;
        Ok(Registered(cookie))
    }
}
