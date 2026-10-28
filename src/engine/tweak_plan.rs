//! Portable planning and mockable execution of declarative operations.
//!
//! This module does not establish trust, locking, durability, or permission to
//! mutate Windows. The caller must supply a separate authorization policy and
//! a backend and snapshot store satisfying the transaction contract.

use crate::data::catalog::CatalogOperation;
use crate::data::journal::{valid_service_name, MAX_RECORD_BYTES, SERVICE_START_TYPE_MAX};
use crate::data::store::{TypedState, TypedTarget};
use crate::engine::registry::{Hive, RegistryView, ValueState};
use crate::engine::transaction::{
    apply_one_if, ApplyOutcome, Backend, Operation, SnapshotStore, TransactionError,
};

/// An independent, caller-owned policy; catalog metadata is not authorization.
/// This is policy injection, not a security boundary or a grant of privileges.
/// The caller must independently enforce authorization and ensure its backend,
/// locks and snapshot store satisfy their respective safety contracts.
pub trait PlanGate {
    fn permits(&self, operations: &[CatalogOperation]) -> bool;
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlanError {
    Denied,
    Empty,
    InvalidOperation(usize),
    DuplicateTarget(usize),
}

/// An immutable, validated plan. Constructed only after the gate accepts it.
pub struct TweakPlan {
    operations: Vec<PlannedOperation>,
}

struct PlannedOperation {
    operation: Operation<TypedTarget, TypedState>,
    expected_start_type: Option<u32>,
}

impl PlannedOperation {
    fn permits(&self, state: &TypedState) -> bool {
        self.expected_start_type
            .is_none_or(|expected| *state == TypedState::Service(expected))
    }
}

impl TweakPlan {
    pub fn build<G: PlanGate>(
        operations: &[CatalogOperation],
        gate: &G,
    ) -> Result<Self, PlanError> {
        if operations.is_empty() {
            return Err(PlanError::Empty);
        }
        // Reject unbounded allocations and ambiguous Unicode case comparisons.
        if operations.len() > 4096 {
            return Err(PlanError::InvalidOperation(4096));
        }
        let mut result: Vec<PlannedOperation> = Vec::with_capacity(operations.len());
        for (index, operation) in operations.iter().enumerate() {
            let expected_start_type = match operation {
                CatalogOperation::SetServiceStartTypeIfExpected {
                    expected_start_type,
                    ..
                } => Some(*expected_start_type),
                _ => None,
            };
            let converted = match operation {
                CatalogOperation::SetRegistryValue { target, value } => {
                    if !valid_registry_target(target) || value.bytes.len() > MAX_RECORD_BYTES / 6 {
                        return Err(PlanError::InvalidOperation(index));
                    }
                    Operation {
                        target: TypedTarget::Registry(target.clone()),
                        change: TypedState::Registry(ValueState::Present(value.clone())),
                    }
                }
                CatalogOperation::DeleteRegistryValue { target } => {
                    if !valid_registry_target(target) {
                        return Err(PlanError::InvalidOperation(index));
                    }
                    Operation {
                        target: TypedTarget::Registry(target.clone()),
                        change: TypedState::Registry(ValueState::ValueMissing),
                    }
                }
                CatalogOperation::SetServiceStartType { target, start_type }
                | CatalogOperation::SetServiceStartTypeIfExpected {
                    target, start_type, ..
                } => {
                    if !target.name.is_ascii()
                        || !valid_service_name(&target.name)
                        || *start_type > SERVICE_START_TYPE_MAX
                        || expected_start_type.is_some_and(|value| value > SERVICE_START_TYPE_MAX)
                    {
                        return Err(PlanError::InvalidOperation(index));
                    }
                    Operation {
                        target: TypedTarget::Service(target.clone()),
                        change: TypedState::Service(*start_type),
                    }
                }
            };
            if result
                .iter()
                .any(|prior| aliases(&prior.operation.target, &converted.target))
            {
                return Err(PlanError::DuplicateTarget(index));
            }
            result.push(PlannedOperation {
                operation: converted,
                expected_start_type,
            });
        }
        if !gate.permits(operations) {
            return Err(PlanError::Denied);
        }
        Ok(Self { operations: result })
    }

    /// Apply a plan through the managed disk journal with a caller-supplied backend.
    /// The transaction lock lasts for the session; the mutation barrier covers
    /// each operation, not the entire plan. Stop using the session after any
    /// error and diagnose its journal before recovery: an incomplete tail is
    /// rejected by the recovery session and may require separate remediation.
    /// Authorization and backend correctness remain the caller's responsibility.
    #[cfg(windows)]
    pub fn apply_to_file_session<B>(
        &self,
        session: &mut crate::data::file_store::FileTypedTransactionSession,
        backend: &mut B,
    ) -> Result<Vec<usize>, FilePlanApplyError<B::Error>>
    where
        B: Backend<Target = TypedTarget, Change = TypedState, Snapshot = TypedState>,
    {
        self.apply_file_with(session, |session, operation| {
            let mut guarded = ExistingKeyBackend(backend);
            session.apply_if(&mut guarded, &operation.operation, |state| {
                operation.permits(state)
            })
        })
    }

