//! Journal write protocol and storage boundary.
//!
//! Persistent adapters must commit records and enumerate them after restart.
//! Cross-process locking, access control, and power-loss behavior remain
//! external requirements before this store may authorize real mutations.

use super::journal::{
    decode, decode_transaction_record, encode, encode_transaction_record, valid_transaction_id,
    RecordError, RegistryRecord, ServiceRecord, ServiceTarget, TransactionRecord, FORMAT_VERSION,
    SERVICE_FORMAT_VERSION,
};
use crate::engine::registry::{ValueState, ValueTarget};
use crate::engine::transaction::SnapshotStore;

pub trait JournalStorage {
    type Error;
    /// Reject a candidate before creating any file if the resulting log cannot
    /// be recovered within the storage implementation's limits.
    fn preflight(
        &mut self,
        _transaction: &str,
        _sequence: u64,
        _data: &[u8],
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Create a new record exclusively. Must never replace an existing record.
    fn create_new(
        &mut self,
        transaction: &str,
        sequence: u64,
        data: &[u8],
    ) -> Result<(), Self::Error>;
    /// Durably commit an already-created record so recovery can distinguish it
    /// from an incomplete write that never authorized a mutation.
    fn sync(&mut self, transaction: &str, sequence: u64) -> Result<(), Self::Error>;
    /// Read the record anew, bounded by the format's maximum permitted length.
    fn read(&mut self, transaction: &str, sequence: u64) -> Result<Vec<u8>, Self::Error>;
    /// Load the contiguous committed prefix and identify an incomplete tail.
    fn scan(&mut self, transaction: &str) -> Result<JournalScan, Self::Error>;
}

/// Validated durable records and an optional incomplete record at the tail.
#[derive(Debug, Default)]
pub struct JournalScan {
    /// Legacy projection for registry-only callers. Use `typed_committed` to
    /// validate sequence and resource kinds in mixed-resource transactions.
    pub committed: Vec<RegistryRecord>,
    /// Service-only projection; not sequence-complete for mixed transactions.
    pub service_committed: Vec<ServiceRecord>,
    /// Authoritative contiguous committed prefix, including every resource kind.
    pub typed_committed: Vec<TransactionRecord>,
    pub incomplete_tail: Option<u64>,
}

#[derive(Debug)]
pub enum JournalError<E> {
    Format(RecordError),
    Preflight(E),
    Create(E),
    Sync(E),
    Read(E),
    Scan(E),
    Mismatch,
    Exhausted,
    Poisoned,
    InvalidTransactionId,
    IncompleteTail(u64),
    UnsupportedRecordKind,
}

/// A transaction writer. The backend rejects duplicate records; callers must
/// hold the external transaction lock and must use `resume` for existing IDs.
pub struct RegistryJournal<S> {
    storage: S,
    transaction: String,
    next: u64,
    poisoned: bool,
}
impl<S> RegistryJournal<S> {
    #[cfg(test)]
    pub(crate) fn test_storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }
    pub fn new(storage: S, transaction: String) -> Self {
        Self {
            storage,
            transaction,
            next: 0,
            poisoned: false,
        }
    }
    pub fn into_storage(self) -> S {
        self.storage
    }
}

impl<S: JournalStorage> RegistryJournal<S> {
    /// Reopen a transaction only when the committed log is contiguous and has
    /// no incomplete trailing record. The caller must still hold the external
    /// transaction lock before using the returned writer.
    pub fn resume(storage: S, transaction: String) -> Result<Self, JournalError<S::Error>> {
        if !valid_transaction_id(&transaction) {
            return Err(JournalError::InvalidTransactionId);
        }
        let mut storage = storage;
        let scan = storage.scan(&transaction).map_err(JournalError::Scan)?;
        if let Some(sequence) = scan.incomplete_tail {
            return Err(JournalError::IncompleteTail(sequence));
        }
        if scan.typed_committed.len() != scan.committed.len()
            || scan
                .typed_committed
                .iter()
                .any(|record| !matches!(record, TransactionRecord::Registry(_)))
        {
            return Err(JournalError::UnsupportedRecordKind);
        }
        let next =
            u64::try_from(scan.typed_committed.len()).map_err(|_| JournalError::Exhausted)?;
        Ok(Self {
            storage,
            transaction,
            next,
            poisoned: false,
        })
    }
}

/// Journal writer for service startup-type snapshots. It shares the same
/// ordered storage protocol as registry records, while preserving the service
/// target as a distinct tagged record kind.
pub struct ServiceJournal<S> {
    storage: S,
    transaction: String,
    next: u64,
    poisoned: bool,
}

