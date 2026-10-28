//! Source-specific, mockable operation description for window snapping.
//!
//! This module only builds and exercises a typed registry operation. It has no
//! Windows writer and does not authorize callers to apply the operation.

use crate::data::store::{TypedState, TypedTarget};
use crate::engine::registry::{Hive, RegistryValue, RegistryView, ValueState, ValueTarget};
use crate::engine::transaction::Operation;

const KEY_PATH: &str = "Control Panel\\Desktop";
const VALUE_NAME: &str = "WindowArrangementActive";
const REG_SZ: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowSnappingIntent {
    Enable,
    Disable,
}

/// Construct the exact registry operation declared by the PowerShell source.
///
/// The source's Undo value (`"0"`) is a configured state. Transaction recovery
/// instead restores the exact state captured before apply; these are not
/// equivalent when the prior value differs or was absent.
pub(crate) fn source_operation(intent: WindowSnappingIntent) -> Operation<TypedTarget, TypedState> {
    let text = match intent {
        WindowSnappingIntent::Enable => "1",
        WindowSnappingIntent::Disable => "0",
    };
    let mut bytes = text
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    bytes.extend_from_slice(&[0, 0]);
    Operation {
        target: TypedTarget::Registry(ValueTarget {
            hive: Hive::CurrentUser,
            // The source does not select a registry view. The Windows write
            // backend maps Native to the 64-bit view from a 32-bit process.
            view: RegistryView::Native,
            key_path: KEY_PATH.into(),
            value_name: VALUE_NAME.into(),
        }),
        change: TypedState::Registry(ValueState::Present(RegistryValue {
            native_type: REG_SZ,
            bytes,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::TypedState;
    use crate::engine::recovery::{recover_typed_one, RecoveryRecord, TypedRecoveryBackend};
    use crate::engine::transaction::{apply_one, Backend, SnapshotStore, TransactionError};
    use crate::engine::tweak_plan::{ExistingKeyBackend, QueryError};

    #[derive(Default)]
    struct MockBackend {
        state: Option<TypedState>,
        fail_query: bool,
        fail_apply: bool,
        writes: usize,
    }

    impl Backend for MockBackend {
        type Target = TypedTarget;
        type Change = TypedState;
        type Snapshot = TypedState;
        type Error = &'static str;

        fn query(&mut self, _: &TypedTarget) -> Result<TypedState, Self::Error> {
            if self.fail_query {
                return Err("query");
            }
            self.state.clone().ok_or("unavailable")
        }

        fn apply(&mut self, _: &TypedTarget, change: &TypedState) -> Result<(), Self::Error> {
            self.writes += 1;
            self.state = Some(change.clone());
            if self.fail_apply {
                Err("apply")
            } else {
                Ok(())
            }
        }
    }

    impl TypedRecoveryBackend for MockBackend {
        type Error = &'static str;

        fn query_typed(&mut self, target: &TypedTarget) -> Result<TypedState, Self::Error> {
            Backend::query(self, target)
        }

        fn restore_typed(
            &mut self,
            target: &TypedTarget,
            state: &TypedState,
        ) -> Result<(), Self::Error> {
            self.fail_apply = false;
            Backend::apply(self, target, state)
        }
    }

    #[derive(Default)]
    struct MockSnapshots {
        records: Vec<RecoveryRecord<TypedTarget, TypedState>>,
        fail: bool,
    }

    impl SnapshotStore<TypedTarget, TypedState, TypedState> for MockSnapshots {
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

    fn prior_present() -> TypedState {
        TypedState::Registry(ValueState::Present(RegistryValue {
            native_type: 2,
            bytes: vec![0x37, 0x00, 0x00, 0x00],
        }))
    }

    #[test]
    fn operations_use_exact_target_and_terminated_utf16le_reg_sz_values() {
        let enable = source_operation(WindowSnappingIntent::Enable);
        let disable = source_operation(WindowSnappingIntent::Disable);
        let expected_target = TypedTarget::Registry(ValueTarget {
            hive: Hive::CurrentUser,
            view: RegistryView::Native,
            key_path: KEY_PATH.into(),
            value_name: VALUE_NAME.into(),
        });
        assert_eq!(enable.target, expected_target);
        assert_eq!(disable.target, expected_target);
        assert_eq!(
            enable.change,
            TypedState::Registry(ValueState::Present(RegistryValue {
                native_type: REG_SZ,
                bytes: vec![b'1', 0, 0, 0],
            }))
        );
        assert_eq!(
            disable.change,
            TypedState::Registry(ValueState::Present(RegistryValue {
                native_type: REG_SZ,
                bytes: vec![b'0', 0, 0, 0],
            }))
        );
    }

    #[test]
    fn missing_key_fails_closed_before_snapshot_or_apply() {
        let operation = source_operation(WindowSnappingIntent::Enable);
        let mut backend = MockBackend {
            state: Some(TypedState::Registry(ValueState::KeyMissing)),
            ..MockBackend::default()
        };
        let mut snapshots = MockSnapshots::default();
        let mut existing = ExistingKeyBackend(&mut backend);
        assert_eq!(
            apply_one(&mut existing, &mut snapshots, &operation),
            Err(TransactionError::Query(QueryError::MissingKey))
        );
        assert!(snapshots.records.is_empty());
        assert_eq!(backend.writes, 0);
    }

    #[test]
    fn query_and_persist_failures_prevent_apply() {
        let operation = source_operation(WindowSnappingIntent::Enable);
        let mut backend = MockBackend {
            state: Some(prior_present()),
            fail_query: true,
            ..MockBackend::default()
        };
        let mut snapshots = MockSnapshots::default();
        {
            let mut existing = ExistingKeyBackend(&mut backend);
            assert_eq!(
                apply_one(&mut existing, &mut snapshots, &operation),
                Err(TransactionError::Query(QueryError::Backend("query")))
            );
        }
        assert_eq!(backend.writes, 0);

        backend.fail_query = false;
        snapshots.fail = true;
        {
            let mut existing = ExistingKeyBackend(&mut backend);
            assert_eq!(
                apply_one(&mut existing, &mut snapshots, &operation),
                Err(TransactionError::Persist("persist"))
            );
        }
        assert_eq!(backend.writes, 0);
    }

    #[test]
    fn recovery_restores_present_non_default_and_missing_value_exactly() {
        for prior in [
            prior_present(),
            TypedState::Registry(ValueState::ValueMissing),
        ] {
            let operation = source_operation(WindowSnappingIntent::Enable);
            let intended = operation.change.clone();
            let mut backend = MockBackend {
                state: Some(prior.clone()),
                ..MockBackend::default()
            };
            let mut snapshots = MockSnapshots::default();
            {
                let mut existing = ExistingKeyBackend(&mut backend);
                apply_one(&mut existing, &mut snapshots, &operation).unwrap();
            }
            let record = snapshots.records[0].clone();
            assert_eq!(record.before, prior);
            assert_eq!(record.intended, intended);
            recover_typed_one(&mut backend, &mut snapshots, &record).unwrap();
            assert_eq!(backend.state, Some(prior));
        }
    }

    #[test]
    fn partial_apply_failure_keeps_record_and_recovery_restores_exact_prior() {
        let operation = source_operation(WindowSnappingIntent::Enable);
        let prior = prior_present();
        let mut backend = MockBackend {
            state: Some(prior.clone()),
            fail_apply: true,
            ..MockBackend::default()
        };
        let mut snapshots = MockSnapshots::default();
        {
            let mut existing = ExistingKeyBackend(&mut backend);
            assert_eq!(
                apply_one(&mut existing, &mut snapshots, &operation),
                Err(TransactionError::Apply(QueryError::Backend("apply")))
            );
        }
        assert_eq!(snapshots.records.len(), 1);
        let record = snapshots.records[0].clone();
        recover_typed_one(&mut backend, &mut snapshots, &record).unwrap();
        assert_eq!(backend.state, Some(prior));
    }
}