    /// Execute through the guarded Windows backend and a durable typed journal.
    /// PlanGate is not a security boundary; production actions remain disabled
    /// pending the documented authorization, recovery and platform gates.
    #[cfg(windows)]
    pub fn apply_to_windows_file_session(
        &self,
        session: &mut crate::data::file_store::FileTypedTransactionSession,
    ) -> Result<Vec<usize>, FilePlanApplyError<crate::engine::services::TypedBackendError>> {
        self.apply_file_with(session, |session, operation| {
            session.apply_windows_if(&operation.operation, |state| operation.permits(state))
        })
    }

    #[cfg(windows)]
    fn apply_file_with<B>(
        &self,
        session: &mut crate::data::file_store::FileTypedTransactionSession,
        mut apply: impl FnMut(
            &mut crate::data::file_store::FileTypedTransactionSession,
            &PlannedOperation,
        ) -> Result<
            ApplyOutcome,
            crate::data::file_store::FileTypedTransactionError<
                QueryError<B>,
                crate::data::store::JournalError<std::io::Error>,
            >,
        >,
    ) -> Result<Vec<usize>, FilePlanApplyError<B>> {
        let mut skipped = Vec::new();
        for (index, operation) in self.operations.iter().enumerate() {
            match apply(session, operation) {
                Ok(ApplyOutcome::Applied) => {}
                Ok(ApplyOutcome::Skipped) => skipped.push(index),
                Err(error) => {
                    return Err(FilePlanApplyError {
                        index,
                        skipped,
                        error,
                    })
                }
            }
        }
        Ok(skipped)
    }

