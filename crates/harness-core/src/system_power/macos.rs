// allow: SIZE_OK — direct IOKit registration and safe RAII teardown.
//! IOKit system-power notifications and a dark-wake capability query.

use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use super::{PowerCallback, PowerEvent, PowerState};

type MachPort = u32;
const NULL_PORT: MachPort = 0;
const CAN_SLEEP: u32 = 0xe000_0270;
const WILL_SLEEP: u32 = 0xe000_0280;
const WILL_NOT_SLEEP: u32 = 0xe000_0290;
const DID_WAKE: u32 = 0xe000_0300;
const CAPABILITY_CPU: u32 = 0x1;
const CAPABILITY_VIDEO: u32 = 0x2;

type Callback = extern "C" fn(*mut c_void, MachPort, u32, *mut c_void);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: *const c_void;
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRunInMode(
        mode: *const c_void,
        seconds: f64,
        return_after_source_handled: u8,
    ) -> i32;
    fn CFRunLoopStop(runloop: *mut c_void);
    fn CFRunLoopAddSource(runloop: *mut c_void, source: *mut c_void, mode: *const c_void);
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegisterForSystemPower(
        refcon: *mut c_void,
        port: *mut *mut c_void,
        callback: Callback,
        notifier: *mut MachPort,
    ) -> MachPort;
    fn IODeregisterForSystemPower(notifier: *mut MachPort) -> i32;
    fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> *mut c_void;
    fn IONotificationPortDestroy(port: *mut c_void);
    fn IOAllowPowerChange(port: MachPort, notification_id: isize) -> i32;
    fn IOServiceClose(port: MachPort) -> i32;
    fn IOPMConnectionGetSystemCapabilities() -> u32;
}

pub(crate) const fn classify_capabilities(capabilities: u32) -> PowerState {
    if capabilities & CAPABILITY_CPU == 0 {
        PowerState::Unknown
    } else if capabilities & CAPABILITY_VIDEO != 0 {
        PowerState::FullWake
    } else {
        PowerState::DarkWake
    }
}

pub(crate) fn current_power_state() -> PowerState {
    // SAFETY: [Category 8 — FFI boundary] Apple declares this no-argument IOKit
    // function to return a plain bitfield; its ABI is fixed by IOKit.
    classify_capabilities(unsafe { IOPMConnectionGetSystemCapabilities() })
}

pub(crate) const fn unavailable_diagnostic() -> &'static str {
    "macOS system-power registration unavailable: IOKit IORegisterForSystemPower failed"
}

struct Context {
    callback: PowerCallback,
    root_port: MachPort,
}

struct SendRunLoop(*mut c_void);
// SAFETY: [Category 9 — Send/Sync] CoreFoundation documents CFRunLoopStop as
// callable cross-thread; the raw pointer is never dereferenced by Rust.
unsafe impl Send for SendRunLoop {}

