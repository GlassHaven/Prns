use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

use crate::identity::IdentityHash;
use crate::interfaces::{
    ConnectionState, InterfaceGravity, InterfaceId, InterfaceMode, Membership, PeerDetails,
};
use crate::remote_control::{
    RemoteControlControllerGrantTable, RemoteControlRequestKind, RemoteControlStreamEvent,
    RemoteControlStreamEventError, REMOTE_CONTROL_STREAM_EVENT_LEN,
};
use crate::routing::links::channel::byte_stream::StreamId;
use crate::routing::links::LinkId;

use super::node_facade::PrnsNodeHandle;

const MAX_INTERFACE_WATCHES: usize = 8;
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WATCH_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const WATCH_WRITE_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatchReserveFailure {
    Full,
    Duplicate,
}

pub(super) enum WatchAdmission {
    Admitted,
    Withdrawn,
}

struct WatchLease;

pub(super) struct WatchReservation {
    link_id: LinkId,
    stream_id: StreamId,
    lease: Arc<WatchLease>,
    start: oneshot::Sender<()>,
}

impl WatchReservation {
    pub(super) fn start(self) {
        let _ = self.start.send(());
    }
}

struct Watch {
    lease: Arc<WatchLease>,
    controller: IdentityHash,
    stop: WatchCancellation,
    task: tokio::task::JoinHandle<()>,
}

enum WatchCancellation {
    Available(oneshot::Sender<()>),
    Requested,
}

impl Watch {
    fn stop(&mut self) {
        if let WatchCancellation::Available(stop) =
            std::mem::replace(&mut self.stop, WatchCancellation::Requested)
        {
            let _ = stop.send(());
        }
    }
}

pub(super) struct InterfaceWatchRegistry {
    watches: Mutex<HashMap<(LinkId, StreamId), Watch>>,
}

impl Default for InterfaceWatchRegistry {
    fn default() -> Self {
        Self {
            watches: Mutex::new(HashMap::new()),
        }
    }
}

impl InterfaceWatchRegistry {
    pub(super) fn admission(&self, reservation: &WatchReservation) -> WatchAdmission {
        let watches = self
            .watches
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match watches.get(&(reservation.link_id, reservation.stream_id)) {
            Some(watch)
                if Arc::ptr_eq(&watch.lease, &reservation.lease)
                    && matches!(watch.stop, WatchCancellation::Available(_)) =>
            {
                WatchAdmission::Admitted
            }
            _ => WatchAdmission::Withdrawn,
        }
    }
    pub(super) fn reserve(
        &self,
        node: PrnsNodeHandle,
        link_id: LinkId,
        stream_id: StreamId,
        controller: IdentityHash,
    ) -> Result<WatchReservation, WatchReserveFailure> {
        let mut watches = self
            .watches
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        watches.retain(|_, watch| !watch.task.is_finished());
        if watches.contains_key(&(link_id, stream_id)) {
            return Err(WatchReserveFailure::Duplicate);
        }
        if watches.len() >= MAX_INTERFACE_WATCHES {
            return Err(WatchReserveFailure::Full);
        }
        let lease = Arc::new(WatchLease);
        let (start, ready) = oneshot::channel();
        let (stop, mut cancellation) = oneshot::channel();
        let task = tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = &mut cancellation => {},
                started = ready => {
                    if started.is_ok() {
                        run_watch(node, link_id, stream_id, cancellation).await;
                    }
                }
            }
        });
        watches.insert(
            (link_id, stream_id),
            Watch {
                lease: lease.clone(),
                controller,
                stop: WatchCancellation::Available(stop),
                task,
            },
        );
        Ok(WatchReservation {
            link_id,
            stream_id,
            lease,
            start,
        })
    }

    pub(super) fn cancel(&self, reservation: &WatchReservation) {
        let mut watches = self
            .watches
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(watch) = watches.get_mut(&(reservation.link_id, reservation.stream_id)) {
            if Arc::ptr_eq(&watch.lease, &reservation.lease) {
                watch.stop();
            }
        }
    }

    pub(super) fn cancel_link(&self, link_id: LinkId) {
        let mut watches = self
            .watches
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for ((link, _), watch) in watches.iter_mut() {
            if *link == link_id {
                watch.stop();
            }
        }
    }

    pub(super) fn cancel_all(&self) {
        let mut watches = self
            .watches
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for watch in watches.values_mut() {
            watch.stop();
        }
    }

    pub(super) fn reconcile_grants(&self, grants: &impl RemoteControlControllerGrantTable) {
        let mut watches = self
            .watches
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for watch in watches.values_mut() {
            if !grants.grant_for(&watch.controller).is_some_and(|grant| {
                grant
                    .effective_requests()
                    .supports(RemoteControlRequestKind::WatchInterfaces)
            }) {
                watch.stop();
            }
        }
    }
}

