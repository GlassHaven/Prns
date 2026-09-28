use std::sync::{Arc, Mutex};

use prns_core::entropy::{EntropySource, RuntimeEntropy};

use super::OsEntropySource;

pub(crate) type TokioEntropy = SharedEntropy<OsEntropySource>;

/// Clones share ownership, never a copied generator state. Unrelated nodes own distinct streams.
pub(crate) struct SharedEntropy<S> {
    stream: Arc<Mutex<RuntimeEntropy<S>>>,
}

impl<S> Clone for SharedEntropy<S> {
    fn clone(&self) -> Self {
        Self {
            stream: Arc::clone(&self.stream),
        }
    }
}

impl TokioEntropy {
    #[expect(
        clippy::expect_used,
        reason = "runtime entropy requires a functioning OS seed source"
    )]
    pub(crate) fn new() -> Self {
        Self::try_new(OsEntropySource).expect("OS CSPRNG must provide the initial runtime seed")
    }

    #[cfg(test)]
    pub(crate) fn from_test_seed(byte: u8) -> Self {
        let seeded = RuntimeEntropy::try_new(|output: &mut [u8]| {
            output.fill(byte);
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap_or_else(|error| match error {});
        Self {
            stream: Arc::new(Mutex::new(seeded.with_source(OsEntropySource))),
        }
    }
}

impl<S: EntropySource> SharedEntropy<S> {
    fn try_new(source: S) -> Result<Self, S::Error> {
        RuntimeEntropy::try_new(source).map(|stream| Self {
            stream: Arc::new(Mutex::new(stream)),
        })
    }

    #[expect(
        clippy::expect_used,
        reason = "a poisoned entropy stream must not continue emitting bytes"
    )]
    pub(crate) fn fill(&self, output: &mut [u8]) {
        if output.is_empty() {
            return;
        }
        self.stream
            .lock()
            .expect("runtime entropy owner must not be poisoned")
            .fill_random(output);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::convert::Infallible;

    fn source(output: &mut [u8]) -> Result<(), Infallible> {
        output.fill(0x57);
        Ok(())
    }

    #[test]
    fn cloned_owners_continue_one_stream_without_copying_state() {
        let owner = SharedEntropy::try_new(source).unwrap();
        let clone = owner.clone();
        let mut expected = RuntimeEntropy::try_new(source).unwrap();
        let mut actual_bytes = [0; 128];
        let mut expected_bytes = [0; 128];
        owner.fill(&mut actual_bytes[..64]);
        clone.fill(&mut actual_bytes[64..]);
        expected.fill_random(&mut expected_bytes[..64]);
        expected.fill_random(&mut expected_bytes[64..]);
        assert_eq!(actual_bytes, expected_bytes);
        assert!(Arc::ptr_eq(&owner.stream, &clone.stream));
    }

    #[test]
    fn unrelated_owners_do_not_consume_each_others_stream() {
        let first = SharedEntropy::try_new(source).unwrap();
        let second = SharedEntropy::try_new(source).unwrap();
        assert!(!Arc::ptr_eq(&first.stream, &second.stream));
        first.fill(&mut [0; 64]);
        let mut observed = [0; 64];
        second.fill(&mut observed);
        let mut expected = [0; 64];
        RuntimeEntropy::try_new(source)
            .unwrap()
            .fill_random(&mut expected);
        assert_eq!(observed, expected);
    }

    #[test]
    fn a_clone_can_move_threads_without_reseeding_or_rewinding() {
        let owner = SharedEntropy::try_new(source).unwrap();
        let clone = owner.clone();
        let first = std::thread::spawn(move || {
            let mut bytes = [0; 64];
            clone.fill(&mut bytes);
            bytes
        })
        .join()
        .unwrap();
        let mut second = [0; 64];
        owner.fill(&mut second);
        let mut expected = [0; 128];
        RuntimeEntropy::try_new(source)
            .unwrap()
            .fill_random(&mut expected);
        assert_eq!([first, second].concat(), expected);
    }

    #[test]
    fn initial_seed_failure_never_constructs_an_owner() {
        let source = |_: &mut [u8]| Err::<(), _>(7);
        assert!(matches!(SharedEntropy::try_new(source), Err(7)));
    }

    #[test]
    fn concurrent_clones_consume_each_block_exactly_once() {
        let owner = SharedEntropy::try_new(source).unwrap();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let clone = owner.clone();
                std::thread::spawn(move || {
                    let mut bytes = [0; 64];
                    clone.fill(&mut bytes);
                    bytes
                })
            })
            .collect();
        let mut actual: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        let mut reference = RuntimeEntropy::try_new(source).unwrap();
        let mut expected = vec![[0; 64]; 8];
        for block in &mut expected {
            reference.fill_random(block);
        }
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }

    #[test]
    fn empty_fills_and_reseeding_preserve_core_stream_semantics() {
        let owner = SharedEntropy::try_new(source).unwrap();
        let clone = owner.clone();
        let mut reference = RuntimeEntropy::try_new(source).unwrap();
        let mut actual = vec![0; 64 * 1024 + 128];
        let mut expected = vec![0; actual.len()];
        owner.fill(&mut []);
        owner.fill(&mut actual[..64 * 1024]);
        clone.fill(&mut actual[64 * 1024..]);
        reference.fill_random(&mut expected[..64 * 1024]);
        reference.fill_random(&mut expected[64 * 1024..]);
        assert_eq!(actual, expected);
        assert_eq!(
            owner.stream.lock().unwrap().reseed_health(),
            prns_core::entropy::ReseedHealth::Healthy
        );
    }

    #[test]
    fn a_failed_reseed_retains_the_existing_secure_stream() {
        fn scripted() -> impl FnMut(&mut [u8]) -> Result<(), ()> {
            let mut calls = 0;
            move |output| {
                calls += 1;
                if calls == 2 {
                    return Err(());
                }
                output.fill(0x57);
                Ok(())
            }
        }
        let owner = SharedEntropy::try_new(scripted()).unwrap();
        let mut reference = RuntimeEntropy::try_new(scripted()).unwrap();
        let mut actual = vec![0; 64 * 1024 + 64];
        let mut expected = vec![0; actual.len()];
        owner.fill(&mut actual);
        reference.fill_random(&mut expected);
        assert_eq!(actual, expected);
        assert_eq!(
            owner.stream.lock().unwrap().reseed_health(),
            prns_core::entropy::ReseedHealth::Deferred
        );
    }

    #[test]
    #[allow(clippy::panic)]
    fn poisoned_ownership_refuses_output_instead_of_recovering_silently() {
        let owner = SharedEntropy::try_new(source).unwrap();
        let clone = owner.clone();
        assert!(std::thread::spawn(move || {
            let _held = clone.stream.lock().unwrap();
            panic!("injected failure while owning the entropy stream");
        })
        .join()
        .is_err());
        let mut bytes = [0xA5; 64];
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.fill(&mut bytes)))
                .is_err()
        );
        assert_eq!(bytes, [0xA5; 64]);
    }

    #[test]
    #[ignore = "local timing probe; not a fleet or publication benchmark"]
    fn compare_thread_local_and_owned_entropy_cost() {
        use crate::runtime::OsRuntimeEntropy;
        use std::{cell::RefCell, hint::black_box, time::Instant};
        thread_local! {
            static PREVIOUS: RefCell<Option<OsRuntimeEntropy>> = const { RefCell::new(None) };
        }
        fn previous(output: &mut [u8]) {
            if output.is_empty() {
                return;
            }
            PREVIOUS.with(|stream| {
                stream
                    .borrow_mut()
                    .get_or_insert_with(|| OsRuntimeEntropy::try_new().unwrap())
                    .fill_random(output)
            });
        }
        const FILLS: usize = 100_000;
        let owner = TokioEntropy::new();
        for length in [16, 64, 256] {
            let mut bytes = vec![0; length];
            previous(&mut bytes);
            owner.fill(&mut bytes);
            for round in 0..3 {
                let start = Instant::now();
                for _ in 0..FILLS {
                    previous(black_box(&mut bytes));
                    black_box(&bytes);
                }
                let old = start.elapsed();
                let start = Instant::now();
                for _ in 0..FILLS {
                    owner.fill(black_box(&mut bytes));
                    black_box(&bytes);
                }
                eprintln!("entropy length={length} round={round} fills={FILLS} previous={old:?} owned={:?}", start.elapsed());
            }
        }
        eprintln!(
            "entropy owner_handle={} mutex_and_stream={} arc_counters={} bytes",
            std::mem::size_of::<TokioEntropy>(),
            std::mem::size_of::<Mutex<RuntimeEntropy<OsEntropySource>>>(),
            2 * std::mem::size_of::<usize>()
        );
    }
}
