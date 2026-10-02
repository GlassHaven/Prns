use super::*;
use personal_rns::identity::vault::IdentitySecretKey;
use personal_rns::manifold::tokio::TokioHost;
use prns_core::entropy::RuntimeEntropy;

pub fn secrets(index: usize) -> RemoteControlNodeIdentitySecrets {
    RemoteControlNodeIdentitySecrets::new(
        RemoteControlControllerIdentitySecret::from(IdentitySecretKey::new(
            [0x41 + index as u8 * 2; 64],
        )),
        RemoteControlTargetIdentitySecret::from(IdentitySecretKey::new(
            [0x42 + index as u8 * 2; 64],
        )),
    )
    .expect("distinct validation keys")
}
pub fn target(index: usize) -> RemoteControlTargetIdentity {
    RemoteControlTargetIdentity::new(*secrets(index).identities().target().public_keys())
}
pub fn controller(index: usize) -> IdentityHash {
    secrets(index).identities().controller().identity_hash()
}
pub fn supervisor(
    radio: VirtualHaLowRadio,
    index: usize,
) -> personal_rns::wifi_halow::HaLow<VirtualHaLowRadio> {
    personal_rns::wifi_halow::HaLow::new(
        radio,
        scope(index),
        policy_for_bitrate(BitrateBps::guess(7_300_000)),
        policy_for_bitrate(BitrateBps::guess(4_000_000)),
        personal_rns::wifi_halow::HaLowLimits {
            peers: NonZeroU8::new(16).expect("peer budget"),
            idle_seconds: NonZeroU32::new(IDLE_SECONDS).expect("idle budget"),
        },
    )
}
impl Lab<'_> {
    pub(super) fn start_node(&mut self, index: usize) {
        let radio = self.medium.attach(mac(index)).expect("unique radio");
        let radio_id = radio.id();
        let generation = self.generations[index];
        self.generations[index] = generation + 1;
        let origin = personal_rns::engine::InstantMillis(1_000_000 + self.medium.now().get());
        let calls = self.calls.clone();
        let announces = self.announces.clone();
        let (ready, mut ready_rx) = oneshot::channel();
        let (shutdown, stopping) = oneshot::channel();
        let task = self.insert(async move {
            let entropy = |domain: u8| {
                RuntimeEntropy::try_new(move |bytes: &mut [u8]| {
                    for (offset, byte) in bytes.iter_mut().enumerate() {
                        *byte = domain ^ (index as u8 * 17) ^ generation.to_be_bytes()[offset % 8];
                    }
                    Ok::<(), core::convert::Infallible>(())
                })
                .expect("fixed validation entropy")
            };
            let grants: Vec<_> = [PRIMARY, HEALTHY]
                .iter()
                .map(|controller| {
                    RemoteControlControllerGrant::new(
                        *secrets(*controller).identities().controller(),
                        RemoteControlControllerAuthority::Operator,
                        permissions(),
                    )
                    .expect("explicit grant")
                })
                .collect();
            let mut node = PrnsNode::new_with_entropy_sources(
                |handle| PrnsNodeRecipe {
                    remote_control: RemoteControlNodeSetup::new(
                        RemoteControlService::with_capabilities(
                            secrets(index),
                            RemoteControlInitialControllerGrants::Grants(
                                RemoteControlControllerGrants::try_from(grants.as_slice())
                                    .expect("bounded grants"),
                            ),
                            RemoteControlSelfAnnouncement::Unavailable,
                            RemoteControlCapabilities::from_requests(permissions())
                                .expect("describe capability"),
                        ),
                    )
                    .with_handlers(host::InspectionHost(handle), host::Messages { calls }),
                    transport_identity: Some(personal_rns::identity::Zeroizing::new(
                        [0x91 + index as u8; 64],
                    )),
                    pre_configured_destinations: [],
                    app_state: (),
                    storage: personal_rns::storage::GrowableHeap,
                    request_endpoints: personal_rns::request_endpoints![
                        crate::traffic::ResourceReply
                    ],
                    on_event: move |event, _: &()| {
                        if let PrnsEvent::Diagnostic(Diagnostic::AnnounceHeard {
                            destination,
                            ..
                        }) = event
                        {
                            let mut observed = announces.borrow_mut();
                            assert!(observed.len() < 4096, "bounded announce observations");
                            observed.push(AnnounceSeen {
                                node: index,
                                destination,
                            });
                        }
                    },
                    interfaces: ManuallyAttached,
                    persistence: NoPersistence,
                },
                TokioHost::with_runtime_entropy(origin, entropy(0xa1)),
                TokioHandleEntropy::from_sources(entropy(0xb1), move |bytes: &mut [u8]| {
                    for (offset, byte) in bytes.iter_mut().enumerate() {
                        *byte = 0xc1 ^ (index as u8 * 17) ^ generation.to_be_bytes()[offset % 8];
                    }
                    Ok::<(), core::convert::Infallible>(())
                }),
            )
            .with_crypto_pool(CryptoPoolConfig::Inline)
            .with_interface_arbitration(InterfaceArbitration::RoundRobin {
                first: InterfaceEventSource::Message,
            });
            let handle = node.handle();
            node.register_request_route::<crate::traffic::ResourceReply>(
                &target(index).endpoint().destination_hash(),
            )
            .expect("resource route");
            let attached = handle.supervise(supervisor(radio, index));
            ready
                .send((handle, attached))
                .unwrap_or_else(|_| panic!("ready receiver"));
            Event::Stopped {
                node: index,
                result: node
                    .run_until(async {
                        let _ = stopping.await;
                    })
                    .await,
            }
        });
        assert!(self.settle().is_empty());
        let (handle, supervisor) = ready_rx.try_recv().expect("initialized node");
        self.nodes.push(Node {
            handle,
            radio: radio_id,
            adapter: AdapterState::Attached(supervisor),
            task,
            shutdown,
        });
    }
    pub fn replace_adapter(&mut self, index: usize) {
        self.detach_adapter(index);
        let radio = self.medium.attach(mac(index)).expect("replacement radio");
        self.nodes[index].radio = radio.id();
        self.nodes[index].adapter =
            AdapterState::Attached(self.nodes[index].handle.supervise(supervisor(radio, index)));
        assert!(self.settle().is_empty());
    }
    pub fn detach_adapter(&mut self, index: usize) {
        if let AdapterState::Attached(attached) =
            std::mem::replace(&mut self.nodes[index].adapter, AdapterState::Detached)
        {
            attached.teardown();
        }
        assert!(self.settle().is_empty());
    }
    pub fn restart(&mut self, index: usize) {
        self.runner
            .cancel(self.nodes[index].task)
            .expect("retire old node actor");
        self.start_node(index);
        let last = self.nodes.len() - 1;
        self.nodes.swap(index, last);
        self.nodes.pop().expect("discard old handles");
    }
}
