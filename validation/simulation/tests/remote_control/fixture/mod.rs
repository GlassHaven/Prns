use std::cell::RefCell;
use std::future::Future;
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::time::Duration;

use personal_rns::engine::SendRequestFailure;
use personal_rns::identity::IdentityHash;
use personal_rns::remote_control::*;
use personal_rns::routing::links::LinkId;
use personal_rns::runtime::{
    NodeRunError, PrnsNodeHandle, RemoteControlInterfaceWatch, RemoteControlWatchReadError,
    SendError,
};
use prns_simulation::*;

mod control;
mod host;
mod node;
mod watch;
pub use control::encoded;
pub use node::Node;
pub use watch::watch_result;
pub mod inventory;

pub const TARGET: usize = 0;
pub const CONTROLLER: usize = 1;
pub const OUTSIDER: usize = 2;
const NODE_COUNT: usize = 3;
const MAX_ACTORS: usize = 20;
const POLL_BUDGET: usize = 8192;
pub const REQUEST_TIMEOUT_MS: u64 = 50;
const TRACE_CAPACITY: usize = 32_768;
const RECEIVE_FRAME_CAPACITY: usize = 64;
const PENDING_DELIVERY_CAPACITY: usize = 256;
pub(super) const MAX_APP_INVOCATIONS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInvocation {
    pub controller: IdentityHash,
    pub payload: Vec<u8>,
}

pub enum Event {
    Stopped {
        node: usize,
        result: Result<(), NodeRunError>,
    },
    Linked(LinkId),
    Response(Result<Vec<u8>, SendError<SendRequestFailure>>),
    Done,
    WatchOpened(RemoteControlInterfaceWatch),
    WatchRead {
        watch: RemoteControlInterfaceWatch,
        result: Result<RemoteControlStreamEvent, RemoteControlWatchReadError>,
    },
}

pub struct Lab<'a> {
    pub runner: ManualTaskRunner<'a, Event>,
    pub medium: VirtualMedium,
    pub nodes: Vec<Node>,
    pub calls: Rc<RefCell<Vec<AppInvocation>>>,
}

pub fn requests() -> RemoteControlRequestSet {
    let mut requests = RemoteControlRequestSet::only(RemoteControlRequestKind::Describe);
    for kind in [
        RemoteControlRequestKind::DescribeBuild,
        RemoteControlRequestKind::InventoryInterfaces,
        RemoteControlRequestKind::InventoryInterfaceConfig,
        RemoteControlRequestKind::InventoryInterfacePeers,
        RemoteControlRequestKind::AppMessage,
        RemoteControlRequestKind::WatchInterfaces,
    ] {
        requests.insert(kind);
    }
    requests
}

fn nonzero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("positive fixture capacity")
}
pub fn tick(value: u64) -> SimulationTick {
    SimulationTick::from_ticks(value)
}

pub enum LinkIdentity {
    Controller,
    Unidentified,
}

#[derive(Clone)]
pub enum ControllerPolicy {
    Granted(RemoteControlRequestSet),
    Nobody,
}

