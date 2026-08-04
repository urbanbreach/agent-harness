use super::{CapabilityClassifier, axes::*};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "viewport")]
pub struct CapabilityCell {
    pub color: ColorCapability,
    pub graphics: GraphicsCapability,
    pub keyboard: KeyboardCapability,
    pub focus: FocusCapability,
    pub notification: NotificationCapability,
    pub clipboard: ClipboardCapability,
    pub title: TitleCapability,
    pub multiplexer: MultiplexerCapability,
    pub platform: PlatformCapability,
    pub width: WidthCapability,
    pub motion: MotionCapability,
    pub viewport: ViewportCapability,
}
impl CapabilityCell {
    pub const fn is_classified(&self) -> bool {
        true
    }
    pub fn label(&self) -> String {
        format!(
            "{}:{}:{}:{}x{}",
            self.color.label(),
            self.graphics.label(),
            self.keyboard.label(),
            self.viewport.dimensions().0,
            self.viewport.dimensions().1
        )
    }
}

pub struct CapabilityMatrix {
    cells: Vec<CapabilityCell>,
    classified_by: CapabilityClassifier,
}
impl CapabilityMatrix {
    pub fn new(classifier: CapabilityClassifier) -> Self {
        let cells = ViewportCapability::all()
            .into_iter()
            .map(|viewport| CapabilityCell {
                color: classifier.color(),
                graphics: classifier.graphics(),
                keyboard: classifier.keyboard(),
                focus: classifier.focus(),
                notification: classifier.notification(),
                clipboard: classifier.clipboard(),
                title: classifier.title(),
                multiplexer: classifier.multiplexer(),
                platform: classifier.platform(),
                width: classifier.width(),
                motion: classifier.motion(),
                viewport,
            })
            .collect();
        Self {
            cells,
            classified_by: classifier,
        }
    }
    pub fn cells(&self) -> &[CapabilityCell] {
        &self.cells
    }
    pub fn len(&self) -> usize {
        self.cells.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
    pub fn for_viewport(&self, viewport: ViewportCapability) -> Option<&CapabilityCell> {
        self.cells.iter().find(|cell| cell.viewport == viewport)
    }
    pub fn all_classified(&self) -> bool {
        self.cells.iter().all(CapabilityCell::is_classified)
    }
    pub fn unclassified_combinations(&self) -> Vec<&CapabilityCell> {
        self.cells
            .iter()
            .filter(|cell| !cell.is_classified())
            .collect()
    }
    pub fn classified_by(&self) -> &CapabilityClassifier {
        &self.classified_by
    }
}
