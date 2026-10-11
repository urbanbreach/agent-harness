use crossbeam_channel::{bounded, Receiver};
use signal_hook::{
    consts::{SIGHUP, SIGINT, SIGTERM},
    iterator::{Handle, Signals},
};

pub(super) struct ShutdownSignals {
    pub receiver: Receiver<i32>,
    handle: Handle,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ShutdownSignals {
    pub fn new() -> std::io::Result<Self> {
        let mut signals = Signals::new([SIGHUP, SIGINT, SIGTERM])?;
        let handle = signals.handle();
        let (sender, receiver) = bounded(1);
        let thread = std::thread::Builder::new()
            .name("tui-signals".into())
            .spawn(move || {
                for signal in signals.forever() {
                    let _ = sender.try_send(signal);
                }
            })?;
        Ok(Self {
            receiver,
            handle,
            thread: Some(thread),
        })
    }
}

impl Drop for ShutdownSignals {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
