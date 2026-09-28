use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::mpsc;

use super::*;
use crate::{
    engine::CommandTiming, manifold::driver::HostCommand, runtime::BitrateTimingOracle,
    units::HopCount,
};

const PEER: DestinationHash = DestinationHash::new([0xAB; 16]);

struct TimingProbe(Arc<AtomicUsize>);

impl BitrateTimingOracle for TimingProbe {
    fn first_hop_timeout(
        &self,
        _: DestinationHash,
    ) -> Pin<Box<dyn Future<Output = Option<Duration>> + Send + '_>> {
        Box::pin(async { None })
    }

    fn medium_path_timeout(&self) -> Pin<Box<dyn Future<Output = Option<Duration>> + Send + '_>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Some(Duration::from_millis(1234)) })
    }
}

#[tokio::test]
async fn a_partial_source_failure_never_reaches_timing_or_command_admission() {
    let (commands, mut receiver) = mpsc::unbounded_channel();
    let handle = PrnsNodeHandle::over(commands);
    let timing_calls = Arc::new(AtomicUsize::new(0));
    handle.install_bitrate_timing_oracle(Arc::new(TimingProbe(Arc::clone(&timing_calls))));
    let mut source_calls = 0;
    let mut source = |output: &mut [u8]| {
        source_calls += 1;
        output[0] = 0x57;
        Err::<(), _>(())
    };
    assert_eq!(
        handle.request_path_with_source(PEER, &mut source).await,
        Err(RequestPathError::EntropyUnavailable)
    );
    assert_eq!(
        (
            source_calls,
            timing_calls.load(Ordering::Relaxed),
            handle.ids.load(Ordering::Relaxed)
        ),
        (1, 0, 0)
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn supplied_identifiers_reach_timed_commands_and_exact_settlement() {
    let (commands, mut receiver) = mpsc::unbounded_channel();
    let handle = PrnsNodeHandle::over(commands);
    let timing_calls = Arc::new(AtomicUsize::new(0));
    handle.install_bitrate_timing_oracle(Arc::new(TimingProbe(Arc::clone(&timing_calls))));
    let mut source_calls = 0u8;
    let mut source = |output: &mut [u8]| {
        source_calls += 1;
        assert_eq!(output.len(), PATH_REQUEST_ID_LEN);
        output.fill(source_calls);
        Ok::<(), core::convert::Infallible>(())
    };
    for byte in 1..=2 {
        let request = handle.request_path_with_source(PEER, &mut source);
        let settle = async {
            let HostCommand::AwaitedEngineWithTiming {
                issued,
                timing,
                completion,
            } = receiver.recv().await.expect("path request")
            else {
                panic!("expected timed path request");
            };
            let PrnsCommand::RequestPath(request) = issued.command else {
                panic!("expected path command");
            };
            assert_eq!(
                request,
                RequestPath {
                    destination: PEER,
                    id: PathRequestId::new([byte; PATH_REQUEST_ID_LEN])
                }
            );
            assert_eq!(
                timing,
                CommandTiming {
                    first_hop_timeout_floor_ms: None,
                    path_timeout_floor_ms: Some(1234)
                }
            );
            completion
                .send(Settlement::RequestPath(Ok(PathFound {
                    hops: HopCount(byte),
                })))
                .unwrap();
        };
        let (result, ()) = tokio::join!(request, settle);
        assert_eq!(
            result,
            Ok(PathFound {
                hops: HopCount(byte)
            })
        );
    }
    assert_eq!((source_calls, timing_calls.load(Ordering::Relaxed)), (2, 2));
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn successful_source_does_not_hide_a_stopped_node() {
    let (commands, receiver) = mpsc::unbounded_channel();
    let handle = PrnsNodeHandle::over(commands);
    drop(receiver);
    let mut source = |output: &mut [u8]| {
        output.fill(0x57);
        Ok::<(), core::convert::Infallible>(())
    };
    assert_eq!(
        handle.request_path_with_source(PEER, &mut source).await,
        Err(RequestPathError::NodeStopped)
    );
}
