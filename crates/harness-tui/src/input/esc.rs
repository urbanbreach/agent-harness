#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscLayer {
    Modal,
    ChildOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscAction {
    Dismiss(EscLayer),
    PassThrough,
}

#[derive(Debug, Default)]
pub struct EscRouter {
    layers: Vec<EscLayer>,
}

impl EscRouter {
    pub fn push(&mut self, layer: EscLayer) {
        self.layers.push(layer);
    }

    pub fn pop(&mut self) -> Option<EscLayer> {
        self.layers.pop()
    }

    pub fn handle(&mut self) -> EscAction {
        match self.pop() {
            Some(layer) => EscAction::Dismiss(layer),
            None => EscAction::PassThrough,
        }
    }

    pub fn len(&self) -> usize {
        self.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}