pub fn with_lab<R>(
    scheduling: ManualTaskScheduling,
    faults: FaultPlan,
    scenario: impl FnOnce(&mut Lab<'_>) -> R,
) -> (R, Vec<MediumEvent>) {
    with_policy(
        scheduling,
        faults,
        ControllerPolicy::Granted(requests()),
        scenario,
    )
}

pub fn with_policy<R>(
    scheduling: ManualTaskScheduling,
    faults: FaultPlan,
    policy: ControllerPolicy,
    scenario: impl FnOnce(&mut Lab<'_>) -> R,
) -> (R, Vec<MediumEvent>) {
    let medium = VirtualMedium::new(
        VirtualMediumConfig::new(
            TopologyConfig::Explicit {
                max_neighbors: nonzero(NODE_COUNT - 1),
            },
            NODE_COUNT,
            RECEIVE_FRAME_CAPACITY,
            PENDING_DELIVERY_CAPACITY,
            TRACE_CAPACITY,
            faults,
        )
        .expect("bounded control medium"),
    );
    let mut clock = ManualTimeDriver::new(
        ManualMedium::Frames(medium.clone()),
        Duration::from_millis(1),
    )
    .expect("manual control clock");
    let mut lab = Lab {
        runner: ManualTaskRunner::new_with_scheduling(&mut clock, nonzero(MAX_ACTORS), scheduling),
        medium,
        nodes: Vec::new(),
        calls: Rc::new(RefCell::new(Vec::new())),
    };
    for index in 0..NODE_COUNT {
        lab.start_node(
            index,
            match index {
                TARGET => policy.clone(),
                _ => ControllerPolicy::Nobody,
            },
        );
    }
    for index in [CONTROLLER, OUTSIDER] {
        lab.set_reachability(index, Reachability::Reachable);
    }
    let result = scenario(&mut lab);
    let nodes = std::mem::take(&mut lab.nodes);
    let tasks: Vec<_> = nodes.iter().map(|node| node.task).collect();
    for node in nodes {
        node.shutdown.send(()).expect("live shutdown receiver");
    }
    let stopped = lab.settle();
    assert_eq!(stopped.len(), NODE_COUNT);
    for (task, event) in stopped {
        let Event::Stopped { node, result } = event else {
            unreachable!("only node shutdowns remain");
        };
        assert_eq!(task, tasks[node]);
        assert_eq!(result, Ok(()));
    }
    assert_eq!(lab.runner.task_count(), 0);
    assert_eq!(lab.medium.pending_delivery_count(), 0);
    let trace = lab.medium.trace();
    assert_eq!(
        trace.discarded_events, 0,
        "bounded trace must remain complete"
    );
    let mut attached = Vec::new();
    let mut detached = Vec::new();
    for event in &trace.events {
        match event {
            MediumEvent::EndpointAttached { endpoint, .. } => attached.push(*endpoint),
            MediumEvent::EndpointDetached { endpoint } => detached.push(*endpoint),
            MediumEvent::ReceptionDropped {
                reason:
                    ReceptionDropReason::ReceiveQueueFull | ReceptionDropReason::PendingCapacityReached,
                ..
            } => unreachable!("fixture capacity loss"),
            _ => {}
        }
    }
    attached.sort();
    detached.sort();
    assert_eq!(attached, detached);
    (result, trace.events)
}

impl Lab<'_> {
    pub fn settle(&mut self) -> Vec<(ManualTaskId, Event)> {
        let mut completed = Vec::new();
        for _ in 0..POLL_BUDGET {
            match self.runner.poll_next().expect("manual control poll") {
                ManualTaskPoll::Idle => return completed,
                ManualTaskPoll::Pending { .. } => {}
                ManualTaskPoll::Completed { task, output } => completed.push((task, output)),
            }
        }
        unreachable!("control scenario exceeded {POLL_BUDGET} actor polls");
    }
    pub fn insert(&mut self, future: impl Future<Output = Event> + 'static) -> ManualTaskId {
        self.runner.insert(future).expect("bounded operation actor")
    }
    pub fn advance(&mut self, millis: u64) -> Vec<(ManualTaskId, Event)> {
        let mut completed = Vec::new();
        let end = self.medium.now().get() + millis;
        while self.medium.now().get() < end {
            self.runner
                .advance_to_next_event(tick(self.medium.now().get() + 1))
                .expect("coordinated control time");
            completed.extend(self.settle());
        }
        completed
    }
    pub fn set_reachability(&self, node: usize, reachability: Reachability) {
        assert_eq!(
            self.medium.set_reachability(
                self.nodes[TARGET].endpoint,
                self.nodes[node].endpoint,
                reachability
            ),
            Ok(TopologyMutation::Applied)
        );
    }
    pub fn expect_done(&mut self, task: ManualTaskId) {
        let completed = self.settle();
        assert_eq!(completed.len(), 1, "one local control completion");
        let (found, event) = &completed[0];
        assert_eq!(*found, task);
        assert!(matches!(event, Event::Done));
    }
}
