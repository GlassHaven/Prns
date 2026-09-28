use super::*;

#[derive(Clone, Copy)]
pub(super) enum Retirement {
    KeepAll,
    CancelMiddle,
    CancelMiddleAtDeadline,
}

impl Retirement {
    pub(super) fn cancels(self, admission: usize) -> bool {
        match self {
            Self::KeepAll => false,
            Self::CancelMiddle | Self::CancelMiddleAtDeadline => admission == 1,
        }
    }

    pub(super) fn apply(
        self,
        runner: &mut ManualTaskRunner<'_, Completion>,
        frames: &VirtualMedium,
        ble: &VirtualBleLab,
        pending: &[PendingRequest],
    ) -> usize {
        match self {
            Self::KeepAll => return 0,
            Self::CancelMiddle => {}
            Self::CancelMiddleAtDeadline => {
                let deadline = pending[1].deadline;
                assert_eq!(
                    pending
                        .iter()
                        .map(|request| request.deadline)
                        .collect::<Vec<_>>(),
                    vec![deadline; WAVES]
                );
                assert!(frames.now() < deadline);
                for _ in 0..POLL_BUDGET {
                    let next = tick((frames.now().get() + 1).min(deadline.get()));
                    assert!(runner.advance_to_next_event(next).is_ok());
                    assert_eq!(frames.now(), ble.now());
                    if frames.now() == deadline {
                        break;
                    }
                    assert!(settle(runner).is_empty());
                }
                assert_eq!(frames.now(), deadline);
                assert_eq!(runner.task_count(), 3 + WAVES);
            }
        }
        let clock = runner.snapshot().ok();
        let activity = ble.data_snapshots();
        assert_eq!(
            runner.cancel(pending[1].task).ok(),
            Some(ManualTaskCancellation::Cancelled)
        );
        assert_eq!(
            runner.cancel(pending[1].task).ok(),
            Some(ManualTaskCancellation::NotLive)
        );
        assert_eq!(runner.task_count(), 3 + WAVES - 1);
        assert_eq!(runner.snapshot().ok(), clock);
        assert_eq!(ble.data_snapshots(), activity);
        1
    }
}

fn matrix(boundary: CutBoundary, retirement: Retirement) {
    for exchange in [Exchange::Request, Exchange::Response] {
        matrix_scenario(
            exchange,
            boundary,
            Scenario {
                retirement,
                order: ExchangeOrder::Alternating,
                deadlines: deadlines::DeadlineOrder::Shared,
            },
        );
    }
}

#[test]
fn early_cancellation_preserves_shared_queued_deadline() {
    matrix(CutBoundary::Queued, Retirement::CancelMiddle);
}

#[test]
fn early_cancellation_preserves_shared_consumed_deadline() {
    matrix(CutBoundary::Consumed, Retirement::CancelMiddle);
}

#[test]
fn deadline_tick_cancellation_preserves_shared_queued_survivors() {
    matrix(CutBoundary::Queued, Retirement::CancelMiddleAtDeadline);
}

#[test]
fn deadline_tick_cancellation_preserves_shared_consumed_survivors() {
    matrix(CutBoundary::Consumed, Retirement::CancelMiddleAtDeadline);
}