impl<S> ServiceJournal<S> {
    pub fn new(storage: S, transaction: String) -> Self {
        Self {
            storage,
            transaction,
            next: 0,
            poisoned: false,
        }
    }

    pub fn into_storage(self) -> S {
        self.storage
    }
}

impl<S: JournalStorage> ServiceJournal<S> {
    /// Resume only a transaction whose complete committed prefix contains
    /// service records. Use `TypedJournal` for mixed-resource transactions.
    pub fn resume(storage: S, transaction: String) -> Result<Self, JournalError<S::Error>> {
        if !valid_transaction_id(&transaction) {
            return Err(JournalError::InvalidTransactionId);
        }
        let mut storage = storage;
        let scan = storage.scan(&transaction).map_err(JournalError::Scan)?;
        if let Some(sequence) = scan.incomplete_tail {
            return Err(JournalError::IncompleteTail(sequence));
        }
        if scan.typed_committed.len() != scan.service_committed.len()
            || scan
                .typed_committed
                .iter()
                .any(|record| !matches!(record, TransactionRecord::Service(_)))
        {
            return Err(JournalError::UnsupportedRecordKind);
        }
        let next =
            u64::try_from(scan.typed_committed.len()).map_err(|_| JournalError::Exhausted)?;
        Ok(Self {
            storage,
            transaction,
            next,
            poisoned: false,
        })
    }
}

impl<S: JournalStorage> SnapshotStore<ServiceTarget, u32, u32> for ServiceJournal<S> {
    type Error = JournalError<S::Error>;

    fn persist(
        &mut self,
        target: &ServiceTarget,
        before: &u32,
        intended: &u32,
    ) -> Result<(), Self::Error> {
        if self.poisoned {
            return Err(JournalError::Poisoned);
        }
        let next = self.next.checked_add(1).ok_or(JournalError::Exhausted)?;
        let record = TransactionRecord::Service(ServiceRecord {
            version: SERVICE_FORMAT_VERSION,
            transaction_id: self.transaction.clone(),
            sequence: self.next,
            target: target.clone(),
            before_start_type: *before,
            intended_start_type: *intended,
        });
        let bytes = encode_transaction_record(&record).map_err(JournalError::Format)?;
        self.storage
            .preflight(&self.transaction, self.next, &bytes)
            .map_err(JournalError::Preflight)?;
        self.poisoned = true;
        self.storage
            .create_new(&self.transaction, self.next, &bytes)
            .map_err(JournalError::Create)?;
        self.storage
            .sync(&self.transaction, self.next)
            .map_err(JournalError::Sync)?;
        let read = self
            .storage
            .read(&self.transaction, self.next)
            .map_err(JournalError::Read)?;
        if read != bytes {
            return Err(JournalError::Mismatch);
        }
        let decoded = decode_transaction_record(&read).map_err(JournalError::Format)?;
        if decoded != record {
            return Err(JournalError::Mismatch);
        }
        self.next = next;
        self.poisoned = false;
        Ok(())
    }
}

/// A resource identity accepted by the typed journal writer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedTarget {
    Registry(ValueTarget),
    Service(ServiceTarget),
}

/// A resource state accepted by the typed journal writer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedState {
    Registry(ValueState),
    Service(u32),
}

/// Append/resume writer for a transaction containing registry and service
/// snapshots in one contiguous sequence.
pub struct TypedJournal<S> {
    storage: S,
    transaction: String,
    next: u64,
    poisoned: bool,
}

impl<S> TypedJournal<S> {
    #[cfg(test)]
    pub(crate) fn test_storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }
    pub fn new(storage: S, transaction: String) -> Self {
        Self {
            storage,
            transaction,
            next: 0,
            poisoned: false,
        }
    }

    pub fn into_storage(self) -> S {
        self.storage
    }
}

impl<S: JournalStorage> TypedJournal<S> {
    pub fn resume(storage: S, transaction: String) -> Result<Self, JournalError<S::Error>> {
        if !valid_transaction_id(&transaction) {
            return Err(JournalError::InvalidTransactionId);
        }
        let mut storage = storage;
        let scan = storage.scan(&transaction).map_err(JournalError::Scan)?;
        if let Some(sequence) = scan.incomplete_tail {
            return Err(JournalError::IncompleteTail(sequence));
        }
        if scan.typed_committed.len()
            != scan
                .committed
                .len()
                .checked_add(scan.service_committed.len())
                .ok_or(JournalError::Exhausted)?
        {
            return Err(JournalError::UnsupportedRecordKind);
        }
        for (index, record) in scan.typed_committed.iter().enumerate() {
            if record.transaction_id() != transaction
                || u64::try_from(index).ok() != Some(record.sequence())
            {
                return Err(JournalError::Mismatch);
            }
            encode_transaction_record(record).map_err(JournalError::Format)?;
        }
        let next =
            u64::try_from(scan.typed_committed.len()).map_err(|_| JournalError::Exhausted)?;
        Ok(Self {
            storage,
            transaction,
            next,
            poisoned: false,
        })
    }
}