impl Drop for InterfaceWatchRegistry {
    fn drop(&mut self) {
        // Node shutdown cannot await stream cleanup. Abort instead of detaching workers.
        for watch in self
            .watches
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .values_mut()
        {
            watch.task.abort();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StableInterface {
    id: InterfaceId,
    mode: InterfaceMode,
    gravity: InterfaceGravity,
    destinations: u32,
    links: u32,
    transported_links: u32,
    connection: ConnectionState,
    failure_reason: Option<&'static str>,
    membership: Membership,
    details: PeerDetails,
}

fn stable_interfaces(node: &PrnsNodeHandle) -> Vec<StableInterface> {
    let mut snapshots = node
        .interfaces()
        .into_iter()
        .map(|snapshot| StableInterface {
            id: snapshot.id,
            mode: snapshot.mode,
            gravity: snapshot.gravity,
            destinations: snapshot.destinations,
            links: snapshot.links,
            transported_links: snapshot.transported_links,
            connection: snapshot.connection,
            failure_reason: snapshot.failure_reason,
            membership: snapshot.membership,
            details: snapshot.details,
        })
        .collect::<Vec<_>>();
    snapshots.sort_unstable_by_key(|snapshot| *snapshot.id.as_bytes());
    snapshots
}

#[derive(Debug)]
enum WatchSendError {
    Cancelled,
    Frame(RemoteControlStreamEventError),
    Io(std::io::Error),
    Deadline,
}

impl std::fmt::Display for WatchSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("interface watch cancelled"),
            Self::Frame(error) => write!(f, "interface watch encoding failed: {error:?}"),
            Self::Io(error) => write!(f, "interface watch write failed: {error}"),
            Self::Deadline => f.write_str("interface watch write deadline exceeded"),
        }
    }
}

async fn send_event(
    writer: &mut super::node_facade::ByteStreamWriter,
    event: RemoteControlStreamEvent,
    cancellation: &mut oneshot::Receiver<()>,
) -> Result<(), WatchSendError> {
    let mut frame = [0; REMOTE_CONTROL_STREAM_EVENT_LEN];
    event
        .write_into(&mut frame)
        .map_err(WatchSendError::Frame)?;
    tokio::select! {
        biased;
        _ = cancellation => Err(WatchSendError::Cancelled),
        sent = tokio::time::timeout(WATCH_WRITE_DEADLINE, writer.write_all(&frame)) => {
            sent.map_err(|_| WatchSendError::Deadline)?.map_err(WatchSendError::Io)
        }
    }
}

async fn run_watch(
    node: PrnsNodeHandle,
    link_id: LinkId,
    stream_id: StreamId,
    mut cancellation: oneshot::Receiver<()>,
) {
    let mut writer = node.byte_stream_writer(link_id, stream_id);
    let result = run_watch_events(&node, &mut writer, &mut cancellation).await;
    if let Err(_error) = result {
        #[cfg(feature = "tracing")]
        tracing::debug!(target: "prns.runtime", event = "interface_watch_stopped", ?link_id, ?stream_id, error = %_error);
    }
    // Cancellation and write failure both finish the stream, within a bounded deadline.
    match tokio::time::timeout(WATCH_WRITE_DEADLINE, writer.shutdown()).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) | Err(_) => {
            node.close_link(link_id);
        }
    }
}

