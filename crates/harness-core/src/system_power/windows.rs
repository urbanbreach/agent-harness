//! Windows suspend/resume notifications without a window message loop.

use std::os::raw::c_void;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Power::{
    PowerRegisterSuspendResumeNotification, PowerUnregisterSuspendResumeNotification,
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK;

use super::{PowerCallback, PowerEvent, PowerState};

const SUSPEND: u32 = 0x0004;
const RESUME_SUSPEND: u32 = 0x0007;
const RESUME_AUTOMATIC: u32 = 0x0012;
const SUCCESS: u32 = 0;

struct Context {
    callback: PowerCallback,
}

pub(crate) struct Listener {
    handle: *mut c_void,
    context: *mut Context,
}

// SAFETY: [Category 9 — Send/Sync] Windows invokes the callback on arbitrary
// threads, while the registration handle is used only for unregistering.
unsafe impl Send for Listener {}
// SAFETY: [Category 9 — Send/Sync] `PowerCallback` is Send + Sync and the raw
// context is reclaimed only after the OS unregister call completes.
unsafe impl Sync for Listener {}

impl Listener {
    pub(crate) fn start(callback: PowerCallback) -> Option<Self> {
        let context = Box::into_raw(Box::new(Context { callback }));
        let mut parameters = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(callback_entry),
            Context: context.cast(),
        };
        let mut handle = std::ptr::null_mut();
        // SAFETY: [Category 8 — FFI boundary] parameter and output pointers are
        // valid for the registration call; Windows retains only `context`.
        let result = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                (&mut parameters as *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS).cast::<c_void>()
                    as HANDLE,
                &mut handle,
            )
        };
        if result != SUCCESS || handle.is_null() {
            // SAFETY: [Category 12 — invalid free] the registration failed, so
            // Windows has not retained `context` and this is its only owner.
            unsafe { drop(Box::from_raw(context)) };
            return None;
        }
        Some(Self { handle, context })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // SAFETY: [Category 12 — invalid free] unregister stops future callbacks
        // before the one heap context is reclaimed.
        unsafe {
            PowerUnregisterSuspendResumeNotification(self.handle as HPOWERNOTIFY);
            drop(Box::from_raw(self.context));
        }
    }
}

unsafe extern "system" fn callback_entry(
    context: *const c_void,
    event: u32,
    _setting: *const c_void,
) -> u32 {
    // SAFETY: [Category 3 — dangling pointer] Windows calls with the live
    // context given at registration, which remains owned until unregister.
    let context = unsafe { &*context.cast::<Context>() };
    match event {
        SUSPEND => (context.callback)(PowerEvent::WillSleep),
        RESUME_SUSPEND | RESUME_AUTOMATIC => (context.callback)(PowerEvent::DidWake),
        _ => {}
    }
    SUCCESS
}

pub(crate) const fn current_power_state() -> PowerState {
    PowerState::Unknown
}

pub(crate) const fn unavailable_diagnostic() -> &'static str {
    "Windows system-power registration unavailable: PowerRegisterSuspendResumeNotification failed"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_power_constants_distinguish_suspend_and_resume() {
        // arrange
        let suspend = SUSPEND;

        // act
        let resume = RESUME_SUSPEND;

        // assert
        assert_ne!(suspend, resume);
        assert_ne!(resume, RESUME_AUTOMATIC);
    }
}
