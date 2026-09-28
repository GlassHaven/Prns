use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

use super::BleAddress;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleWireChannel {
    Control,
    Data,
}

/// A value accepted by a virtual characteristic queue, not a delivery or RF observation.
/// Addresses identify peers, not connection incarnations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleWireValue {
    pub from: BleAddress,
    pub to: BleAddress,
    pub channel: BleWireChannel,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleWireSnapshot {
    pub discarded_values: u64,
    pub values: Vec<BleWireValue>,
}

struct Buffer {
    capacity: usize,
    discarded: u64,
    values: VecDeque<BleWireValue>,
}

/// Explicit, bounded payload retention for isolated replay scenarios.
/// Disabled labs do not allocate this buffer or copy wire values.
///
/// Capacity is in whole characteristic values; the backend bounds each value by
/// its negotiated control/data limit. Oldest values are evicted with an explicit
/// discard count. Snapshots clone retained payloads, so callers must also bound
/// their snapshot retention. Order is capture order, not a global enqueue order
/// across concurrent executors; deterministic replay uses the serial manual runner.
#[derive(Clone)]
pub struct BleWireCapture {
    buffer: Arc<Mutex<Buffer>>,
}

impl BleWireCapture {
    #[must_use]
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Buffer {
                capacity: capacity.get(),
                discarded: 0,
                values: VecDeque::new(),
            })),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> BleWireSnapshot {
        let buffer = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        BleWireSnapshot {
            discarded_values: buffer.discarded,
            values: buffer.values.iter().cloned().collect(),
        }
    }

    pub(super) fn record(
        &self,
        from: BleAddress,
        to: BleAddress,
        channel: BleWireChannel,
        bytes: &[u8],
    ) {
        let mut buffer = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buffer.values.len() == buffer.capacity {
            let _ = buffer.values.pop_front();
            buffer.discarded = buffer.discarded.saturating_add(1);
        }
        buffer.values.push_back(BleWireValue {
            from,
            to,
            channel,
            bytes: bytes.to_vec(),
        });
    }
}

#[cfg(test)]
mod tests;