    /// Execute a prefix, stopping at the first error. This is not atomic.
    /// A failed apply can have partially changed the target; retain its snapshot.
    /// Callers must hold the appropriate lock and provide durable storage.
    pub fn apply<B, S>(
        &self,
        backend: &mut B,
        snapshots: &mut S,
    ) -> Result<Vec<usize>, ApplyError<B::Error, S::Error>>
    where
        B: Backend<Target = TypedTarget, Change = TypedState, Snapshot = TypedState>,
        S: SnapshotStore<TypedTarget, TypedState, TypedState>,
    {
        let mut skipped = Vec::new();
        for (index, operation) in self.operations.iter().enumerate() {
            let mut guarded = ExistingKeyBackend(backend);
            match apply_one_if(&mut guarded, snapshots, &operation.operation, |state| {
                operation.permits(state)
            }) {
                Ok(ApplyOutcome::Applied) => {}
                Ok(ApplyOutcome::Skipped) => skipped.push(index),
                Err(error) => {
                    return Err(ApplyError {
                        index,
                        skipped,
                        error,
                    })
                }
            }
        }
        Ok(skipped)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum QueryError<E> {
    Backend(E),
    MissingKey,
    InvalidResourceKind,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ApplyError<B, S> {
    /// Zero-based index; previous operations may have succeeded.
    pub index: usize,
    pub skipped: Vec<usize>,
    pub error: TransactionError<QueryError<B>, S>,
}

#[cfg(windows)]
#[derive(Debug)]
pub struct FilePlanApplyError<B> {
    pub index: usize,
    pub skipped: Vec<usize>,
    pub error: crate::data::file_store::FileTypedTransactionError<
        QueryError<B>,
        crate::data::store::JournalError<std::io::Error>,
    >,
}

pub(crate) struct ExistingKeyBackend<'a, B>(pub(crate) &'a mut B);
impl<B> Backend for ExistingKeyBackend<'_, B>
where
    B: Backend<Target = TypedTarget, Change = TypedState, Snapshot = TypedState>,
{
    type Target = TypedTarget;
    type Change = TypedState;
    type Snapshot = TypedState;
    type Error = QueryError<B::Error>;
    fn query(&mut self, target: &TypedTarget) -> Result<TypedState, Self::Error> {
        let state = self.0.query(target).map_err(QueryError::Backend)?;
        match (target, &state) {
            (TypedTarget::Registry(_), TypedState::Registry(ValueState::KeyMissing)) => {
                Err(QueryError::MissingKey)
            }
            (TypedTarget::Registry(_), TypedState::Registry(_))
            | (TypedTarget::Service(_), TypedState::Service(_)) => Ok(state),
            _ => Err(QueryError::InvalidResourceKind),
        }
    }
    fn apply(&mut self, target: &TypedTarget, change: &TypedState) -> Result<(), Self::Error> {
        self.0.apply(target, change).map_err(QueryError::Backend)
    }
}

fn valid_registry_target(target: &crate::engine::registry::ValueTarget) -> bool {
    // HKEY_USERS can refer to any profile and may be loaded, unloaded, or
    // replaced across restart. Until the journal binds a profile identity,
    // typed mutation is restricted to the machine hive.
    matches!(target.hive, Hive::LocalMachine)
        && !matches!(target.view, RegistryView::Registry32)
        && target.key_path.is_ascii()
        && target.value_name.is_ascii()
        && !target.key_path.is_empty()
        && !target.key_path.contains('/')
        && !target.key_path.contains('\0')
        && !target.value_name.contains('\0')
        && target
            .key_path
            .split('\\')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && target.key_path.len() <= 16_384
        && target.value_name.len() <= 16_384
}

// Treat both accepted views as potentially aliased. All accepted names are ASCII.
fn aliases(a: &TypedTarget, b: &TypedTarget) -> bool {
    match (a, b) {
        (TypedTarget::Registry(a), TypedTarget::Registry(b)) => {
            a.hive == b.hive
                && a.key_path.eq_ignore_ascii_case(&b.key_path)
                && a.value_name.eq_ignore_ascii_case(&b.value_name)
        }
        (TypedTarget::Service(a), TypedTarget::Service(b)) => a.name.eq_ignore_ascii_case(&b.name),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::journal::ServiceTarget;
    use crate::engine::recovery::{recover_typed_one, RecoveryRecord, TypedRecoveryBackend};
    use crate::engine::registry::{RegistryValue, ValueTarget};

    struct Gate(bool);
    impl PlanGate for Gate {
        fn permits(&self, _: &[CatalogOperation]) -> bool {
            self.0
        }
    }
    fn target(name: &str) -> ValueTarget {
        ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Test".into(),
            value_name: name.into(),
        }
    }
    fn set(name: &str) -> CatalogOperation {
        CatalogOperation::SetRegistryValue {
            target: target(name),
            value: RegistryValue {
                native_type: 7,
                bytes: vec![0, 255, 0, 42],
            },
        }
    }
    fn service(name: &str) -> CatalogOperation {
        CatalogOperation::SetServiceStartType {
            target: ServiceTarget { name: name.into() },
            start_type: 4,
        }
    }
    #[derive(Default)]
    struct Mock {
        states: Vec<(TypedTarget, TypedState)>,
        queries: usize,
        writes: usize,
        fail_query: Option<usize>,
        fail_apply: Option<usize>,
    }
    impl Backend for Mock {
        type Target = TypedTarget;
        type Change = TypedState;
        type Snapshot = TypedState;
        type Error = &'static str;
        fn query(&mut self, target: &TypedTarget) -> Result<TypedState, Self::Error> {
            let index = self.queries;
            self.queries += 1;
            if self.fail_query == Some(index) {
                return Err("query");
            }
            Ok(self
                .states
                .iter()
                .find(|(t, _)| t == target)
                .ok_or("missing")?
                .1
                .clone())
        }
        fn apply(&mut self, target: &TypedTarget, state: &TypedState) -> Result<(), Self::Error> {
            let index = self.writes;
            self.writes += 1;
            self.states.iter_mut().find(|(t, _)| t == target).unwrap().1 = state.clone();
            if self.fail_apply == Some(index) {
                return Err("apply");
            }
            Ok(())
        }
    }
    impl TypedRecoveryBackend for Mock {
        type Error = &'static str;
        fn query_typed(&mut self, target: &TypedTarget) -> Result<TypedState, Self::Error> {
            Backend::query(self, target)
        }
        fn restore_typed(
            &mut self,
            target: &TypedTarget,
            state: &TypedState,
        ) -> Result<(), Self::Error> {
            Backend::apply(self, target, state)
        }
    }
    #[derive(Default)]
    struct Snapshots {
        records: Vec<RecoveryRecord<TypedTarget, TypedState>>,
        fail: bool,
    }
    impl SnapshotStore<TypedTarget, TypedState, TypedState> for Snapshots {
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
            self.records.push(RecoveryRecord {
                target: target.clone(),
                before: before.clone(),
                intended: intended.clone(),
            });
            Ok(())
        }
    }
    fn mock() -> Mock {
        Mock {
            states: vec![
                (
                    TypedTarget::Registry(target("a")),
                    TypedState::Registry(ValueState::Present(RegistryValue {
                        native_type: 99,
                        bytes: vec![9, 0, 9],
                    })),
                ),
                (
                    TypedTarget::Registry(target("b")),
                    TypedState::Registry(ValueState::ValueMissing),
                ),
                (
                    TypedTarget::Service(ServiceTarget { name: "Svc".into() }),
                    TypedState::Service(3),
                ),
            ],
            ..Mock::default()
        }
    }
    #[cfg(windows)]
    struct DiskRoot(std::path::PathBuf);
    #[cfg(windows)]
    impl DiskRoot {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path =
                std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(format!(
                    "rustweaker-plan-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            crate::data::file_store::FileJournalStorage::create_private(&path).unwrap();
            Self(path)
        }
    }
    #[cfg(windows)]
    impl Drop for DiskRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(windows)]
    struct DiskMock {
        inner: Mock,
        root: std::path::PathBuf,
        transaction: &'static str,
        recovery: &'static str,
        restored: Vec<TypedTarget>,
    }
    #[cfg(windows)]
    impl DiskMock {
        fn marker(&self, id: &str, sequence: usize) -> bool {
            self.root
                .join(format!("{:03}-{id}-{sequence:020}.commit", id.len()))
                .is_file()
        }
    }
    #[cfg(windows)]
    impl Backend for DiskMock {
        type Target = TypedTarget;
        type Change = TypedState;
        type Snapshot = TypedState;
        type Error = &'static str;
        fn query(&mut self, target: &TypedTarget) -> Result<TypedState, Self::Error> {
            assert!(
                crate::data::file_store::FileJournalStorage::open(&self.root)
                    .unwrap()
                    .lock_system_mutations()
                    .is_err(),
                "query must run under the mutation barrier"
            );
            Backend::query(&mut self.inner, target)
        }
        fn apply(&mut self, target: &TypedTarget, change: &TypedState) -> Result<(), Self::Error> {
            assert!(
                crate::data::file_store::FileJournalStorage::open(&self.root)
                    .unwrap()
                    .lock_system_mutations()
                    .is_err(),
                "apply must run under the mutation barrier"
            );
            assert!(
                self.marker(self.transaction, self.inner.writes),
                "backup marker must precede apply"
            );
            Backend::apply(&mut self.inner, target, change)
        }
    }
    #[cfg(windows)]
    impl TypedRecoveryBackend for DiskMock {
        type Error = &'static str;
        fn query_typed(&mut self, target: &TypedTarget) -> Result<TypedState, Self::Error> {
            Backend::query(&mut self.inner, target)
        }
        fn restore_typed(
            &mut self,
            target: &TypedTarget,
            state: &TypedState,
        ) -> Result<(), Self::Error> {
            assert!(
                self.marker(self.recovery, self.restored.len()),
                "recovery backup must precede restore"
            );
            self.restored.push(target.clone());
            Backend::apply(&mut self.inner, target, state)
        }
    }
    #[cfg(windows)]
    fn disk_mock(root: &DiskRoot) -> DiskMock {
        DiskMock {
            inner: mock(),
            root: root.0.clone(),
            transaction: "plan_tx",
            recovery: "plan_undo",
            restored: Vec::new(),
        }
    }

    #[cfg(windows)]
    #[test]
    fn file_session_partial_failure_reopens_and_recovers_in_reverse() {
        use crate::data::file_store::{
            FileJournalStorage, FileTypedRecoverySession, FileTypedTransactionError,
        };
        use crate::data::journal::TransactionRecord;
        use crate::data::store::JournalStorage;
        let root = DiskRoot::new();
        let plan = TweakPlan::build(&[set("a"), set("b"), service("Svc")], &Gate(true)).unwrap();
        let mut backend = disk_mock(&root);
        let original = backend.inner.states.clone();
        backend.inner.fail_apply = Some(1);
        let mut session =
            FileJournalStorage::start_typed_transaction(&root.0, backend.transaction).unwrap();
        let error = plan
            .apply_to_file_session(&mut session, &mut backend)
            .unwrap_err();
        assert_eq!(error.index, 1);
        assert!(matches!(
            error.error,
            FileTypedTransactionError::Operation(TransactionError::Apply(QueryError::Backend(
                "apply"
            )))
        ));
        assert_eq!(backend.inner.writes, 2);
        drop(session);
        let mut storage = FileJournalStorage::open(&root.0)
            .unwrap()
            .lock_transaction(backend.transaction)
            .unwrap();
        let scan = storage.scan(backend.transaction).unwrap();
        assert!(scan.incomplete_tail.is_none());
        assert_eq!(scan.typed_committed.len(), 2);
        assert!(
            matches!(&scan.typed_committed[0], TransactionRecord::Registry(r) if r.before == match &original[0].1 { TypedState::Registry(v) => v.clone(), _ => unreachable!() })
        );
        assert!(
            matches!(&scan.typed_committed[1], TransactionRecord::Registry(r) if r.before == ValueState::ValueMissing)
        );
        drop(storage);
        backend.inner.fail_apply = None;
        let mut recovery =
            FileTypedRecoverySession::start_with_id(&root.0, backend.transaction, backend.recovery)
                .unwrap();
        recovery.run(&mut backend).unwrap();
        assert_eq!(recovery.next_exclusive(), 0);
        drop(recovery);
        assert_eq!(
            backend.restored,
            vec![original[1].0.clone(), original[0].0.clone()]
        );
        assert_eq!(backend.inner.states, original);
        let mut storage = FileJournalStorage::open(&root.0)
            .unwrap()
            .lock_transaction(backend.recovery)
            .unwrap();
        let scan = storage.scan(backend.recovery).unwrap();
        assert_eq!(scan.typed_committed.len(), 2);
        assert!(
            matches!(&scan.typed_committed[0], TransactionRecord::Registry(r) if r.before == ValueState::Present(RegistryValue { native_type: 7, bytes: vec![0, 255, 0, 42] }) && r.intended == ValueState::ValueMissing)
        );
        assert!(
            matches!(&scan.typed_committed[1], TransactionRecord::Registry(r) if r.before == ValueState::Present(RegistryValue { native_type: 7, bytes: vec![0, 255, 0, 42] }) && r.intended == match &original[0].1 { TypedState::Registry(v) => v.clone(), _ => unreachable!() })
        );
    }

    #[cfg(windows)]
    #[test]
    fn file_session_query_missing_key_and_persist_fail_without_mutation() {
        use crate::data::file_store::{FileJournalStorage, FileTypedTransactionError};
        use crate::data::store::JournalStorage;
        for mode in 0..3 {
            let root = DiskRoot::new();
            let mut backend = disk_mock(&root);
            match mode {
                0 => backend.inner.fail_query = Some(0),
                1 => backend.inner.states[0].1 = TypedState::Registry(ValueState::KeyMissing),
                _ => {
                    backend.inner.states[0].1 =
                        TypedState::Registry(ValueState::Present(RegistryValue {
                            native_type: 7,
                            bytes: vec![0; MAX_RECORD_BYTES],
                        }))
                }
            }
            let plan = TweakPlan::build(&[set("a")], &Gate(true)).unwrap();
            let mut session =
                FileJournalStorage::start_typed_transaction(&root.0, backend.transaction).unwrap();
            let error = plan
                .apply_file_with(&mut session, |session, operation| {
                    session.apply_guarded_mock_if(&mut backend, &operation.operation, |state| {
                        operation.permits(state)
                    })
                })
                .unwrap_err();
            assert_eq!(error.index, 0);
            match mode {
                0 => assert!(matches!(
                    error.error,
                    FileTypedTransactionError::Operation(TransactionError::Query(
                        QueryError::Backend("query")
                    ))
                )),
                1 => assert!(matches!(
                    error.error,
                    FileTypedTransactionError::Operation(TransactionError::Query(
                        QueryError::MissingKey
                    ))
                )),
                _ => assert!(matches!(
                    error.error,
                    FileTypedTransactionError::Operation(TransactionError::Persist(_))
                )),
            }
            assert_eq!(backend.inner.writes, 0);
            drop(session);
            let mut storage = FileJournalStorage::open(&root.0)
                .unwrap()
                .lock_transaction(backend.transaction)
                .unwrap();
            assert!(storage
                .scan(backend.transaction)
                .unwrap()
                .typed_committed
                .is_empty());
        }
    }

    #[cfg(windows)]
    #[test]
    fn guarded_plan_lock_failure_does_not_query_or_write() {
        use crate::data::file_store::{FileJournalStorage, FileTypedTransactionError};
        let root = DiskRoot::new();
        let mut backend = disk_mock(&root);
        let plan = TweakPlan::build(&[conditional(3)], &Gate(true)).unwrap();
        let mut session =
            FileJournalStorage::start_typed_transaction(&root.0, backend.transaction).unwrap();
        let storage = FileJournalStorage::open(&root.0).unwrap();
        let lock = storage.lock_system_mutations().unwrap();
        let error = plan
            .apply_file_with(&mut session, |session, operation| {
                session.apply_guarded_mock_if(&mut backend, &operation.operation, |state| {
                    operation.permits(state)
                })
            })
            .unwrap_err();
        assert_eq!(error.index, 0);
        assert!(matches!(
            error.error,
            FileTypedTransactionError::MutationLock(_)
        ));
        assert_eq!(backend.inner.queries, 0);
        assert_eq!(backend.inner.writes, 0);
        drop(lock);
    }

    fn conditional(expected_start_type: u32) -> CatalogOperation {
        CatalogOperation::SetServiceStartTypeIfExpected {
            target: ServiceTarget { name: "Svc".into() },
            start_type: 4,
            expected_start_type,
        }
    }

    #[test]
    fn gate_receives_the_conditional_operation() {
        struct GuardGate;
        impl PlanGate for GuardGate {
            fn permits(&self, operations: &[CatalogOperation]) -> bool {
                matches!(
                    operations,
                    [CatalogOperation::SetServiceStartTypeIfExpected {
                        expected_start_type: 3,
                        ..
                    }]
                )
            }
        }
        assert!(TweakPlan::build(&[conditional(3)], &GuardGate).is_ok());
        assert!(matches!(
            TweakPlan::build(&[conditional(2)], &GuardGate),
            Err(PlanError::Denied)
        ));
    }

    #[test]
    fn conditional_service_uses_snapshot_and_reports_skips_and_failures() {
        let plan = TweakPlan::build(&[conditional(2), set("a")], &Gate(true)).unwrap();
        let mut backend = mock();
        let mut snapshots = Snapshots::default();
        assert_eq!(plan.apply(&mut backend, &mut snapshots).unwrap(), vec![0]);
        assert_eq!(snapshots.records.len(), 1);
        assert_eq!(backend.states[2].1, TypedState::Service(3));
        let mut backend = mock();
        backend.fail_query = Some(1);
        let error = plan
            .apply(&mut backend, &mut Snapshots::default())
            .unwrap_err();
        assert_eq!(error.index, 1);
        assert_eq!(error.skipped, vec![0]);
        let plan = TweakPlan::build(&[conditional(3)], &Gate(true)).unwrap();
        let mut backend = mock();
        let mut snapshots = Snapshots::default();
        assert!(plan.apply(&mut backend, &mut snapshots).unwrap().is_empty());
        assert_eq!(snapshots.records[0].before, TypedState::Service(3));
        assert_eq!(backend.states[2].1, TypedState::Service(4));
        let mut backend = mock();
        backend.states.pop();
        let mut snapshots = Snapshots::default();
        assert!(matches!(
            plan.apply(&mut backend, &mut snapshots),
            Err(ApplyError {
                error: TransactionError::Query(QueryError::Backend("missing")),
                ..
            })
        ));
        assert!(snapshots.records.is_empty());
        assert_eq!(backend.writes, 0);
        let mut backend = mock();
        let mut snapshots = Snapshots {
            fail: true,
            ..Snapshots::default()
        };
        assert!(matches!(
            plan.apply(&mut backend, &mut snapshots),
            Err(ApplyError {
                error: TransactionError::Persist("persist"),
                ..
            })
        ));
        assert_eq!(backend.writes, 0);
        assert!(matches!(
            TweakPlan::build(&[conditional(5)], &Gate(true)),
            Err(PlanError::InvalidOperation(0))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn conditional_disk_journal_skips_and_recovers_partial_batch() {
        use crate::data::file_store::{FileJournalStorage, FileTypedRecoverySession};
        use crate::data::store::JournalStorage;
        let root = DiskRoot::new();
        let plan = TweakPlan::build(&[conditional(2), set("a"), set("b")], &Gate(true)).unwrap();
        let mut backend = disk_mock(&root);
        let original = backend.inner.states.clone();
        backend.inner.fail_apply = Some(1);
        let mut session =
            FileJournalStorage::start_typed_transaction(&root.0, backend.transaction).unwrap();
        let error = plan
            .apply_file_with(&mut session, |session, operation| {
                session.apply_guarded_mock_if(&mut backend, &operation.operation, |state| {
                    operation.permits(state)
                })
            })
            .unwrap_err();
        assert_eq!(error.index, 2);
        assert_eq!(error.skipped, vec![0]);
        drop(session);
        let mut storage = FileJournalStorage::open(&root.0)
            .unwrap()
            .lock_transaction(backend.transaction)
            .unwrap();
        let scan = storage.scan(backend.transaction).unwrap();
        assert_eq!(scan.typed_committed.len(), 2);
        assert!(scan
            .typed_committed
            .iter()
            .all(|r| matches!(r, crate::data::journal::TransactionRecord::Registry(_))));
        drop(storage);
        backend.inner.fail_apply = None;
        let mut recovery =
            FileTypedRecoverySession::start_with_id(&root.0, backend.transaction, backend.recovery)
                .unwrap();
        recovery.run(&mut backend).unwrap();
        assert_eq!(backend.inner.states, original);
        assert_eq!(
            backend.restored,
            vec![original[1].0.clone(), original[0].0.clone()]
        );
    }

    #[cfg(windows)]
    #[test]
    fn conditional_disk_match_and_query_failure() {
        use crate::data::file_store::FileJournalStorage;
        use crate::data::store::JournalStorage;
        for fail in [false, true] {
            let root = DiskRoot::new();
            let mut backend = disk_mock(&root);
            if fail {
                backend.inner.fail_query = Some(0);
            }
            let plan = TweakPlan::build(&[conditional(3)], &Gate(true)).unwrap();
            let mut session =
                FileJournalStorage::start_typed_transaction(&root.0, backend.transaction).unwrap();
            let result = plan.apply_file_with(&mut session, |session, operation| {
                session.apply_guarded_mock_if(&mut backend, &operation.operation, |state| {
                    operation.permits(state)
                })
            });
            assert_eq!(result.is_err(), fail);
            drop(session);
            let mut storage = FileJournalStorage::open(&root.0)
                .unwrap()
                .lock_transaction(backend.transaction)
                .unwrap();
            let scan = storage.scan(backend.transaction).unwrap();
            assert_eq!(scan.typed_committed.len(), usize::from(!fail));
            if !fail {
                assert!(
                    matches!(&scan.typed_committed[0], crate::data::journal::TransactionRecord::Service(r) if r.before_start_type == 3 && r.intended_start_type == 4)
                );
            }
        }
    }

    #[test]
    fn validation_and_independent_gate_fail_before_execution() {
        assert!(matches!(
            TweakPlan::build(&[set("a")], &Gate(false)),
            Err(PlanError::Denied)
        ));
        let mut other = target("A");
        other.view = RegistryView::Native;
        assert!(matches!(
            TweakPlan::build(
                &[
                    set("a"),
                    CatalogOperation::DeleteRegistryValue { target: other }
                ],
                &Gate(true)
            ),
            Err(PlanError::DuplicateTarget(1))
        ));
        assert!(matches!(
            TweakPlan::build(&[service("Svc"), service("sVC")], &Gate(true)),
            Err(PlanError::DuplicateTarget(1))
        ));
        let mut bad = target("b");
        bad.hive = Hive::CurrentUser;
        assert!(matches!(
            TweakPlan::build(
                &[
                    set("a"),
                    CatalogOperation::DeleteRegistryValue { target: bad }
                ],
                &Gate(true)
            ),
            Err(PlanError::InvalidOperation(1))
        ));
        let mut bad = target("b");
        bad.hive = Hive::Users;
        assert!(matches!(
            TweakPlan::build(
                &[CatalogOperation::SetRegistryValue {
                    target: bad.clone(),
                    value: RegistryValue {
                        native_type: 7,
                        bytes: vec![1]
                    },
                }],
                &Gate(true)
            ),
            Err(PlanError::InvalidOperation(0))
        ));
        assert!(matches!(
            TweakPlan::build(
                &[CatalogOperation::DeleteRegistryValue {
                    target: bad.clone()
                }],
                &Gate(true)
            ),
            Err(PlanError::InvalidOperation(0))
        ));
        assert!(matches!(
            TweakPlan::build(
                &[
                    set("a"),
                    CatalogOperation::DeleteRegistryValue { target: bad }
                ],
                &Gate(true)
            ),
            Err(PlanError::InvalidOperation(1))
        ));
        let mut bad = target("b");
        bad.view = RegistryView::Registry32;
        assert!(matches!(
            TweakPlan::build(
                &[CatalogOperation::DeleteRegistryValue { target: bad }],
                &Gate(true)
            ),
            Err(PlanError::InvalidOperation(0))
        ));
        assert!(matches!(
            TweakPlan::build(
                &[CatalogOperation::SetServiceStartType {
                    target: ServiceTarget { name: "Svc".into() },
                    start_type: 5
                }],
                &Gate(true)
            ),
            Err(PlanError::InvalidOperation(0))
        ));
    }
    #[test]
    fn exact_values_absence_and_reverse_recovery() {
        let entries = [set("a"), set("b"), service("Svc")];
        let plan = TweakPlan::build(&entries, &Gate(true)).unwrap();
        let mut backend = mock();
        let original = backend.states.clone();
        let mut store = Snapshots::default();
        plan.apply(&mut backend, &mut store).unwrap();
        assert_eq!(store.records[0].before, original[0].1);
        assert_eq!(
            store.records[1].before,
            TypedState::Registry(ValueState::ValueMissing)
        );
        assert_eq!(
            store.records[0].intended,
            TypedState::Registry(ValueState::Present(RegistryValue {
                native_type: 7,
                bytes: vec![0, 255, 0, 42]
            }))
        );
        let mut undo = Snapshots::default();
        for record in store.records.iter().rev() {
            recover_typed_one(&mut backend, &mut undo, record).unwrap();
        }
        assert_eq!(backend.states, original);
        assert_eq!(undo.records.len(), 3);
        let deletion = TweakPlan::build(
            &[CatalogOperation::DeleteRegistryValue {
                target: target("a"),
            }],
            &Gate(true),
        )
        .unwrap();
        deletion
            .apply(&mut backend, &mut Snapshots::default())
            .unwrap();
        assert_eq!(
            backend.states[0].1,
            TypedState::Registry(ValueState::ValueMissing)
        );
    }
    #[test]
    fn failures_stop_batch_and_preserve_snapshots() {
        let plan = TweakPlan::build(&[set("a"), set("b"), service("Svc")], &Gate(true)).unwrap();
        let mut backend = mock();
        backend.fail_query = Some(0);
        let mut store = Snapshots::default();
        assert!(matches!(
            plan.apply(&mut backend, &mut store),
            Err(ApplyError {
                index: 0,
                skipped: _,
                error: TransactionError::Query(QueryError::Backend("query"))
            })
        ));
        assert_eq!(backend.writes, 0);
        assert!(store.records.is_empty());
        let mut backend = mock();
        let mut store = Snapshots {
            fail: true,
            ..Snapshots::default()
        };
        assert!(matches!(
            plan.apply(&mut backend, &mut store),
            Err(ApplyError {
                index: 0,
                skipped: _,
                error: TransactionError::Persist("persist")
            })
        ));
        assert_eq!(backend.writes, 0);
        let mut backend = mock();
        backend.fail_apply = Some(1);
        let mut store = Snapshots::default();
        assert!(matches!(
            plan.apply(&mut backend, &mut store),
            Err(ApplyError {
                index: 1,
                skipped: _,
                error: TransactionError::Apply(QueryError::Backend("apply"))
            })
        ));
        assert_eq!(backend.writes, 2);
        assert_eq!(store.records.len(), 2);
        assert_eq!(backend.states[2].1, TypedState::Service(3));
        backend.fail_apply = None;
        let mut undo = Snapshots::default();
        for record in store.records.iter().rev() {
            recover_typed_one(&mut backend, &mut undo, record).unwrap();
        }
        assert_eq!(backend.states, mock().states);
        let mut backend = mock();
        backend.states[0].1 = TypedState::Registry(ValueState::KeyMissing);
        let mut store = Snapshots::default();
        assert!(matches!(
            plan.apply(&mut backend, &mut store),
            Err(ApplyError {
                index: 0,
                skipped: _,
                error: TransactionError::Query(QueryError::MissingKey)
            })
        ));
        assert_eq!(backend.writes, 0);
        assert!(store.records.is_empty());
    }

    #[test]
    fn reviewed_fixture_is_only_exercised_with_mocks_and_explicit_gate() {
        let fixture: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../config/catalog.registry-review.v1.json"
        ))
        .unwrap();
        let tweaks = fixture["tweaks"].as_array().unwrap();
        let accepted: std::collections::BTreeSet<&str> = [
            "WPFTweaksActivity",
            "WPFTweaksRevertStartMenu",
            "WPFTweaksBraveDebloat",
            "WPFTweaksConsumerFeatures",
            "WPFTweaksDeliveryOptimization",
            "WPFTweaksWPBT",
            "WPFTweaksPreventDeviceMetadataFromNetwork",
            "WPFTweaksIPv46",
            "WPFToggleDetailedBSoD",
            "WPFToggleVerboseLogon",
            "WPFToggleS3Sleep",
            "WPFToggleLoginBlur",
            "WPFTweaksDisableLockscreen",
            "WPFToggleLongPaths",
            "WPFTweaksEdgeDebloat",
            "WPFTweaksUTC",
        ]
        .into_iter()
        .collect();
        // HKCU and HKU cannot be recovered safely until the target user/profile
        // identity is bound to the transaction and remains verifiable on recovery.
        let rejected: std::collections::BTreeMap<&str, (usize, Hive)> = [
            (
                "WPFTweaksDisableWarningForUnsignedRdp",
                (1, Hive::CurrentUser),
            ),
            ("WPFTweaksRemoveHomeAndGallery", (0, Hive::CurrentUser)),
            ("WPFTweaksEndTaskOnTaskbar", (0, Hive::CurrentUser)),
            ("WPFTweaksStorage", (0, Hive::CurrentUser)),
            ("WPFTweaksDisableNotifications", (0, Hive::CurrentUser)),
            ("WPFTweaksDisableBGapps", (0, Hive::CurrentUser)),
            ("WPFToggleBatteryPercentage", (0, Hive::CurrentUser)),
            ("WPFToggleNewOutlook", (0, Hive::CurrentUser)),
            ("WPFToggleScrollbars", (0, Hive::CurrentUser)),
            ("WPFToggleMouseAcceleration", (0, Hive::CurrentUser)),
            ("WPFToggleNumLock", (0, Hive::Users)),
            ("WPFToggleWindowSnapping", (0, Hive::CurrentUser)),
            ("WPFToggleStandbyFix", (0, Hive::CurrentUser)),
            ("WPFToggleHideSettingsHome", (0, Hive::CurrentUser)),
            ("WPFToggleBingSearch", (0, Hive::CurrentUser)),
            ("WPFToggleStickyKeys", (0, Hive::CurrentUser)),
            ("WPFToggleTaskbarSearch", (0, Hive::CurrentUser)),
            ("WPFToggleTaskView", (0, Hive::CurrentUser)),
            ("WPFToggleGameMode", (0, Hive::CurrentUser)),
        ]
        .into_iter()
        .collect();
        let mut encountered = std::collections::BTreeSet::new();
        let mut checked = 0;
        for tweak in tweaks {
            let id = tweak["id"].as_str().unwrap();
            assert!(encountered.insert(id), "duplicate fixture ID: {id}");
            assert!(
                accepted.contains(id) || rejected.contains_key(id),
                "unclassified fixture ID: {id}"
            );
            let operations: Vec<CatalogOperation> =
                serde_json::from_value(tweak["operations"].clone()).unwrap();
            checked += operations.len();
            if let Some((index, expected_hive)) = rejected.get(id) {
                let CatalogOperation::SetRegistryValue { target, .. } = &operations[*index] else {
                    panic!("expected HKCU registry set for {id}");
                };
                assert_eq!(&target.hive, expected_hive);
                assert!(
                    matches!(TweakPlan::build(&operations, &Gate(true)), Err(PlanError::InvalidOperation(i)) if i == *index)
                );
                continue;
            }
            match TweakPlan::build(&operations, &Gate(true)) {
                Ok(plan) => {
                    assert!(matches!(
                        TweakPlan::build(&operations, &Gate(false)),
                        Err(PlanError::Denied)
                    ));
                    let mut backend = Mock::default();
                    for op in &plan.operations {
                        let initial = match &op.operation.target {
                            TypedTarget::Registry(_) => {
                                TypedState::Registry(ValueState::ValueMissing)
                            }
                            TypedTarget::Service(_) => TypedState::Service(3),
                        };
                        backend.states.push((op.operation.target.clone(), initial));
                    }
                    let original = backend.states.clone();
                    let mut snapshots = Snapshots::default();
                    plan.apply(&mut backend, &mut snapshots).unwrap();
                    let mut undo = Snapshots::default();
                    for record in snapshots.records.iter().rev() {
                        recover_typed_one(&mut backend, &mut undo, record).unwrap();
                    }
                    assert_eq!(backend.states, original);
                }
                Err(other) => panic!("unexpected fixture rejection: {other:?}"),
            }
        }
        assert_eq!(
            encountered,
            accepted
                .union(&rejected.keys().copied().collect())
                .copied()
                .collect()
        );
        assert_eq!(checked, 76);
    }
}
