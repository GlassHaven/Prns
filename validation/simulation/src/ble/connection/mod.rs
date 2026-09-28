use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use super::BleAddress;

mod data;
mod index;

pub(in crate::ble) use data::ConnectionSide;
pub(in crate::ble) use data::DataEvent;
pub use data::{BleConnectionDataSnapshot, BleDataCounters};

pub(super) use index::ConnectionIndex;

pub(super) struct Connection {
    addresses: [BleAddress; 2],
    closed: watch::Sender<bool>,
    data: Mutex<[BleDataCounters; 2]>,
}

impl Connection {
    pub(super) fn new(first: BleAddress, second: BleAddress) -> Self {
        let (closed, _) = watch::channel(false);
        Self {
            addresses: [first, second],
            closed,
            data: Mutex::new([BleDataCounters::default(); 2]),
        }
    }

    pub(super) fn connects(&self, address: BleAddress) -> bool {
        self.addresses.contains(&address)
    }

    pub(in crate::ble) fn data_snapshot(&self) -> BleConnectionDataSnapshot {
        let data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        BleConnectionDataSnapshot {
            dialer: self.addresses[0],
            listener: self.addresses[1],
            dialer_to_listener: data[0],
            listener_to_dialer: data[1],
        }
    }

    pub(super) fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    pub(super) fn subscribe(&self) -> watch::Receiver<bool> {
        self.closed.subscribe()
    }

    pub(super) fn close(&self) -> bool {
        self.closed.send_if_modified(|closed| {
            let changed = !*closed;
            *closed = true;
            changed
        })
    }
}

pub(super) struct ConnectionEndpoint {
    pub(super) connection: Arc<Connection>,
    pub(super) side: ConnectionSide,
}

impl ConnectionEndpoint {
    pub(in crate::ble) fn outgoing(&self, event: DataEvent) {
        self.connection
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[self.side.outgoing()]
        .record(event);
    }

    pub(in crate::ble) fn incoming(&self, event: DataEvent) {
        self.connection
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[self.side.incoming()]
        .record(event);
    }
}

impl Drop for ConnectionEndpoint {
    fn drop(&mut self) {
        let _ = self.connection.close();
    }
}