impl<S: JournalStorage> SnapshotStore<TypedTarget, TypedState, TypedState> for TypedJournal<S> {
    type Error = JournalError<S::Error>;

    fn persist(
        &mut self,
        target: &TypedTarget,
        before: &TypedState,
        intended: &TypedState,
    ) -> Result<(), Self::Error> {
        if self.poisoned {
            return Err(JournalError::Poisoned);
        }
        let next = self.next.checked_add(1).ok_or(JournalError::Exhausted)?;
        let record = match (target, before, intended) {
            (
                TypedTarget::Registry(target),
                TypedState::Registry(before),
                TypedState::Registry(intended),
            ) => TransactionRecord::Registry(RegistryRecord {
                version: FORMAT_VERSION,
                transaction_id: self.transaction.clone(),
                sequence: self.next,
                target: target.clone(),
                before: before.clone(),
                intended: intended.clone(),
            }),
            (
                TypedTarget::Service(target),
                TypedState::Service(before),
                TypedState::Service(intended),
            ) => TransactionRecord::Service(ServiceRecord {
                version: SERVICE_FORMAT_VERSION,
                transaction_id: self.transaction.clone(),
                sequence: self.next,
                target: target.clone(),
                before_start_type: *before,
                intended_start_type: *intended,
            }),
            _ => return Err(JournalError::UnsupportedRecordKind),
        };
        let bytes = encode_transaction_record(&record).map_err(JournalError::Format)?;
        self.storage
            .preflight(&self.transaction, self.next, &bytes)
            .map_err(JournalError::Preflight)?;
        self.poisoned = true;
        self.storage
            .create_new(&self.transaction, self.next, &bytes)
            .map_err(JournalError::Create)?;
        self.storage
            .sync(&self.transaction, self.next)
            .map_err(JournalError::Sync)?;
        let read = self
            .storage
            .read(&self.transaction, self.next)
            .map_err(JournalError::Read)?;
        if read != bytes
            || decode_transaction_record(&read).map_err(JournalError::Format)? != record
        {
            return Err(JournalError::Mismatch);
        }
        self.next = next;
        self.poisoned = false;
        Ok(())
    }
}

