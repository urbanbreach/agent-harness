use std::cell::RefCell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameHyperlink {
    pub(crate) row: u16,
    pub(crate) start_column: u16,
    pub(crate) end_column: u16,
    pub(crate) destination: String,
}

thread_local! {
    static FRAME_HYPERLINKS: RefCell<Vec<FrameHyperlink>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn set_frame_hyperlinks(links: Vec<FrameHyperlink>) {
    FRAME_HYPERLINKS.with(|current| *current.borrow_mut() = links);
}

pub(super) fn take_frame_hyperlinks() -> Vec<FrameHyperlink> {
    FRAME_HYPERLINKS.with(|current| std::mem::take(&mut *current.borrow_mut()))
}
