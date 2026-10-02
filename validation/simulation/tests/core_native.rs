#![allow(clippy::unwrap_used)]
use personal_rns::{
    identity::vault::IdentitySecretKey, interfaces::*, remote_control::*, runtime::*,
};
use prns_runtime_tokio::manifold::interface_seam::{Interface, InterfaceSeam};
use std::{num::NonZeroUsize, time::Duration};
const FRAMES: usize = 64;
const DEADLINE: Duration = Duration::from_secs(10);
struct Loopback {
    id: InterfaceId,
    inbound: tokio::sync::mpsc::Receiver<Vec<u8>>,
    outbound: tokio::sync::mpsc::Sender<Vec<u8>>,
}
impl Interface for Loopback {
    const HW_MTU: usize = personal_rns::wire::BROADCAST_MTU;
    const KIND: InterfaceKind = InterfaceKind::Loopback;
    fn channel_tag(&self) -> &[u8] {
        self.id.as_bytes()
    }
    fn descriptor(&self) -> InterfaceDescriptor {
        InterfaceDescriptor {
            id: self.id,
            capabilities: InterfaceCapabilities {
                ingress: IngressCapability::Enabled,
                egress: EgressCapability::Enabled(TransportCapability::CrossInterfaceOnly),
            },
            mode: InterfaceMode::Full,
            gravity: InterfaceGravity::ZERO,
            bitrate: BitrateBps::guess(1_000_000),
            hardware_mtu: None,
            announce_rate_limit: None,
            announce_bandwidth_cap: AnnounceBandwidthCap::Unlimited,
            airtime_duty_cycle: None,
            common: InterfaceCommonPolicy::RNS_DEFAULT,
        }
    }
    async fn run<S: InterfaceSeam>(mut self, mut seam: S) {
        loop {
            tokio::select! { frame = self.inbound.recv() => match frame { Some(bytes) => seam.next_inbound(&bytes).await, None => return }, frame = seam.next_outbound() => { if self.outbound.send(frame.to_vec()).await.is_err() { return; } } }
        }
    }
}
impl ReportsStatus for Loopback {}
struct Echo {
    expected: personal_rns::identity::IdentityHash,
}
impl RemoteControlAppMessages<()> for Echo {
    async fn handle_app_message(
        &self,
        _: &(),
        identity: personal_rns::identity::IdentityHash,
        bytes: &[u8],
    ) -> Result<RemoteControlAppMessage, RemoteControlHostCommandError> {
        assert_eq!(identity, self.expected);
        RemoteControlAppMessage::from_slice(bytes)
            .map_err(|_| RemoteControlHostCommandError::ApplyFailed)
    }
}
fn secrets(index: u8) -> RemoteControlNodeIdentitySecrets {
    RemoteControlNodeIdentitySecrets::new(
        RemoteControlControllerIdentitySecret::from(IdentitySecretKey::new([0x71 + index * 2; 64])),
        RemoteControlTargetIdentitySecret::from(IdentitySecretKey::new([0x72 + index * 2; 64])),
    )
    .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_one_and_four_worker_nodes_deliver_verified_remote_control_requests() {
    use personal_rns::engine::{AnnounceAppData, AnnounceNow, AnnounceTarget};
    for workers in [1, 4] {
        let controller = secrets(0).identities().controller().identity_hash();
        let grants = [RemoteControlControllerGrant::new(
            *secrets(0).identities().controller(),
            RemoteControlControllerAuthority::Operator,
            RemoteControlRequestSet::only(RemoteControlRequestKind::AppMessage),
        )
        .unwrap()];
        let make = |index| {
            PrnsNode::new(PrnsNodeRecipe {
                remote_control: RemoteControlNodeSetup::new(
                    RemoteControlService::with_capabilities(
                        secrets(index),
                        RemoteControlInitialControllerGrants::Grants(
                            RemoteControlControllerGrants::try_from(grants.as_slice()).unwrap(),
                        ),
                        RemoteControlSelfAnnouncement::Unavailable,
                        RemoteControlCapabilities::describe_only()
                            .with_request(RemoteControlRequestKind::AppMessage),
                    ),
                )
                .with_handlers(
                    NoRemoteControlHostControls,
                    Echo {
                        expected: controller,
                    },
                ),
                transport_identity: None,
                pre_configured_destinations: [],
                app_state: (),
                storage: personal_rns::storage::GrowableHeap,
                request_endpoints: personal_rns::request_endpoints![],
                interfaces: ManuallyAttached,
                persistence: NoPersistence,
                on_event: |_, _: &()| {},
            })
            .with_crypto_pool(CryptoPoolConfig::Pooled {
                workers: PoolWorkers::Fixed(NonZeroUsize::new(workers).unwrap()),
                placement: CryptoWorkerPlacement::SchedulerManaged,
            })
        };
        let left = make(0);
        let right = make(1);
        let left_handle = left.handle();
        let right_handle = right.handle();
        let (left_tx, left_rx) = tokio::sync::mpsc::channel(FRAMES);
        let (right_tx, right_rx) = tokio::sync::mpsc::channel(FRAMES);
        left_handle.add_interface(Loopback {
            id: InterfaceId::from_channel_tag(InterfaceKind::Loopback, b"native-left"),
            inbound: left_rx,
            outbound: right_tx,
        });
        right_handle.add_interface(Loopback {
            id: InterfaceId::from_channel_tag(InterfaceKind::Loopback, b"native-right"),
            inbound: right_rx,
            outbound: left_tx,
        });
        let (stop_left, stopped_left) = tokio::sync::oneshot::channel();
        let (stop_right, stopped_right) = tokio::sync::oneshot::channel();
        let exercise = async move {
            let target = secrets(1)
                .identities()
                .target()
                .endpoint()
                .destination_hash();
            right_handle
                .announce_now(AnnounceNow {
                    destination: target,
                    target: AnnounceTarget::AllInterfaces,
                    app_data: AnnounceAppData::Registered,
                })
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            let link = left_handle.establish_link(target).await.unwrap();
            left_handle.identify(link, controller).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            for marker in 0..16 {
                let payload = RemoteControlAppMessage::from_slice(&[marker; 96]).unwrap();
                assert_eq!(
                    left_handle
                        .remote_control(link)
                        .app_message(payload.clone())
                        .await
                        .unwrap()
                        .0,
                    payload
                );
            }
            stop_left.send(()).unwrap();
            stop_right.send(()).unwrap();
        };
        let bounded = tokio::time::timeout(DEADLINE, exercise);
        let (left_result, right_result, result) = tokio::join!(
            left.run_until(async {
                let _ = stopped_left.await;
            }),
            right.run_until(async {
                let _ = stopped_right.await;
            }),
            bounded
        );
        result.unwrap();
        assert!(left_result.is_ok());
        assert!(right_result.is_ok());
    }
}