async fn run_watch_events(
    node: &PrnsNodeHandle,
    writer: &mut super::node_facade::ByteStreamWriter,
    cancellation: &mut oneshot::Receiver<()>,
) -> Result<(), WatchSendError> {
    let mut sequence = 1;
    // Capture before invalidation: changes during the initial send must remain visible.
    let mut previous = stable_interfaces(node);
    send_event(
        writer,
        RemoteControlStreamEvent::ResyncRequired { sequence },
        cancellation,
    )
    .await?;
    let mut last_sent = Instant::now();
    let mut interval = tokio::time::interval(WATCH_POLL_INTERVAL);
    interval.tick().await;
    loop {
        tokio::select! {
            biased;
            _ = &mut *cancellation => return Err(WatchSendError::Cancelled),
            _ = interval.tick() => {
                let current = stable_interfaces(node);
                let event = if current != previous {
                    previous = current;
                    Some(RemoteControlStreamEvent::ResyncRequired {
                        sequence: sequence.wrapping_add(1),
                    })
                } else if last_sent.elapsed() >= WATCH_HEARTBEAT_INTERVAL {
                    Some(RemoteControlStreamEvent::Heartbeat {
                        sequence: sequence.wrapping_add(1),
                    })
                } else {
                    None
                };
                if let Some(event) = event {
                    sequence = event.sequence();
                    send_event(writer, event, cancellation).await?;
                    last_sent = Instant::now();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        DeliveryEvidence, DeliveryProof, PacketReceiptDelivered, PrnsCommand, Settlement,
    };
    use crate::manifold::driver::HostCommand;
    use crate::remote_control::FixedRemoteControlControllerGrantTable;
    use crate::routing::dedup::PacketHash;
    use crate::routing::links::channel::byte_stream::parse;
    use crate::units::RttMillis;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn revoked_pending_watch_cannot_start_and_old_reservation_cannot_cancel_its_replacement()
    {
        let (commands, mut receiver) = mpsc::unbounded_channel();
        let node = PrnsNodeHandle::over(commands);
        let watches = InterfaceWatchRegistry::default();
        let link_id = LinkId::new([7; 16]);
        let stream_id = StreamId::new(3).unwrap();
        let controller = IdentityHash::new([8; 16]);
        let original = watches
            .reserve(node.clone(), link_id, stream_id, controller)
            .unwrap();
        watches.reconcile_grants(&FixedRemoteControlControllerGrantTable::<8>::default());
        tokio::task::yield_now().await;
        let replacement = watches
            .reserve(node, link_id, stream_id, controller)
            .unwrap();
        assert!(matches!(
            watches.admission(&original),
            WatchAdmission::Withdrawn
        ));
        watches.cancel(&original);
        assert!(matches!(
            watches.admission(&replacement),
            WatchAdmission::Admitted
        ));
        original.start();
        tokio::task::yield_now().await;
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn blocked_writer_finishes_within_the_cleanup_deadline() {
        let (commands, mut receiver) = mpsc::unbounded_channel();
        let node = PrnsNodeHandle::over(commands);
        let watches = InterfaceWatchRegistry::default();
        let link_id = LinkId::new([7; 16]);
        let stream_id = StreamId::new(3).unwrap();
        watches
            .reserve(node, link_id, stream_id, IdentityHash::new([8; 16]))
            .unwrap()
            .start();
        let initial = receiver.recv().await.unwrap(); // Keep the settlement pending.
        watches.cancel_link(link_id);
        tokio::task::yield_now().await;
        tokio::time::advance(WATCH_WRITE_DEADLINE).await;
        tokio::task::yield_now().await;
        assert!(watches
            .watches
            .lock()
            .unwrap()
            .values()
            .all(|watch| watch.task.is_finished()));
        assert!(matches!(
            receiver.recv().await,
            Some(HostCommand::Engine(crate::engine::IssuedCommand {
                command: PrnsCommand::CloseLink(_),
                ..
            }))
        ));
        drop(initial);
    }

    #[tokio::test]
    async fn watch_registry_bounds_subscriptions_and_releases_revoked_slots() {
        let (commands, _receiver) = mpsc::unbounded_channel();
        let node = PrnsNodeHandle::over(commands);
        let watches = InterfaceWatchRegistry::default();
        let link_id = LinkId::new([7; 16]);
        let controller = IdentityHash::new([8; 16]);
        let mut reservations = Vec::new();
        for index in 0..MAX_INTERFACE_WATCHES {
            let stream_id = StreamId::new(index as u16).unwrap();
            reservations.push(
                watches
                    .reserve(node.clone(), link_id, stream_id, controller)
                    .unwrap(),
            );
        }
        assert!(matches!(
            watches.reserve(node.clone(), link_id, StreamId::new(0).unwrap(), controller),
            Err(WatchReserveFailure::Duplicate),
        ));
        assert!(matches!(
            watches.reserve(node.clone(), link_id, StreamId::new(9).unwrap(), controller),
            Err(WatchReserveFailure::Full),
        ));
        let grants = FixedRemoteControlControllerGrantTable::<8>::default();
        watches.reconcile_grants(&grants);
        // Retiring workers still occupy capacity until they actually stop.
        assert!(matches!(
            watches.reserve(node.clone(), link_id, StreamId::new(9).unwrap(), controller),
            Err(WatchReserveFailure::Full)
        ));
        tokio::task::yield_now().await;
        let replacement = watches
            .reserve(node.clone(), link_id, StreamId::new(9).unwrap(), controller)
            .unwrap();
        watches.cancel_link(link_id);
        let another_link = LinkId::new([9; 16]);
        assert!(watches
            .reserve(node, another_link, StreamId::new(9).unwrap(), controller)
            .is_ok());
        drop(replacement);
        drop(reservations);
    }

    #[tokio::test]
    async fn closing_a_link_ends_its_watch_stream() {
        let (commands, mut receiver) = mpsc::unbounded_channel();
        let node = PrnsNodeHandle::over(commands);
        let watches = InterfaceWatchRegistry::default();
        let link_id = LinkId::new([0x41; 16]);
        let stream_id = StreamId::new(0x123).unwrap();
        watches
            .reserve(node, link_id, stream_id, IdentityHash::new([0x42; 16]))
            .unwrap()
            .start();
        let Some(HostCommand::AwaitedEngine { issued, completion }) =
            tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
        else {
            panic!("initial stream event");
        };
        let PrnsCommand::SendToChannel(initial) = issued.command else {
            panic!("initial stream frame");
        };
        assert!(!parse(initial.body.as_slice()).unwrap().header.eof);
        completion
            .send(Settlement::SendToChannel(Ok(PacketReceiptDelivered {
                rtt: RttMillis::new(0),
                evidence: DeliveryEvidence::Proof(DeliveryProof::Implicit(PacketHash::new(
                    [0; 32],
                ))),
            })))
            .unwrap();
        watches.cancel_link(link_id);
        let Some(HostCommand::AwaitedEngine { issued, .. }) =
            tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
        else {
            panic!("watch stream closes with EOF");
        };
        let PrnsCommand::SendToChannel(closing) = issued.command else {
            panic!("stream close frame");
        };
        assert!(parse(closing.body.as_slice()).unwrap().header.eof);
    }
}