impl<S: JournalStorage> SnapshotStore<ValueTarget, ValueState, ValueState> for RegistryJournal<S> {
    type Error = JournalError<S::Error>;
    fn persist(
        &mut self,
        target: &ValueTarget,
        before: &ValueState,
        intended: &ValueState,
    ) -> Result<(), Self::Error> {
        if self.poisoned {
            return Err(JournalError::Poisoned);
        }
        let next = self.next.checked_add(1).ok_or(JournalError::Exhausted)?;
        let record = RegistryRecord {
            version: FORMAT_VERSION,
            transaction_id: self.transaction.clone(),
            sequence: self.next,
            target: target.clone(),
            before: before.clone(),
            intended: intended.clone(),
        };
        let bytes = encode(&record).map_err(JournalError::Format)?;
        self.storage
            .preflight(&self.transaction, self.next, &bytes)
            .map_err(JournalError::Preflight)?;
        // An error may leave a partial or complete record; never reuse this writer.
        self.poisoned = true;
        self.storage
            .create_new(&self.transaction, self.next, &bytes)
            .map_err(JournalError::Create)?;
        self.storage
            .sync(&self.transaction, self.next)
            .map_err(JournalError::Sync)?;
        let read = self
            .storage
            .read(&self.transaction, self.next)
            .map_err(JournalError::Read)?;
        if read != bytes {
            return Err(JournalError::Mismatch);
        }
        let decoded = decode(&read).map_err(JournalError::Format)?;
        if decoded != record {
            return Err(JournalError::Mismatch);
        }
        self.next = next;
        self.poisoned = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::registry::{Hive, RegistryView};
    use std::collections::{HashMap, HashSet};
    #[derive(Default)]
    struct Storage {
        records: HashMap<(String, u64), Vec<u8>>,
        committed: HashSet<(String, u64)>,
        fail: Option<&'static str>,
        corrupt: bool,
    }
    impl JournalStorage for Storage {
        type Error = &'static str;
        fn preflight(&mut self, _: &str, _: u64, _: &[u8]) -> Result<(), Self::Error> {
            if self.fail == Some("preflight") {
                Err("preflight")
            } else {
                Ok(())
            }
        }
        fn create_new(&mut self, id: &str, seq: u64, data: &[u8]) -> Result<(), Self::Error> {
            if self.fail == Some("create") {
                return Err("create");
            }
            if self.records.contains_key(&(id.into(), seq)) {
                return Err("exists");
            }
            self.records.insert((id.into(), seq), data.into());
            Ok(())
        }
        fn sync(&mut self, id: &str, seq: u64) -> Result<(), Self::Error> {
            if self.fail == Some("sync") {
                Err("sync")
            } else {
                self.committed.insert((id.into(), seq));
                Ok(())
            }
        }
        fn read(&mut self, id: &str, seq: u64) -> Result<Vec<u8>, Self::Error> {
            if self.fail == Some("read") {
                return Err("read");
            }
            let mut data = self
                .records
                .get(&(id.into(), seq))
                .ok_or("missing")?
                .clone();
            if self.corrupt {
                data.push(b' ');
            }
            Ok(data)
        }
        fn scan(&mut self, id: &str) -> Result<JournalScan, Self::Error> {
            let mut sequences: Vec<_> = self
                .records
                .keys()
                .filter_map(|(tx, sequence)| (tx == id).then_some(*sequence))
                .collect();
            sequences.sort_unstable();
            let mut scan = JournalScan::default();
            for sequence in sequences {
                let bytes = self.records.get(&(id.into(), sequence)).ok_or("missing")?;
                if !self.committed.contains(&(id.into(), sequence)) {
                    if scan.incomplete_tail.replace(sequence).is_some() {
                        return Err("multiple incomplete records");
                    }
                    continue;
                }
                let record = decode_transaction_record(bytes).map_err(|_| "invalid record")?;
                if record.transaction_id() != id
                    || record.sequence() != scan.typed_committed.len() as u64
                {
                    return Err("invalid sequence");
                }
                if let TransactionRecord::Registry(registry_record) = &record {
                    scan.committed.push(registry_record.clone());
                } else if let TransactionRecord::Service(service_record) = &record {
                    scan.service_committed.push(service_record.clone());
                }
                scan.typed_committed.push(record);
            }
            Ok(scan)
        }
    }
    fn target() -> ValueTarget {
        ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Test".into(),
        }
    }
    #[test]
    fn all_journal_writers_stop_before_create_on_preflight_failure() {
        let storage = || Storage {
            fail: Some("preflight"),
            ..Default::default()
        };
        let mut registry = RegistryJournal::new(storage(), "tx".into());
        assert!(matches!(
            registry.persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing
            ),
            Err(JournalError::Preflight("preflight"))
        ));
        assert!(registry.into_storage().records.is_empty());
        let service_target = ServiceTarget {
            name: "Example".into(),
        };
        let mut service = ServiceJournal::new(storage(), "tx".into());
        assert!(matches!(
            service.persist(&service_target, &3, &4),
            Err(JournalError::Preflight("preflight"))
        ));
        assert!(service.into_storage().records.is_empty());
        let mut typed = TypedJournal::new(storage(), "tx".into());
        assert!(matches!(
            typed.persist(
                &TypedTarget::Service(service_target),
                &TypedState::Service(3),
                &TypedState::Service(4)
            ),
            Err(JournalError::Preflight("preflight"))
        ));
        assert!(typed.into_storage().records.is_empty());
    }

    #[test]
    fn new_unsupported_service_records_never_reach_storage() {
        for (name, before, intended) in [("old service", 3, 4), ("valid", 5, 4), ("valid", 3, 5)] {
            let target = ServiceTarget { name: name.into() };
            let mut service = ServiceJournal::new(Storage::default(), "tx".into());
            assert!(matches!(
                service.persist(&target, &before, &intended),
                Err(JournalError::Format(_))
            ));
            assert!(service.into_storage().records.is_empty());
            let mut typed = TypedJournal::new(Storage::default(), "tx".into());
            assert!(matches!(
                typed.persist(
                    &TypedTarget::Service(target),
                    &TypedState::Service(before),
                    &TypedState::Service(intended)
                ),
                Err(JournalError::Format(_))
            ));
            assert!(typed.into_storage().records.is_empty());
        }
    }

    #[test]
    fn typed_journal_writes_both_resource_kinds_and_resumes_sequence() {
        let mut journal = TypedJournal::new(Storage::default(), "tx".into());
        let service = TypedTarget::Service(ServiceTarget {
            name: "ExampleService".into(),
        });
        let registry = TypedTarget::Registry(target());
        assert!(matches!(
            journal.persist(
                &service,
                &TypedState::Registry(ValueState::ValueMissing),
                &TypedState::Registry(ValueState::ValueMissing),
            ),
            Err(JournalError::UnsupportedRecordKind)
        ));
        journal
            .persist(&service, &TypedState::Service(3), &TypedState::Service(4))
            .unwrap();
        journal
            .persist(
                &registry,
                &TypedState::Registry(ValueState::ValueMissing),
                &TypedState::Registry(ValueState::KeyMissing),
            )
            .unwrap();
        let mut storage = journal.into_storage();
        let records = storage.scan("tx").unwrap().typed_committed;
        assert_eq!(records.len(), 2);
        assert!(matches!(records[0], TransactionRecord::Service(_)));
        assert!(matches!(records[1], TransactionRecord::Registry(_)));

        let mut journal = TypedJournal::resume(storage, "tx".into()).unwrap();
        journal
            .persist(&service, &TypedState::Service(4), &TypedState::Service(2))
            .unwrap();
        let records = journal.into_storage().scan("tx").unwrap().typed_committed;
        assert_eq!(records.len(), 3);
        assert_eq!(records[2].sequence(), 2);
    }

    #[test]
    fn ordered_records_are_verified_and_readable() {
        let mut journal = RegistryJournal::new(Storage::default(), "tx".into());
        journal
            .persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        journal
            .persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let storage = journal.into_storage();
        assert_eq!(
            decode(&storage.records[&(String::from("tx"), 0)])
                .unwrap()
                .sequence,
            0
        );
        assert_eq!(
            decode(&storage.records[&(String::from("tx"), 1)])
                .unwrap()
                .sequence,
            1
        );
    }
    #[test]
    fn failures_poison_writer_without_reusing_sequence() {
        for failure in ["create", "sync", "read"] {
            let mut journal = RegistryJournal::new(
                Storage {
                    fail: Some(failure),
                    ..Default::default()
                },
                "tx".into(),
            );
            assert!(journal
                .persist(
                    &target(),
                    &ValueState::ValueMissing,
                    &ValueState::ValueMissing
                )
                .is_err());
            assert!(matches!(
                journal.persist(
                    &target(),
                    &ValueState::ValueMissing,
                    &ValueState::ValueMissing
                ),
                Err(JournalError::Poisoned)
            ));
        }
        let mut journal = RegistryJournal::new(
            Storage {
                corrupt: true,
                ..Default::default()
            },
            "tx".into(),
        );
        assert!(matches!(
            journal.persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing
            ),
            Err(JournalError::Mismatch)
        ));
        assert!(matches!(
            journal.persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing
            ),
            Err(JournalError::Poisoned)
        ));
    }
    #[test]
    fn duplicate_id_never_replaces_existing_record() {
        let mut first = RegistryJournal::new(Storage::default(), "tx".into());
        first
            .persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut second = RegistryJournal::new(first.into_storage(), "tx".into());
        assert!(matches!(
            second.persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing
            ),
            Err(JournalError::Create("exists"))
        ));
    }

    #[test]
    fn resume_continues_after_the_validated_commit_prefix() {
        let mut first = RegistryJournal::new(Storage::default(), "tx".into());
        first
            .persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut resumed = RegistryJournal::resume(first.into_storage(), "tx".into()).unwrap();
        resumed
            .persist(
                &target(),
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let storage = resumed.into_storage();
        assert_eq!(
            decode(&storage.records[&(String::from("tx"), 1)])
                .unwrap()
                .sequence,
            1
        );
    }

    #[test]
    fn resume_refuses_an_incomplete_tail() {
        let mut storage = Storage::default();
        let record = RegistryRecord {
            version: FORMAT_VERSION,
            transaction_id: "tx".into(),
            sequence: 0,
            target: target(),
            before: ValueState::ValueMissing,
            intended: ValueState::ValueMissing,
        };
        storage
            .records
            .insert((String::from("tx"), 0), encode(&record).unwrap());
        assert!(matches!(
            RegistryJournal::resume(storage, "tx".into()),
            Err(JournalError::IncompleteTail(0))
        ));
    }
}
