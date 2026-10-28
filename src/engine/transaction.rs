//! Capture and persist a target's exact pre-state before applying a change.
//!
//! Implementations must serialize access to their targets and ensure `persist`
//! really is durable and validated. This coordinator alone cannot guarantee
//! crash recovery, concurrent-change detection, or filesystem durability.

/// A target and its intended change.
pub struct Operation<T, C> {
    pub target: T,
    pub change: C,
}

/// Reads and changes a system resource. Query errors must not represent absence.
pub trait Backend {
    type Target;
    type Change;
    type Snapshot;
    type Error;

    fn query(&mut self, target: &Self::Target) -> Result<Self::Snapshot, Self::Error>;
    fn apply(&mut self, target: &Self::Target, change: &Self::Change) -> Result<(), Self::Error>;
}

/// Stores the pre-state durably and validates it before returning success.
pub trait SnapshotStore<T, S, C> {
    type Error;

    fn persist(&mut self, target: &T, before: &S, intended: &C) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransactionError<B, S> {
    Query(B),
    Persist(S),
    /// The change may have partially applied; recovery must use the snapshot.
    Apply(B),
}

/// Executes one operation. Failed capture or persistence prevents application.
pub fn apply_one<B, S>(
    backend: &mut B,
    store: &mut S,
    operation: &Operation<B::Target, B::Change>,
) -> Result<(), TransactionError<B::Error, S::Error>>
where
    B: Backend,
    S: SnapshotStore<B::Target, B::Snapshot, B::Change>,
{
    let before = backend
        .query(&operation.target)
        .map_err(TransactionError::Query)?;
    store
        .persist(&operation.target, &before, &operation.change)
        .map_err(TransactionError::Persist)?;
    backend
        .apply(&operation.target, &operation.change)
        .map_err(TransactionError::Apply)
}

/// The guard is evaluated against the same snapshot persisted before mutation.
#[derive(Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    Skipped,
}

pub fn apply_one_if<B, S, F>(
    backend: &mut B,
    store: &mut S,
    operation: &Operation<B::Target, B::Change>,
    guard: F,
) -> Result<ApplyOutcome, TransactionError<B::Error, S::Error>>
where
    B: Backend,
    S: SnapshotStore<B::Target, B::Snapshot, B::Change>,
    F: FnOnce(&B::Snapshot) -> bool,
{
    let before = backend
        .query(&operation.target)
        .map_err(TransactionError::Query)?;
    if !guard(&before) {
        return Ok(ApplyOutcome::Skipped);
    }
    store
        .persist(&operation.target, &before, &operation.change)
        .map_err(TransactionError::Persist)?;
    backend
        .apply(&operation.target, &operation.change)
        .map_err(TransactionError::Apply)?;
    Ok(ApplyOutcome::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockBackend {
        value: Option<Vec<u8>>,
        fail_query: bool,
        fail_apply: bool,
        writes: usize,
    }

    impl Backend for MockBackend {
        type Target = ();
        type Change = Option<Vec<u8>>;
        type Snapshot = Option<Vec<u8>>;
        type Error = &'static str;

        fn query(&mut self, _: &()) -> Result<Self::Snapshot, Self::Error> {
            if self.fail_query {
                Err("query")
            } else {
                Ok(self.value.clone())
            }
        }

        fn apply(&mut self, _: &(), value: &Self::Change) -> Result<(), Self::Error> {
            self.writes += 1;
            self.value.clone_from(value);
            if self.fail_apply {
                Err("apply")
            } else {
                Ok(())
            }
        }
    }

    #[derive(Default)]
    struct MockStore {
        records: Vec<Option<Vec<u8>>>,
        fail: bool,
    }

    impl SnapshotStore<(), Option<Vec<u8>>, Option<Vec<u8>>> for MockStore {
        type Error = &'static str;
        fn persist(
            &mut self,
            _: &(),
            before: &Option<Vec<u8>>,
            _: &Option<Vec<u8>>,
        ) -> Result<(), Self::Error> {
            if self.fail {
                return Err("persist");
            }
            self.records.push(before.clone());
            Ok(())
        }
    }

    fn op() -> Operation<(), Option<Vec<u8>>> {
        Operation {
            target: (),
            change: Some(vec![7]),
        }
    }

    #[test]
    fn query_failure_prevents_persistence_and_mutation() {
        let mut backend = MockBackend {
            fail_query: true,
            ..Default::default()
        };
        let mut store = MockStore::default();
        assert_eq!(
            apply_one(&mut backend, &mut store, &op()),
            Err(TransactionError::Query("query"))
        );
        assert!(store.records.is_empty());
        assert_eq!(backend.writes, 0);
    }

    #[test]
    fn persistence_failure_prevents_mutation() {
        let mut backend = MockBackend::default();
        let mut store = MockStore {
            fail: true,
            ..Default::default()
        };
        assert_eq!(
            apply_one(&mut backend, &mut store, &op()),
            Err(TransactionError::Persist("persist"))
        );
        assert_eq!(backend.writes, 0);
    }

    #[test]
    fn preserves_absent_and_empty_before_values() {
        for before in [None, Some(Vec::new())] {
            let mut backend = MockBackend {
                value: before.clone(),
                ..Default::default()
            };
            let mut store = MockStore::default();
            apply_one(&mut backend, &mut store, &op()).unwrap();
            assert_eq!(store.records, vec![before]);
            assert_eq!(backend.value, Some(vec![7]));
        }
    }

    #[test]
    fn apply_failure_retains_snapshot_for_recovery() {
        let mut backend = MockBackend {
            value: Some(vec![3]),
            fail_apply: true,
            ..Default::default()
        };
        let mut store = MockStore::default();
        assert_eq!(
            apply_one(&mut backend, &mut store, &op()),
            Err(TransactionError::Apply("apply"))
        );
        assert_eq!(store.records, vec![Some(vec![3])]);
        assert_eq!(backend.value, Some(vec![7]));
    }
}
