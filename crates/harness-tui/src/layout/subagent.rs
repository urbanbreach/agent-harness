//! Shared child geometry for painting, scrolling and pointer targets.
use super::*;

pub(crate) fn frame(area: Rect) -> Option<Rect> {
    let compact = area.height > 0 && area.height <= 20;
    let horizontal = if compact { 1 } else { 2 };
    let vertical = u16::from(!compact);
    let frame = Rect::new(
        area.x.saturating_add(horizontal),
        area.y.saturating_add(vertical),
        area.width.saturating_sub(horizontal * 2),
        area.height.saturating_sub(vertical * 2),
    );
    (frame.width >= 10 && frame.height >= 5).then_some(frame)
}

pub(crate) fn close(area: Rect) -> Option<Rect> {
    let frame = frame(area)?;
    Some(Rect::new(
        frame.right().saturating_sub(5),
        frame.y + 1,
        3,
        1,
    ))
}

pub(super) fn project(plan: &mut FrameLayoutPlan) {
    plan.dock = None;
    plan.composer = None;
    plan.disclosure = None;
    plan.todo = None;
    plan.tasks = None;
    plan.terminal_panel = None;
    plan.operator_sidebar = None;
    plan.details_overlay = None;
    let Some(frame) = frame(plan.root) else {
        plan.transcript = None;
        plan.header = Rect::default();
        plan.footer = Rect::default();
        plan.status = None;
        plan.wheel_hit_areas = WheelHitAreas::default();
        return;
    };
    let inner = Rect::new(frame.x + 1, frame.y + 3, frame.width - 2, frame.height - 4);
    plan.shell = inner;
    plan.header = Rect::new(inner.x + 2, inner.y + 1, inner.width.saturating_sub(4), 1);
    plan.header_text = plan.header;
    plan.footer = Rect::new(
        inner.x + 2,
        inner.bottom().saturating_sub(2),
        inner.width.saturating_sub(4),
        1,
    );
    plan.footer_text = plan.footer;
    plan.status = Some(Rect::new(
        inner.x + 2,
        inner.bottom().saturating_sub(5),
        inner.width.saturating_sub(4),
        1,
    ));
    if inner.height < 9 {
        plan.status = None;
        plan.header.height = u16::from(inner.height > 1);
        plan.header_text = plan.header;
        plan.footer.height = u16::from(inner.height > 3);
        plan.footer_text = plan.footer;
    }
    let transcript = Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        inner.height.saturating_sub(7),
    );
    plan.content = transcript;
    plan.transcript = Some(transcript);
    plan.wheel_hit_areas = WheelHitAreas {
        transcript: Some(transcript),
        ..Default::default()
    };
}