pub(crate) struct Listener {
    runloop: SendRunLoop,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Listener {
    pub(crate) fn start(callback: PowerCallback) -> Option<Self> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("harness-system-power".to_string())
            .spawn(move || run_listener(callback, ready_tx, worker_stop))
            .ok()?;
        match ready_rx.recv() {
            Ok(Some(runloop)) => Some(Self {
                runloop,
                stop,
                worker: Some(worker),
            }),
            _ => {
                let _ = worker.join();
                None
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // SAFETY: [Category 8 — FFI boundary] `runloop` was returned by the
        // owning run-loop thread and remains live until that thread joins below.
        unsafe { CFRunLoopStop(self.runloop.0) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_listener(
    callback: PowerCallback,
    ready: mpsc::Sender<Option<SendRunLoop>>,
    stop: Arc<AtomicBool>,
) {
    let context = Box::into_raw(Box::new(Context {
        callback,
        root_port: NULL_PORT,
    }));
    let mut notifier = NULL_PORT;
    let mut port = std::ptr::null_mut();
    // SAFETY: [Category 8 — FFI boundary] `context`, `port`, and `notifier` are
    // live writable allocations for this registration and the callback type
    // matches IOKit's documented signature.
    let root_port = unsafe {
        IORegisterForSystemPower(context.cast(), &mut port, callback_entry, &mut notifier)
    };
    if root_port == NULL_PORT || port.is_null() {
        // SAFETY: [Category 12 — invalid free] registration failed before IOKit
        // can retain the context, so this is the sole reclamation path.
        unsafe { drop(Box::from_raw(context)) };
        let _ = ready.send(None);
        return;
    }
    // SAFETY: [Category 3 — dangling pointer] IOKit cannot invoke the callback
    // before the run loop starts below, so the live context is initialized first.
    unsafe { (*context).root_port = root_port };
    // SAFETY: [Category 8 — FFI boundary] `port` and the current run loop are
    // valid IOKit/CoreFoundation handles for this registration.
    let runloop = unsafe { CFRunLoopGetCurrent() };
    // SAFETY: [Category 8 — FFI boundary] IOKit returns a run-loop source owned
    // by `port`; it stays valid until teardown after the loop stops.
    unsafe {
        CFRunLoopAddSource(
            runloop,
            IONotificationPortGetRunLoopSource(port),
            kCFRunLoopCommonModes,
        )
    };
    if ready.send(Some(SendRunLoop(runloop))).is_err() {
        cleanup(context, &mut notifier, port, root_port);
        return;
    }
    while !stop.load(Ordering::SeqCst) {
        // SAFETY: [Category 8 — FFI boundary] the active thread owns this loop
        // and uses the SDK-provided default mode with a finite timeout.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 5.0, 0) };
    }
    cleanup(context, &mut notifier, port, root_port);
}

fn cleanup(context: *mut Context, notifier: &mut MachPort, port: *mut c_void, root_port: MachPort) {
    // SAFETY: [Category 12 — invalid free] this function runs once after the
    // listener loop exits; it unregisters IOKit before reclaiming its context.
    unsafe {
        IODeregisterForSystemPower(notifier);
        IONotificationPortDestroy(port);
        IOServiceClose(root_port);
        drop(Box::from_raw(context));
    }
}

fn map_message(message: u32) -> (Option<PowerEvent>, bool) {
    match message {
        CAN_SLEEP | WILL_SLEEP => (Some(PowerEvent::WillSleep), true),
        WILL_NOT_SLEEP | DID_WAKE => (Some(PowerEvent::DidWake), false),
        _ => (None, false),
    }
}

extern "C" fn callback_entry(
    context: *mut c_void,
    _service: MachPort,
    message: u32,
    argument: *mut c_void,
) {
    // SAFETY: [Category 3 — dangling pointer] IOKit calls only while the
    // listener registration owns `Context`; teardown unregisters before freeing.
    let context = unsafe { &*(context.cast::<Context>()) };
    let (event, acknowledge) = map_message(message);
    if let Some(event) = event {
        (context.callback)(event);
    }
    if acknowledge {
        // SAFETY: [Category 8 — FFI boundary] IOKit provided this notification
        // identifier for the currently registered root port.
        unsafe { IOAllowPowerChange(context.root_port, argument as isize) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_wake_has_cpu_without_video() {
        // arrange
        let cpu_only = CAPABILITY_CPU;

        // act
        let state = classify_capabilities(cpu_only);

        // assert
        assert_eq!(state, PowerState::DarkWake);
        assert_eq!(
            classify_capabilities(CAPABILITY_CPU | CAPABILITY_VIDEO),
            PowerState::FullWake
        );
        assert_eq!(classify_capabilities(0), PowerState::Unknown);
    }

    #[test]
    fn idle_sleep_and_cancelled_sleep_map_to_gate_transitions() {
        // arrange

        // act
        let idle_sleep = map_message(CAN_SLEEP);

        // assert
        assert_eq!(idle_sleep, (Some(PowerEvent::WillSleep), true));
        assert_eq!(
            map_message(WILL_NOT_SLEEP),
            (Some(PowerEvent::DidWake), false)
        );
    }
}
