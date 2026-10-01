use super::*;
use personal_rns::engine::InstantMillis;
use personal_rns::identity::vault::IdentitySecretKey;
use personal_rns::manifold::tokio::TokioHost;
use personal_rns::runtime::{
    CryptoPoolConfig, InterfaceArbitration, InterfaceEventSource, ManuallyAttached, NoPersistence,
    PrnsNode, PrnsNodeRecipe, RemoteControlNodeSetup, TokioHandleEntropy,
};
use personal_rns::storage::GrowableHeap;
use prns_core::entropy::RuntimeEntropy;
use tokio::sync::oneshot;

const TIMELINE_ORIGIN: InstantMillis = InstantMillis(1_000_000);

pub struct Node {
    pub handle: PrnsNodeHandle,
    pub endpoint: EndpointId,
    pub identity: RemoteControlControllerIdentity,
    pub target: RemoteControlTargetIdentity,
    pub(super) task: ManualTaskId,
    pub(super) shutdown: oneshot::Sender<()>,
}

fn secrets(node: usize) -> RemoteControlNodeIdentitySecrets {
    let base = 0x31 + node as u8 * 2;
    RemoteControlNodeIdentitySecrets::new(
        RemoteControlControllerIdentitySecret::from(IdentitySecretKey::new([base; 64])),
        RemoteControlTargetIdentitySecret::from(IdentitySecretKey::new([base + 1; 64])),
    )
    .expect("distinct validation identities")
}

impl Lab<'_> {
    pub(super) fn start_node(&mut self, index: usize, policy: ControllerPolicy) {
        let interface = self
            .medium
            .attach(&(index as u64).to_be_bytes())
            .expect("unique control interface");
        let endpoint = interface.endpoint_id();
        let secrets = secrets(index);
        let identity = *secrets.identities().controller();
        let target = RemoteControlTargetIdentity::new(*secrets.identities().target().public_keys());
        let calls = self.calls.clone();
        let (ready, mut ready_rx) = oneshot::channel();
        let (shutdown, stopping) = oneshot::channel();
        let task = self
            .runner
            .insert(async move {
                let host_seed = 0x80 + index as u8;
                let host_stream = RuntimeEntropy::try_new(move |output: &mut [u8]| {
                    output.fill(host_seed);
                    Ok::<(), core::convert::Infallible>(())
                })
                .expect("fixed fixture host entropy");
                let handle_stream = RuntimeEntropy::try_new(move |output: &mut [u8]| {
                    output.fill(host_seed + 8);
                    Ok::<(), core::convert::Infallible>(())
                })
                .expect("fixed fixture handle entropy");
                let entropy =
                    TokioHandleEntropy::from_sources(handle_stream, move |output: &mut [u8]| {
                        output.fill(host_seed + 16);
                        Ok::<(), core::convert::Infallible>(())
                    });
                let grants = match policy {
                    ControllerPolicy::Granted(requests) => vec![RemoteControlControllerGrant::new(
                        *self::secrets(CONTROLLER).identities().controller(),
                        RemoteControlControllerAuthority::Operator,
                        requests,
                    )
                    .expect("explicit control grant")],
                    ControllerPolicy::Nobody => vec![],
                };
                let node = PrnsNode::new_with_entropy_sources(
                    |handle| {
                        let initial = match grants.as_slice() {
                            [] => RemoteControlInitialControllerGrants::Nobody,
                            grants => RemoteControlInitialControllerGrants::Grants(
                                RemoteControlControllerGrants::try_from(grants)
                                    .expect("one controller"),
                            ),
                        };
                        let mut configured_requests = requests();
                        configured_requests.insert(RemoteControlRequestKind::DescribePower);
                        let capabilities =
                            RemoteControlCapabilities::from_requests(configured_requests)
                                .expect("Describe required");
                        PrnsNodeRecipe {
                            remote_control: RemoteControlNodeSetup::new(
                                RemoteControlService::with_capabilities(
                                    secrets,
                                    initial,
                                    RemoteControlSelfAnnouncement::Unavailable,
                                    capabilities,
                                ),
                            )
                            .with_handlers(host::InspectionHost(handle), host::Messages(calls)),
                            transport_identity: None,
                            pre_configured_destinations: []
                                as [personal_rns::runtime::PreConfiguredDestination<'static>; 0],
                            app_state: (),
                            storage: GrowableHeap,
                            request_endpoints: personal_rns::request_endpoints![],
                            on_event: |_, _: &()| {},
                            interfaces: ManuallyAttached,
                            persistence: NoPersistence,
                        }
                    },
                    TokioHost::with_runtime_entropy(TIMELINE_ORIGIN, host_stream),
                    entropy,
                )
                .with_crypto_pool(CryptoPoolConfig::Inline)
                .with_interface_arbitration(InterfaceArbitration::RoundRobin {
                    first: InterfaceEventSource::Message,
                });
                let handle = node.handle();
                let _attached = handle.add_interface(interface);
                assert!(ready.send(handle).is_ok(), "fixture ready receiver");
                Event::Stopped {
                    node: index,
                    result: node
                        .run_until(async {
                            let _ = stopping.await;
                        })
                        .await,
                }
            })
            .expect("bounded node actor");
        assert!(self.settle().is_empty());
        self.nodes.push(Node {
            handle: ready_rx.try_recv().expect("node initialized"),
            endpoint,
            identity,
            target,
            task,
            shutdown,
        });
    }
}
