#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RedrawCoalescer {
    pending: bool,
}

impl RedrawCoalescer {
    pub const fn new() -> Self {
        Self { pending: false }
    }

    pub fn request(&mut self) {
        self.pending = true;
    }

    pub const fn is_pending(self) -> bool {
        self.pending
    }

    pub fn take(&mut self) -> bool {
        let pending = self.pending;
        self.pending = false;
        pending
    }
}
