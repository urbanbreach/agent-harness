use super::*;
use std::sync::Arc;

pub(crate) fn observer(store: Arc<dyn EventStore>) -> Arc<dyn EventStore> {
    Arc::new(Observer(store))
}

// Keep the public stream contract while confining writes to the coordinator.
struct Observer(Arc<dyn EventStore>);
impl EventStore for Observer {
    fn append(&self, _: EventEnvelopeWithoutSeqV1) -> Result<EventEnvelopeV1, EventStoreError> {
        Err(EventStoreError::Invalid("event observer is read-only"))
    }
    fn append_applied(
        &self,
        _: EventEnvelopeWithoutSeqV1,
        _: &mut dyn FnMut(&EventEnvelopeV1),
    ) -> Result<EventEnvelopeV1, EventStoreError> {
        Err(EventStoreError::Invalid("event observer is read-only"))
    }
    fn replay(&self, seq: u64) -> Result<EventStream, EventStoreError> {
        self.0.replay(seq)
    }
    fn subscribe(&self, seq: u64) -> Result<EventStream, EventStoreError> {
        self.0.subscribe(seq)
    }
    fn subscribe_runtime(&self, seq: u64) -> Result<RuntimeEventStream, EventStoreError> {
        self.0.subscribe_runtime(seq)
    }
    fn publish_live(&self, _: LiveEventEnvelope) {
        // The legacy signature cannot report rejection; observers cannot publish.
    }
    fn close_writer(&self) -> Result<(), EventStoreError> {
        Err(EventStoreError::Invalid("event observer is read-only"))
    }
}
