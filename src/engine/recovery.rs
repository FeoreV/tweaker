//! Conflict-aware recovery for records captured by a transaction journal.
//!
//! `recover_journal_prefix` composes journal validation, reverse restoration,
//! pre-restore snapshot persistence, and checkpoint progress. Callers must
//! supply an appropriate durable snapshot store and lock each target for the
//! whole query/restore/verification sequence.

use super::transaction::SnapshotStore;
use crate::data::journal::{
    encode as encode_journal_record, encode_transaction_record, RecordError, RegistryRecord,
    TransactionRecord,
};
use crate::data::store::{TypedState, TypedTarget};
use crate::engine::registry::{ValueState, ValueTarget};

#[derive(Debug)]
pub enum RecoveryPlanError {
    InvalidSequence,
    InvalidRecord(RecordError),
}

/// Convert a validated journal prefix to the engine's recovery representation.
///
/// This verifies that all records belong to one transaction, have contiguous
/// sequence numbers, and satisfy the current journal format. Callers must only
/// pass records returned from a committed scan; this function does not provide
/// storage or commit-marker verification.
pub fn recovery_records_from_journal(
    records: &[RegistryRecord],
) -> Result<Vec<RecoveryRecord<ValueTarget, ValueState>>, RecoveryPlanError> {
    let Some(first) = records.first() else {
        return Ok(Vec::new());
    };
    let transaction_id = &first.transaction_id;
    let mut recovery_records = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        if record.transaction_id != *transaction_id
            || u64::try_from(index).ok() != Some(record.sequence)
        {
            return Err(RecoveryPlanError::InvalidSequence);
        }
        encode_journal_record(record).map_err(RecoveryPlanError::InvalidRecord)?;
        recovery_records.push(RecoveryRecord {
            target: record.target.clone(),
            before: record.before.clone(),
            intended: record.intended.clone(),
        });
    }
    Ok(recovery_records)
}

/// Convert a committed typed prefix into a resource-tagged recovery plan.
pub fn typed_recovery_records_from_journal(
    records: &[TransactionRecord],
) -> Result<Vec<RecoveryRecord<TypedTarget, TypedState>>, RecoveryPlanError> {
    let Some(first) = records.first() else {
        return Ok(Vec::new());
    };
    let transaction_id = first.transaction_id();
    let mut plan = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        if record.transaction_id() != transaction_id
            || u64::try_from(index).ok() != Some(record.sequence())
        {
            return Err(RecoveryPlanError::InvalidSequence);
        }
        encode_transaction_record(record).map_err(RecoveryPlanError::InvalidRecord)?;
        plan.push(match record {
            TransactionRecord::Registry(record) => RecoveryRecord {
                target: TypedTarget::Registry(record.target.clone()),
                before: TypedState::Registry(record.before.clone()),
                intended: TypedState::Registry(record.intended.clone()),
            },
            TransactionRecord::Service(record) => RecoveryRecord {
                target: TypedTarget::Service(record.target.clone()),
                before: TypedState::Service(record.before_start_type),
                intended: TypedState::Service(record.intended_start_type),
            },
        });
    }
    Ok(plan)
}

/// Persisted state for a single operation, including both possible states after
/// interruption. The state type must represent absence and exact native data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryRecord<T, S> {
    pub target: T,
    pub before: S,
    pub intended: S,
}

/// Access to a resource while the caller holds exclusive target ownership.
pub trait RecoveryBackend {
    type Target;
    type State: PartialEq;
    type Error;

    fn query(&mut self, target: &Self::Target) -> Result<Self::State, Self::Error>;
    fn restore(&mut self, target: &Self::Target, state: &Self::State) -> Result<(), Self::Error>;
}

/// Resource-tagged backend. A production implementation must keep each
/// resource's exclusive lock over query, restore and verification.
pub trait TypedRecoveryBackend {
    type Error;

