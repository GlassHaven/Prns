use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard};

use personal_rns::wire::WirePacketHeader;
use tokio::sync::Notify;

mod backend;
#[cfg(test)]
mod tests;

pub(super) use backend::GatedBackend;

enum State {
    Idle,
    Armed {
        header: WirePacketHeader,
        remaining: NonZeroUsize,
    },
    Held(WirePacketHeader),
    Released,
}

struct Inner {
    state: Mutex<State>,
    reached: Notify,
    released: Notify,
}

// One held send per radio, with no copied payload or queued observations. The
// sink retains its original frame; cancellation releases the gate's ownership.
#[derive(Clone)]
pub(super) struct WireGate(Arc<Inner>);

impl WireGate {
    pub(super) fn new() -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State::Idle),
            reached: Notify::new(),
            released: Notify::new(),
        }))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap()
    }

    pub(super) fn arm(&self, header: WirePacketHeader, occurrence: NonZeroUsize) {
        let mut state = self.state();
        assert!(
            matches!(*state, State::Idle),
            "one armed wire gate per radio"
        );
        *state = State::Armed {
            header,
            remaining: occurrence,
        };
    }

    pub(super) fn is_idle(&self) -> bool {
        matches!(*self.state(), State::Idle)
    }

    pub(super) async fn held(&self) -> WirePacketHeader {
        loop {
            let reached = self.0.reached.notified();
            match *self.state() {
                State::Held(header) => return header,
                State::Idle | State::Armed { .. } => {}
                State::Released => unreachable!("observe a held frame before releasing it"),
            }
            reached.await;
        }
    }

    pub(super) fn release(&self) {
        let mut state = self.state();
        assert!(
            matches!(*state, State::Held(_)),
            "release only a held frame"
        );
        *state = State::Released;
        self.0.released.notify_one();
    }

    fn hold(&self, frame: &[u8]) -> Option<HeldSend<'_>> {
        let (observed, _) = WirePacketHeader::parse(frame).ok()?;
        let mut state = self.state();
        let State::Armed { header, remaining } = &mut *state else {
            return None;
        };
        if observed != *header {
            return None;
        }
        if let Some(next) = NonZeroUsize::new(remaining.get() - 1) {
            *remaining = next;
            return None;
        }
        *state = State::Held(observed);
        self.0.reached.notify_one();
        Some(HeldSend(self))
    }

    async fn before_send(&self, frame: &[u8]) {
        let Some(_held) = self.hold(frame) else {
            return;
        };
        loop {
            let released = self.0.released.notified();
            if matches!(*self.state(), State::Released) {
                return;
            }
            released.await;
        }
    }
}

struct HeldSend<'a>(&'a WireGate);

impl Drop for HeldSend<'_> {
    fn drop(&mut self) {
        *self.0.state() = State::Idle;
    }
}
