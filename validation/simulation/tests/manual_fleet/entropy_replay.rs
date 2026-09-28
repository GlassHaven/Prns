use super::*;
use personal_rns::manifold::tokio::TokioHost;
use personal_rns::runtime::PrnsNodeHandle;
use prns_core::entropy::{EntropySource, RuntimeEntropy};
use scenario::add_node_with_host;

// Isolated validation input, never a production entropy source or provisioning identity.
struct ScriptedSource {
    seed: u8,
    calls: Rc<RefCell<Vec<u8>>>,
}

impl EntropySource for ScriptedSource {
    type Error = core::convert::Infallible;

    fn try_fill_entropy(&mut self, output: &mut [u8]) -> Result<(), Self::Error> {
        self.calls.borrow_mut().push(self.seed);
        output.fill(self.seed);
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Transcript {
    trace: Vec<MediumEvent>,
    response: Vec<u8>,
    source_calls: Vec<u8>,
}

fn run(seed: u8, payload: &[u8]) -> Transcript {
    const NODES: usize = 2;
    let medium = VirtualMedium::new(
        VirtualMediumConfig::new(
            TopologyConfig::FullyConnected,
            NODES,
            16,
            32,
            256,
            FaultPlan::none(),
        )
        .unwrap_or_else(|error| unreachable!("bounded replay medium: {error}")),
    );
    let mut driver = ManualTimeDriver::new(
        ManualMedium::Frames(medium.clone()),
        Duration::from_millis(1),
    )
    .unwrap_or_else(|error| unreachable!("replay clock: {error}"));
    let mut runner = ManualTaskRunner::new(&mut driver, nonzero(NODES + 1));
    let source_calls = Rc::new(RefCell::new(Vec::new()));
    let mut nodes = Vec::new();
    let mut tasks = Vec::new();
    for index in 0..NODES {
        let interface = medium
            .attach(&(index as u64).to_be_bytes())
            .unwrap_or_else(|error| unreachable!("unique replay interface: {error}"));
        let calls = Rc::clone(&source_calls);
        let (task, mut ready) = add_node_with_host(
            &mut runner,
            NodeSpec {
                index,
                role: NodeRole::Endpoint,
                attach_interfaces: move |handle: &PrnsNodeHandle| {
                    let _attached = handle.add_interface(interface);
                },
                heard: Rc::new(RefCell::new(Vec::new())),
                heard_capacity: nonzero(1),
            },
            move |origin| {
                let source = ScriptedSource {
                    seed: seed
                        .checked_add(index as u8)
                        .unwrap_or_else(|| unreachable!("bounded replay seed")),
                    calls,
                };
                TokioHost::with_runtime_entropy(
                    origin,
                    RuntimeEntropy::try_new(source).unwrap_or_else(|never| match never {}),
                )
            },
        );
        assert!(settle(&mut runner).is_empty());
        tasks.push(task);
        nodes.push(
            ready
                .try_recv()
                .unwrap_or_else(|error| unreachable!("initialized replay node: {error}")),
        );
    }
    let remote = destination(1)
        .destination_hash()
        .unwrap_or_else(|error| unreachable!("replay destination: {error:?}"));
    announce(&nodes[1], remote);
    assert!(settle(&mut runner).is_empty());
    let handle = nodes[0].handle.clone();
    let linking = runner
        .insert(async move {
            Completion::Linked {
                node: 0,
                link: handle
                    .establish_link(remote)
                    .await
                    .unwrap_or_else(|error| unreachable!("replay link: {error:?}")),
            }
        })
        .unwrap_or_else(|error| unreachable!("bounded link actor: {error}"));
    let completed = settle(&mut runner);
    let [(task, Completion::Linked { node: 0, link })] = completed.as_slice() else {
        unreachable!("one link completion: {completed:?}")
    };
    assert_eq!(*task, linking);
    let requested = request(
        &mut runner,
        0,
        nodes[0].handle.clone(),
        *link,
        payload.to_vec(),
    );
    let mut completed = settle(&mut runner);
    assert_eq!(completed.len(), 1);
    let (
        task,
        Completion::Response {
            node: 0,
            bytes: response,
        },
    ) = completed.remove(0)
    else {
        unreachable!("one replay response")
    };
    assert_eq!((task, response.as_slice()), (requested, payload));
    for node in nodes {
        assert_eq!(node.shutdown.send(()), Ok(()));
    }
    let completed: BTreeMap<_, _> = settle(&mut runner).into_iter().collect();
    let expected = tasks
        .into_iter()
        .enumerate()
        .map(|(node, task)| {
            (
                task,
                Completion::Stopped {
                    node,
                    result: Ok(()),
                },
            )
        })
        .collect();
    assert_eq!(completed, expected);
    assert_eq!(
        (runner.task_count(), medium.pending_delivery_count()),
        (0, 0)
    );
    let trace = medium.trace();
    assert_eq!(trace.discarded_events, 0);
    let mut attached = Vec::new();
    let mut detached = Vec::new();
    for event in &trace.events {
        match event {
            MediumEvent::EndpointAttached { endpoint, .. } => attached.push(*endpoint),
            MediumEvent::EndpointDetached { endpoint } => detached.push(*endpoint),
            _ => {}
        }
    }
    attached.sort();
    detached.sort();
    assert_eq!(attached.len(), NODES);
    assert_eq!(attached, detached);
    let calls = source_calls.borrow().clone();
    assert_eq!(calls, [seed, seed + 1]);
    Transcript {
        trace: trace.events,
        response,
        source_calls: calls,
    }
}

#[test]
fn explicit_host_entropy_repeats_a_real_node_frame_exchange_byte_for_byte() {
    let first = run(0x31, b"controlled host source");
    assert!(first
        .trace
        .iter()
        .any(|event| matches!(event, MediumEvent::TransmissionAccepted { .. })));
    assert_eq!(first, run(0x31, b"controlled host source"));
    assert_eq!(first, run(0x31, b"controlled host source"));
}

#[test]
fn changed_host_entropy_and_application_input_change_the_frame_trace() {
    let first = run(0x31, b"one");
    assert_ne!(first.trace, run(0x41, b"one").trace);
    assert_ne!(first.trace, run(0x31, b"two").trace);
}