    fn query_typed(&mut self, target: &TypedTarget) -> Result<TypedState, Self::Error>;
    fn restore_typed(
        &mut self,
        target: &TypedTarget,
        state: &TypedState,
    ) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryError<E, S> {
    Query(E),
    /// The target differs from both the saved and intended states.
    Conflict,
    InvalidResourceKind,
    /// Saving the state before recovery failed; no restore was attempted.
    Persist(S),
    Restore(E),
    Verify(E),
    VerificationMismatch,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryOutcome {
    AlreadyRestored,
    Restored,
}

/// Restores one record only if its current state is known to be safe to undo.
///
/// A partial mutation that differs from both states is reported as a conflict,
/// so no unrelated changes are overwritten. This cannot detect an ABA change.
pub fn recover_one<B, S>(
    backend: &mut B,
    store: &mut S,
    record: &RecoveryRecord<B::Target, B::State>,
) -> Result<RecoveryOutcome, RecoveryError<B::Error, S::Error>>
where
    B: RecoveryBackend,
    S: SnapshotStore<B::Target, B::State, B::State>,
{
    let current = backend
        .query(&record.target)
        .map_err(RecoveryError::Query)?;
    if current == record.before {
        return Ok(RecoveryOutcome::AlreadyRestored);
    }
    if current != record.intended {
        return Err(RecoveryError::Conflict);
    }
    store
        .persist(&record.target, &current, &record.before)
        .map_err(RecoveryError::Persist)?;
    backend
        .restore(&record.target, &record.before)
        .map_err(RecoveryError::Restore)?;
    let restored = backend
        .query(&record.target)
        .map_err(RecoveryError::Verify)?;
    if restored != record.before {
        return Err(RecoveryError::VerificationMismatch);
    }
    Ok(RecoveryOutcome::Restored)
}

/// Restore one typed resource only when the saved/intended state pair matches
/// its target kind. A failed snapshot prevents the restore call.
pub fn recover_typed_one<B, S>(
    backend: &mut B,
    store: &mut S,
    record: &RecoveryRecord<TypedTarget, TypedState>,
) -> Result<RecoveryOutcome, RecoveryError<B::Error, S::Error>>
where
    B: TypedRecoveryBackend,
    S: SnapshotStore<TypedTarget, TypedState, TypedState>,
{
    if !matches!(
        (&record.target, &record.before, &record.intended),
        (
            TypedTarget::Registry(_),
            TypedState::Registry(_),
            TypedState::Registry(_)
        ) | (
            TypedTarget::Service(_),
            TypedState::Service(_),
            TypedState::Service(_)
        )
    ) {
        return Err(RecoveryError::InvalidResourceKind);
    }
    let current = backend
        .query_typed(&record.target)
        .map_err(RecoveryError::Query)?;
    if current == record.before {
        return Ok(RecoveryOutcome::AlreadyRestored);
    }
    if current != record.intended {
        return Err(RecoveryError::Conflict);
    }
    store
        .persist(&record.target, &current, &record.before)
        .map_err(RecoveryError::Persist)?;
    backend
        .restore_typed(&record.target, &record.before)
        .map_err(RecoveryError::Restore)?;
    let restored = backend
        .query_typed(&record.target)
        .map_err(RecoveryError::Verify)?;
    if restored != record.before {
        return Err(RecoveryError::VerificationMismatch);
    }
    Ok(RecoveryOutcome::Restored)
}

/// Persist the next exclusive index after each successful recovery step.
/// The checkpoint must be durable and tied to the exact ordered transaction.
pub trait RecoveryCheckpoint {
    type Error;
    /// Return progress loaded from the checkpoint bound to the exact record list.
    fn next_exclusive(&self) -> usize;
    fn persist_next(&mut self, next_exclusive: usize) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum BatchRecoveryError<B, S, C> {
    InvalidCheckpoint,
    Operation {
        index: usize,
        error: RecoveryError<B, S>,
    },
    /// The operation at this index was recovered but its checkpoint failed.
    Checkpoint {
        index: usize,
        error: C,
    },
}

pub type BatchResult<B, S, C> = Result<(), BatchRecoveryError<B, S, C>>;

#[derive(Debug)]
pub enum JournalRecoveryError<B, S, C> {
    InvalidJournal(RecoveryPlanError),
    Recovery(BatchRecoveryError<B, S, C>),
}

pub type JournalRecoveryResult<B, S, C> = Result<(), JournalRecoveryError<B, S, C>>;

/// Resume reverse recovery for any validated typed journal prefix.
pub fn recover_typed_journal_prefix<B, S, C>(
    backend: &mut B,
    store: &mut S,
    checkpoint: &mut C,
    journal_records: &[TransactionRecord],
) -> JournalRecoveryResult<B::Error, S::Error, C::Error>
where
    B: TypedRecoveryBackend,
    S: SnapshotStore<TypedTarget, TypedState, TypedState>,
    C: RecoveryCheckpoint,
{
    let recovery_records = typed_recovery_records_from_journal(journal_records)
        .map_err(JournalRecoveryError::InvalidJournal)?;
    let next_exclusive = checkpoint.next_exclusive();
    recover_typed_reverse(
        backend,
        store,
        checkpoint,
        &recovery_records,
        next_exclusive,
    )
    .map_err(JournalRecoveryError::Recovery)
}

pub fn recover_typed_reverse<B, S, C>(
    backend: &mut B,
    store: &mut S,
    checkpoint: &mut C,
    records: &[RecoveryRecord<TypedTarget, TypedState>],
    next_exclusive: usize,
) -> BatchResult<B::Error, S::Error, C::Error>
where
    B: TypedRecoveryBackend,
    S: SnapshotStore<TypedTarget, TypedState, TypedState>,
    C: RecoveryCheckpoint,
{
    if next_exclusive > records.len() {
        return Err(BatchRecoveryError::InvalidCheckpoint);
    }
    for index in (0..next_exclusive).rev() {
        recover_typed_one(backend, store, &records[index])
            .map_err(|error| BatchRecoveryError::Operation { index, error })?;
        checkpoint
            .persist_next(index)
            .map_err(|error| BatchRecoveryError::Checkpoint { index, error })?;
    }
    Ok(())
}

/// Resume reverse recovery directly from a committed registry journal prefix.
///
/// `checkpoint` must already be opened and validated against `journal_records`;
/// `store` must persist each pre-restore state before `backend.restore` runs.
/// The runner preserves the journal order and resumes from the checkpoint's
/// current exclusive index.
pub fn recover_journal_prefix<B, S, C>(
    backend: &mut B,
    store: &mut S,
    checkpoint: &mut C,
    journal_records: &[RegistryRecord],
) -> JournalRecoveryResult<B::Error, S::Error, C::Error>
where
    B: RecoveryBackend<Target = ValueTarget, State = ValueState>,
    S: SnapshotStore<ValueTarget, ValueState, ValueState>,
    C: RecoveryCheckpoint,
{
    let recovery_records = recovery_records_from_journal(journal_records)
        .map_err(JournalRecoveryError::InvalidJournal)?;
    let next_exclusive = checkpoint.next_exclusive();
    recover_reverse(
        backend,
        store,
        checkpoint,
        &recovery_records,
        next_exclusive,
    )
    .map_err(JournalRecoveryError::Recovery)
}

/// Recover the prefix `[0, next_exclusive)` in reverse order.
///
/// The caller must hold appropriate locks and load a validated checkpoint bound
/// to these records. It must persist the initial checkpoint before starting.
/// Every successful step is checkpointed before touching an earlier record.
/// Repeating the current step after a crash is safe only under the documented
/// conflict and ABA limitations of `recover_one`.
pub fn recover_reverse<B, S, C>(
    backend: &mut B,
    store: &mut S,
    checkpoint: &mut C,
    records: &[RecoveryRecord<B::Target, B::State>],
    next_exclusive: usize,
) -> BatchResult<B::Error, S::Error, C::Error>
where
    B: RecoveryBackend,
    S: SnapshotStore<B::Target, B::State, B::State>,
    C: RecoveryCheckpoint,
{
    if next_exclusive > records.len() {
        return Err(BatchRecoveryError::InvalidCheckpoint);
    }
    for index in (0..next_exclusive).rev() {
        recover_one(backend, store, &records[index])
            .map_err(|error| BatchRecoveryError::Operation { index, error })?;
        checkpoint
            .persist_next(index)
            .map_err(|error| BatchRecoveryError::Checkpoint { index, error })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::journal::{
        RegistryRecord, ServiceRecord, ServiceTarget, TransactionRecord, FORMAT_VERSION,
        SERVICE_FORMAT_VERSION,
    };
    use crate::engine::registry::{Hive, RegistryValue, RegistryView};
    use std::collections::HashMap;

    fn journal_record(sequence: u64) -> RegistryRecord {
        RegistryRecord {
            version: FORMAT_VERSION,
            transaction_id: "tx_recovery".into(),
            sequence,
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Setting".into(),
            },
            before: ValueState::ValueMissing,
            intended: ValueState::Present(RegistryValue {
                native_type: 4,
                bytes: vec![0, 1, 255],
            }),
        }
    }

    #[test]
    fn journal_prefix_converts_without_losing_target_or_exact_state() {
        let records = [journal_record(0), journal_record(1)];
        let recovery = recovery_records_from_journal(&records).unwrap();
        assert_eq!(recovery.len(), 2);
        assert_eq!(recovery[0].target, records[0].target);
        assert_eq!(recovery[0].before, records[0].before);
        assert_eq!(recovery[0].intended, records[0].intended);
        assert_eq!(recovery[1].target, records[1].target);
    }

    #[test]
    fn journal_conversion_rejects_gaps_mixed_transactions_and_invalid_records() {
        let mut records = [journal_record(0), journal_record(1)];
        records[1].sequence = 2;
        assert!(matches!(
            recovery_records_from_journal(&records),
            Err(RecoveryPlanError::InvalidSequence)
        ));
        records[1] = journal_record(1);
        records[1].transaction_id = "other".into();
        assert!(matches!(
            recovery_records_from_journal(&records),
            Err(RecoveryPlanError::InvalidSequence)
        ));
        records[1] = journal_record(1);
        records[1].version += 1;
        assert!(matches!(
            recovery_records_from_journal(&records),
            Err(RecoveryPlanError::InvalidRecord(_))
        ));
    }

    #[derive(Default)]
    struct TypedMock {
        current: Option<TypedState>,
        writes: usize,
    }

    impl TypedRecoveryBackend for TypedMock {
        type Error = &'static str;

        fn query_typed(&mut self, _: &TypedTarget) -> Result<TypedState, Self::Error> {
            self.current.clone().ok_or("missing")
        }

        fn restore_typed(
            &mut self,
            _: &TypedTarget,
            state: &TypedState,
        ) -> Result<(), Self::Error> {
            self.writes += 1;
            self.current = Some(state.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct TypedBackup {
        records: Vec<(TypedTarget, TypedState, TypedState)>,
        fail: bool,
    }

    impl SnapshotStore<TypedTarget, TypedState, TypedState> for TypedBackup {
        type Error = &'static str;

        fn persist(
            &mut self,
            target: &TypedTarget,
            before: &TypedState,
            intended: &TypedState,
        ) -> Result<(), Self::Error> {
            if self.fail {
                return Err("persist");
            }
            self.records
                .push((target.clone(), before.clone(), intended.clone()));
            Ok(())
        }
    }

    #[derive(Default)]
    struct TypedProgress(usize);

    impl RecoveryCheckpoint for TypedProgress {
        type Error = &'static str;

        fn next_exclusive(&self) -> usize {
            self.0
        }

        fn persist_next(&mut self, next_exclusive: usize) -> Result<(), Self::Error> {
            self.0 = next_exclusive;
            Ok(())
        }
    }

    fn service_journal_record(sequence: u64) -> TransactionRecord {
        TransactionRecord::Service(ServiceRecord {
            version: SERVICE_FORMAT_VERSION,
            transaction_id: "tx_typed_recovery".into(),
            sequence,
            target: ServiceTarget {
                name: "ExampleService".into(),
            },
            before_start_type: 3,
            intended_start_type: 4,
        })
    }

    #[test]
    fn typed_recovery_persists_snapshot_before_service_restore_and_checkpoints() {
        let records = [service_journal_record(0)];
        let target = TypedTarget::Service(ServiceTarget {
            name: "ExampleService".into(),
        });
        let mut backend = TypedMock {
            current: Some(TypedState::Service(4)),
            ..Default::default()
        };
        let mut backup = TypedBackup::default();
        let mut progress = TypedProgress(1);

        recover_typed_journal_prefix(&mut backend, &mut backup, &mut progress, &records).unwrap();

        assert_eq!(backend.current, Some(TypedState::Service(3)));
        assert_eq!(backend.writes, 1);
        assert_eq!(
            backup.records,
            vec![(target, TypedState::Service(4), TypedState::Service(3))]
        );
        assert_eq!(progress.next_exclusive(), 0);
    }

    #[test]
    fn typed_recovery_backup_failure_prevents_restore() {
        let records = [service_journal_record(0)];
        let mut backend = TypedMock {
            current: Some(TypedState::Service(4)),
            ..Default::default()
        };
        let mut backup = TypedBackup {
            fail: true,
            ..Default::default()
        };
        let mut progress = TypedProgress(1);
        assert!(matches!(
            recover_typed_journal_prefix(&mut backend, &mut backup, &mut progress, &records),
            Err(JournalRecoveryError::Recovery(
                BatchRecoveryError::Operation {
                    error: RecoveryError::Persist("persist"),
                    ..
                }
            ))
        ));
        assert_eq!(backend.writes, 0);
        assert_eq!(backend.current, Some(TypedState::Service(4)));
        assert_eq!(progress.next_exclusive(), 1);
    }

    struct JournalBackend {
        value: ValueState,
        writes: usize,
    }

    impl Default for JournalBackend {
        fn default() -> Self {
            Self {
                value: ValueState::ValueMissing,
                writes: 0,
            }
        }
    }

    impl RecoveryBackend for JournalBackend {
        type Target = ValueTarget;
        type State = ValueState;
        type Error = &'static str;

        fn query(&mut self, _: &ValueTarget) -> Result<ValueState, Self::Error> {
            Ok(self.value.clone())
        }

        fn restore(&mut self, _: &ValueTarget, state: &ValueState) -> Result<(), Self::Error> {
            self.writes += 1;
            self.value = state.clone();
            Ok(())
        }
    }

    #[derive(Default)]
    struct JournalBackupStore {
        snapshots: Vec<(ValueTarget, ValueState, ValueState)>,
        fail: bool,
    }

    impl SnapshotStore<ValueTarget, ValueState, ValueState> for JournalBackupStore {
        type Error = &'static str;

        fn persist(
            &mut self,
            target: &ValueTarget,
            before: &ValueState,
            intended: &ValueState,
        ) -> Result<(), Self::Error> {
            if self.fail {
                return Err("persist");
            }
            self.snapshots
                .push((target.clone(), before.clone(), intended.clone()));
            Ok(())
        }
    }

    fn chained_journal_records() -> [RegistryRecord; 2] {
        let mut first = journal_record(0);
        first.before = ValueState::ValueMissing;
        first.intended = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1],
        });
        let mut second = journal_record(1);
        second.before = first.intended.clone();
        second.intended = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![2],
        });
        [first, second]
    }

    #[test]
    fn journal_runner_restores_in_reverse_and_persists_each_snapshot_and_progress() {
        let records = chained_journal_records();
        let mut backend = JournalBackend {
            value: records[1].intended.clone(),
            ..Default::default()
        };
        let mut store = JournalBackupStore::default();
        let mut checkpoint = Checkpoint {
            progress: 2,
            ..Default::default()
        };

        recover_journal_prefix(&mut backend, &mut store, &mut checkpoint, &records).unwrap();

        assert_eq!(backend.value, ValueState::ValueMissing);
        assert_eq!(backend.writes, 2);
        assert_eq!(store.snapshots.len(), 2);
        assert_eq!(store.snapshots[0].1, records[1].intended);
        assert_eq!(store.snapshots[0].2, records[1].before);
        assert_eq!(store.snapshots[1].1, records[0].intended);
        assert_eq!(store.snapshots[1].2, records[0].before);
        assert_eq!(checkpoint.next, vec![1, 0]);
        assert_eq!(checkpoint.next_exclusive(), 0);
    }

    #[test]
    fn journal_runner_rejects_invalid_log_before_snapshot_or_restore() {
        let mut records = chained_journal_records();
        records[1].version += 1;
        let mut backend = JournalBackend {
            value: records[1].intended.clone(),
            ..Default::default()
        };
        let mut store = JournalBackupStore::default();
        let mut checkpoint = Checkpoint {
            progress: 2,
            ..Default::default()
        };

        assert!(matches!(
            recover_journal_prefix(&mut backend, &mut store, &mut checkpoint, &records),
            Err(JournalRecoveryError::InvalidJournal(
                RecoveryPlanError::InvalidRecord(_)
            ))
        ));
        assert_eq!(backend.writes, 0);
        assert!(store.snapshots.is_empty());
        assert!(checkpoint.next.is_empty());
    }

    #[test]
    fn journal_runner_stops_before_restore_when_backup_snapshot_fails() {
        let records = chained_journal_records();
        let mut backend = JournalBackend {
            value: records[1].intended.clone(),
            ..Default::default()
        };
        let mut store = JournalBackupStore {
            fail: true,
            ..Default::default()
        };
        let mut checkpoint = Checkpoint {
            progress: 2,
            ..Default::default()
        };

        assert!(matches!(
            recover_journal_prefix(&mut backend, &mut store, &mut checkpoint, &records),
            Err(JournalRecoveryError::Recovery(
                BatchRecoveryError::Operation {
                    index: 1,
                    error: RecoveryError::Persist("persist")
                }
            ))
        ));
        assert_eq!(backend.writes, 0);
        assert!(store.snapshots.is_empty());
        assert!(checkpoint.next.is_empty());
    }

    #[derive(Default)]
    struct Mock {
        values: HashMap<&'static str, Vec<u8>>,
        writes: usize,
        fail_restore: bool,
        ignore_restore: bool,
    }
    impl RecoveryBackend for Mock {
        type Target = &'static str;
        type State = Option<Vec<u8>>;
        type Error = &'static str;
        fn query(&mut self, target: &Self::Target) -> Result<Self::State, Self::Error> {
            Ok(self.values.get(target).cloned())
        }
        fn restore(
            &mut self,
            target: &Self::Target,
            state: &Self::State,
        ) -> Result<(), Self::Error> {
            self.writes += 1;
            if self.fail_restore {
                return Err("restore");
            }
            if !self.ignore_restore {
                match state {
                    Some(value) => {
                        self.values.insert(*target, value.clone());
                    }
                    None => {
                        self.values.remove(target);
                    }
                }
            }
            Ok(())
        }
    }
    type State = Option<Vec<u8>>;

    #[derive(Default)]
    struct Store {
        fail: bool,
        records: Vec<(State, State)>,
    }
    impl SnapshotStore<&'static str, Option<Vec<u8>>, Option<Vec<u8>>> for Store {
        type Error = &'static str;
        fn persist(
            &mut self,
            _: &&'static str,
            before: &Option<Vec<u8>>,
            intended: &Option<Vec<u8>>,
        ) -> Result<(), Self::Error> {
            if self.fail {
                return Err("persist");
            }
            self.records.push((before.clone(), intended.clone()));
            Ok(())
        }
    }
    fn record(
        before: Option<Vec<u8>>,
        intended: Option<Vec<u8>>,
    ) -> RecoveryRecord<&'static str, Option<Vec<u8>>> {
        RecoveryRecord {
            target: "key",
            before,
            intended,
        }
    }
    #[test]
    fn interrupted_before_apply_does_not_write() {
        let mut backend = Mock::default();
        assert_eq!(
            recover_one(
                &mut backend,
                &mut Store::default(),
                &record(None, Some(vec![1]))
            ),
            Ok(RecoveryOutcome::AlreadyRestored)
        );
        assert_eq!(backend.writes, 0);
    }
    #[test]
    fn applied_value_is_restored_to_absence_and_retry_is_safe() {
        let mut backend = Mock::default();
        backend.values.insert("key", vec![1]);
        let r = record(None, Some(vec![1]));
        assert_eq!(
            recover_one(&mut backend, &mut Store::default(), &r),
            Ok(RecoveryOutcome::Restored)
        );
        assert_eq!(
            recover_one(&mut backend, &mut Store::default(), &r),
            Ok(RecoveryOutcome::AlreadyRestored)
        );
        assert_eq!(backend.writes, 1);
    }
    #[test]
    fn unrelated_changes_are_not_overwritten() {
        let mut backend = Mock::default();
        backend.values.insert("key", vec![2]);
        assert_eq!(
            recover_one(
                &mut backend,
                &mut Store::default(),
                &record(None, Some(vec![1]))
            ),
            Err(RecoveryError::Conflict)
        );
        assert_eq!(backend.writes, 0);
    }
    #[test]
    fn failed_restore_can_be_retried_and_verified() {
        let mut backend = Mock {
            fail_restore: true,
            ..Default::default()
        };
        backend.values.insert("key", vec![1]);
        let r = record(Some(vec![]), Some(vec![1]));
        assert_eq!(
            recover_one(&mut backend, &mut Store::default(), &r),
            Err(RecoveryError::Restore("restore"))
        );
        backend.fail_restore = false;
        backend.ignore_restore = true;
        assert_eq!(
            recover_one(&mut backend, &mut Store::default(), &r),
            Err(RecoveryError::VerificationMismatch)
        );
        backend.ignore_restore = false;
        assert_eq!(
            recover_one(&mut backend, &mut Store::default(), &r),
            Ok(RecoveryOutcome::Restored)
        );
        assert_eq!(backend.values.get("key"), Some(&vec![]));
    }
    #[test]
    fn backup_failure_prevents_recovery_write() {
        let mut backend = Mock::default();
        backend.values.insert("key", vec![1]);
        let mut store = Store {
            fail: true,
            ..Default::default()
        };
        let result = recover_one(&mut backend, &mut store, &record(None, Some(vec![1])));
        assert_eq!(result, Err(RecoveryError::Persist("persist")));
        assert_eq!(backend.writes, 0);
        assert_eq!(backend.values.get("key"), Some(&vec![1]));
    }
    #[test]
    fn recovery_backup_includes_both_current_and_desired_state() {
        let mut backend = Mock::default();
        backend.values.insert("key", vec![1]);
        let mut store = Store::default();
        assert_eq!(
            recover_one(&mut backend, &mut store, &record(None, Some(vec![1]))),
            Ok(RecoveryOutcome::Restored)
        );
        assert_eq!(store.records, vec![(Some(vec![1]), None)]);
    }
    #[derive(Default)]
    struct Checkpoint {
        next: Vec<usize>,
        progress: usize,
        fail: bool,
    }
    impl RecoveryCheckpoint for Checkpoint {
        type Error = &'static str;
        fn next_exclusive(&self) -> usize {
            self.progress
        }
        fn persist_next(&mut self, next: usize) -> Result<(), Self::Error> {
            if self.fail {
                return Err("checkpoint");
            }
            self.next.push(next);
            self.progress = next;
            Ok(())
        }
    }
    #[test]
    fn reverse_order_restores_duplicate_targets() {
        let mut backend = Mock::default();
        backend.values.insert("key", vec![2]);
        let records = [
            record(None, Some(vec![1])),
            record(Some(vec![1]), Some(vec![2])),
        ];
        let mut checkpoint = Checkpoint::default();
        recover_reverse(
            &mut backend,
            &mut Store::default(),
            &mut checkpoint,
            &records,
            2,
        )
        .unwrap();
        assert!(!backend.values.contains_key("key"));
        assert_eq!(checkpoint.next, vec![1, 0]);
    }
    #[test]
    fn checkpoint_failure_stops_before_earlier_record_and_retry_resumes() {
        let mut backend = Mock::default();
        backend.values.insert("key", vec![2]);
        let records = [
            record(None, Some(vec![1])),
            record(Some(vec![1]), Some(vec![2])),
        ];
        let mut checkpoint = Checkpoint {
            fail: true,
            ..Default::default()
        };
        let mut store = Store::default();
        assert_eq!(
            recover_reverse(&mut backend, &mut store, &mut checkpoint, &records, 2),
            Err(BatchRecoveryError::Checkpoint {
                index: 1,
                error: "checkpoint"
            })
        );
        assert_eq!(backend.values.get("key"), Some(&vec![1]));
        checkpoint.fail = false;
        recover_reverse(&mut backend, &mut store, &mut checkpoint, &records, 2).unwrap();
        assert!(!backend.values.contains_key("key"));
        assert_eq!(checkpoint.next, vec![1, 0]);
    }
    #[test]
    fn conflict_stops_before_earlier_record_and_invalid_checkpoint_is_rejected() {
        let mut backend = Mock::default();
        backend.values.insert("key", vec![9]);
        let records = [
            record(None, Some(vec![1])),
            record(Some(vec![1]), Some(vec![2])),
        ];
        let mut checkpoint = Checkpoint::default();
        let mut store = Store::default();
        assert_eq!(
            recover_reverse(&mut backend, &mut store, &mut checkpoint, &records, 3),
            Err(BatchRecoveryError::InvalidCheckpoint)
        );
        assert_eq!(
            recover_reverse(&mut backend, &mut store, &mut checkpoint, &records, 2),
            Err(BatchRecoveryError::Operation {
                index: 1,
                error: RecoveryError::Conflict
            })
        );
        assert_eq!(backend.writes, 0);
        assert!(checkpoint.next.is_empty());
    }
}
