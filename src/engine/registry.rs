//! Registry value identity and bytes. Windows access is read-only.

use serde::{Deserialize, Serialize};

#[cfg(windows)]
pub mod windows;

/// Registry hive; names refer to the context of the process performing the operation.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
    Users,
    ClassesRoot,
    CurrentConfig,
}

/// The selected registry view is part of the target identity.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum RegistryView {
    Native,
    Registry64,
    Registry32,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ValueTarget {
    pub hive: Hive,
    pub view: RegistryView,
    pub key_path: String,
    /// An empty name identifies the default value.
    pub value_name: String,
}

/// Native registry type identifier and unmodified bytes, including terminators.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryValue {
    pub native_type: u32,
    pub bytes: Vec<u8>,
}

/// Captures whether the key or only the value was absent at query time.
/// KeyMissing alone does not describe missing ancestors or permit key deletion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ValueState {
    KeyMissing,
    ValueMissing,
    Present(RegistryValue),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::recovery::{recover_one, RecoveryBackend, RecoveryOutcome, RecoveryRecord};
    use crate::engine::transaction::{
        apply_one, Backend, Operation, SnapshotStore, TransactionError,
    };
    use std::collections::HashMap;
    use std::convert::Infallible;

    #[derive(Default)]
    struct Registry {
        values: HashMap<ValueTarget, ValueState>,
        writes: usize,
    }
    impl Registry {
        fn read(&self, target: &ValueTarget) -> ValueState {
            self.values
                .get(target)
                .cloned()
                .unwrap_or(ValueState::ValueMissing)
        }
        fn write(&mut self, target: &ValueTarget, state: &ValueState) {
            self.writes += 1;
            self.values.insert(target.clone(), state.clone());
        }
    }
    impl Backend for Registry {
        type Target = ValueTarget;
        type Change = ValueState;
        type Snapshot = ValueState;
        type Error = Infallible;
        fn query(&mut self, target: &ValueTarget) -> Result<ValueState, Infallible> {
            Ok(self.read(target))
        }
        fn apply(&mut self, target: &ValueTarget, change: &ValueState) -> Result<(), Infallible> {
            self.write(target, change);
            Ok(())
        }
    }
    impl RecoveryBackend for Registry {
        type Target = ValueTarget;
        type State = ValueState;
        type Error = Infallible;
        fn query(&mut self, target: &ValueTarget) -> Result<ValueState, Infallible> {
            Ok(self.read(target))
        }
        fn restore(&mut self, target: &ValueTarget, state: &ValueState) -> Result<(), Infallible> {
            self.write(target, state);
            Ok(())
        }
    }
    #[derive(Default)]
    struct Journal {
        records: Vec<RecoveryRecord<ValueTarget, ValueState>>,
        fail: bool,
    }
    impl SnapshotStore<ValueTarget, ValueState, ValueState> for Journal {
        type Error = &'static str;
        fn persist(
            &mut self,
            target: &ValueTarget,
            before: &ValueState,
            intended: &ValueState,
        ) -> Result<(), Self::Error> {
            if self.fail {
                return Err("journal failure");
            }
            self.records.push(RecoveryRecord {
                target: target.clone(),
                before: before.clone(),
                intended: intended.clone(),
            });
            Ok(())
        }
    }
    fn target() -> ValueTarget {
        ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Value".into(),
        }
    }
    #[test]
    fn apply_and_recover_preserve_exact_original_type_and_bytes() {
        let target = target();
        let original = ValueState::Present(RegistryValue {
            native_type: 7,
            bytes: vec![0, 0, 255],
        });
        let changed = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1, 0, 0, 0],
        });
        let mut registry = Registry::default();
        registry.write(&target, &original);
        let mut journal = Journal::default();
        apply_one(
            &mut registry,
            &mut journal,
            &Operation {
                target: target.clone(),
                change: changed.clone(),
            },
        )
        .unwrap();
        assert_eq!(registry.read(&target), changed);
        let record = journal.records[0].clone();
        assert_eq!(
            recover_one(&mut registry, &mut journal, &record),
            Ok(RecoveryOutcome::Restored)
        );
        assert_eq!(registry.read(&target), original);
        assert_eq!(journal.records.len(), 2);
    }
    #[test]
    fn absent_value_is_removed_on_undo_and_views_are_distinct() {
        let target = target();
        let mut other = target.clone();
        other.view = RegistryView::Registry32;
        let value = ValueState::Present(RegistryValue {
            native_type: 3,
            bytes: vec![],
        });
        let mut registry = Registry::default();
        registry.write(&other, &value);
        let mut journal = Journal::default();
        apply_one(
            &mut registry,
            &mut journal,
            &Operation {
                target: target.clone(),
                change: value.clone(),
            },
        )
        .unwrap();
        let record = journal.records[0].clone();
        assert_eq!(
            recover_one(&mut registry, &mut journal, &record),
            Ok(RecoveryOutcome::Restored)
        );
        assert_eq!(registry.read(&target), ValueState::ValueMissing);
        assert_eq!(registry.read(&other), value);
    }
    #[test]
    fn missing_key_state_is_preserved_through_mock_recovery() {
        let target = target();
        let mut registry = Registry::default();
        registry.write(&target, &ValueState::KeyMissing);
        let mut journal = Journal::default();
        let changed = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1, 0, 0, 0],
        });
        apply_one(
            &mut registry,
            &mut journal,
            &Operation {
                target: target.clone(),
                change: changed,
            },
        )
        .unwrap();
        let record = journal.records[0].clone();
        assert_eq!(record.before, ValueState::KeyMissing);
        assert_eq!(
            recover_one(&mut registry, &mut journal, &record),
            Ok(RecoveryOutcome::Restored)
        );
        assert_eq!(registry.read(&target), ValueState::KeyMissing);
    }
    #[test]
    fn failed_journal_prevents_registry_write() {
        let mut registry = Registry::default();
        let mut journal = Journal {
            fail: true,
            ..Default::default()
        };
        let result = apply_one(
            &mut registry,
            &mut journal,
            &Operation {
                target: target(),
                change: ValueState::Present(RegistryValue {
                    native_type: 4,
                    bytes: vec![1, 0, 0, 0],
                }),
            },
        );
        assert_eq!(result, Err(TransactionError::Persist("journal failure")));
        assert_eq!(registry.writes, 0);
    }
}
