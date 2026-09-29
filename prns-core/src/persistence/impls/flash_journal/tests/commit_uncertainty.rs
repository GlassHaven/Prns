use super::*;

#[derive(Clone, Copy)]
enum CommitWrite {
    Complete,
    Torn,
}

struct UncertainCommitFlash {
    inner: FakeFlash,
    commit: CommitWrite,
    commit_attempted: bool,
    fail_readback: bool,
}

impl ErrorType for UncertainCommitFlash {
    type Error = FakeError;
}

impl ReadNorFlash for UncertainCommitFlash {
    const READ_SIZE: usize = FakeFlash::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        if self.commit_attempted && self.fail_readback {
            return Err(FakeError::Interrupted);
        }
        self.inner.read(offset, bytes).await
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }
}

impl NorFlash for UncertainCommitFlash {
    const WRITE_SIZE: usize = FakeFlash::WRITE_SIZE;
    const ERASE_SIZE: usize = FakeFlash::ERASE_SIZE;

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        if bytes != COMMIT_WORD.to_le_bytes() {
            return self.inner.write(offset, bytes).await;
        }
        self.commit_attempted = true;
        match self.commit {
            CommitWrite::Complete => self.inner.write(offset, bytes).await?,
            CommitWrite::Torn => self.inner.bytes[offset as usize] &= bytes[0],
        }
        Err(FakeError::Interrupted)
    }

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.inner.erase(from, to).await
    }
}

#[test]
fn uncertain_commit_is_successful_only_when_readback_proves_the_commit() {
    embassy_futures::block_on(async {
        let (mut baseline, _, _) = open(FakeFlash::new()).await;
        baseline.initialize_empty().await.unwrap();
        baseline
            .append(FlashJournalRecordKind::RouteUpsert, b"prior")
            .await
            .unwrap();
        let bytes = baseline.release().bytes;
        for commit in [CommitWrite::Complete, CommitWrite::Torn] {
            for fail_readback in [false, true] {
                let mut flash = FakeFlash::new();
                flash.bytes = bytes;
                let flash = UncertainCommitFlash {
                    inner: flash,
                    commit,
                    commit_attempted: false,
                    fail_readback,
                };
                let (mut journal, _) =
                    FlashJournal::open(flash, LAYOUT, &mut [0; IO_CHUNK_LEN], |_| {})
                        .await
                        .unwrap();
                let result = journal
                    .append(
                        FlashJournalRecordKind::RemoteControlControllerGrants,
                        b"candidate",
                    )
                    .await;
                let expected = if matches!(commit, CommitWrite::Complete) && !fail_readback {
                    Ok(())
                } else {
                    Err(FlashJournalError::Flash(FakeError::Interrupted))
                };
                assert_eq!(result, expected);
                if result.is_ok() {
                    journal
                        .append(FlashJournalRecordKind::RouteRemoval, b"next")
                        .await
                        .unwrap();
                }
                let mut expected_records =
                    vec![(FlashJournalRecordKind::RouteUpsert, b"prior".to_vec())];
                if matches!(commit, CommitWrite::Complete) {
                    expected_records.push((
                        FlashJournalRecordKind::RemoteControlControllerGrants,
                        b"candidate".to_vec(),
                    ));
                    if !fail_readback {
                        expected_records
                            .push((FlashJournalRecordKind::RouteRemoval, b"next".to_vec()));
                    }
                }
                let (_, report, records) = open(journal.release().inner).await;
                assert_eq!(report.warning, None);
                assert_eq!(records, expected_records);
            }
        }
    });
}
