//! Read-only desired-state comparison for declarative catalog operations.
//!
//! This is a narrow mockable building block for comparing typed registry and
//! service observations with operation values. It does not execute operations,
//! infer checkbox or applied-tweak parity, validate source provenance, or grant
//! authority to use catalog entries. The current catalog has no toggle/default
//! state semantics; callers must not treat a match as proof of application.

use crate::data::catalog::CatalogOperation;
use crate::data::journal::ServiceTarget;
use crate::engine::registry::{ValueState, ValueTarget};

/// Whether the query observation satisfies an operation's declared desired state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesiredStateStatus {
    Match,
    Mismatch,
    TargetMissing,
    /// The injected query failed. Errors remain data; this layer does no recovery.
    QueryError,
}

/// Exact read-only observation returned by an injected query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservedState {
    Registry(ValueState),
    ServiceStartType(Option<u32>),
}

/// One ordered operation's comparison status and exact query observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesiredStateObservation<E> {
    pub operation_index: usize,
    pub status: DesiredStateStatus,
    pub observed: Result<ObservedState, E>,
}

/// Compare operations to injected observations without performing system I/O.
///
/// Query closures are called only for the corresponding operation, in source
/// order. `SetRegistryValue` compares native type and bytes exactly, and a
/// missing key/value is a mismatch. `DeleteRegistryValue` matches either
/// absence representation while retaining the exact observed state. Both
/// service operation forms compare the queried start type to `start_type`; the
/// conditional form's `expected_start_type` is execution precondition metadata
/// and is not an observation target here.
pub fn compare_catalog_desired_state<E, R, S>(
    operations: &[CatalogOperation],
    mut query_registry: R,
    mut query_service_start_type: S,
) -> Vec<DesiredStateObservation<E>>
where
    R: FnMut(&ValueTarget) -> Result<ValueState, E>,
    S: FnMut(&ServiceTarget) -> Result<Option<u32>, E>,
{
    let mut observations = Vec::with_capacity(operations.len());
    for (operation_index, operation) in operations.iter().enumerate() {
        let (status, observed) = match operation {
            CatalogOperation::SetRegistryValue { target, value } => match query_registry(target) {
                Ok(state) => {
                    let status = match &state {
                        ValueState::Present(actual) if actual == value => DesiredStateStatus::Match,
                        ValueState::Present(_)
                        | ValueState::ValueMissing
                        | ValueState::KeyMissing => DesiredStateStatus::Mismatch,
                    };
                    (status, Ok(ObservedState::Registry(state)))
                }
                Err(error) => (DesiredStateStatus::QueryError, Err(error)),
            },
            CatalogOperation::DeleteRegistryValue { target } => match query_registry(target) {
                Ok(state) => {
                    let status = match state {
                        ValueState::KeyMissing | ValueState::ValueMissing => {
                            DesiredStateStatus::Match
                        }
                        ValueState::Present(_) => DesiredStateStatus::Mismatch,
                    };
                    (status, Ok(ObservedState::Registry(state)))
                }
                Err(error) => (DesiredStateStatus::QueryError, Err(error)),
            },
            CatalogOperation::SetServiceStartType {
                target, start_type, ..
            }
            | CatalogOperation::SetServiceStartTypeIfExpected {
                target, start_type, ..
            } => match query_service_start_type(target) {
                Ok(state) => {
                    let status = match state {
                        Some(actual) if actual == *start_type => DesiredStateStatus::Match,
                        Some(_) => DesiredStateStatus::Mismatch,
                        None => DesiredStateStatus::TargetMissing,
                    };
                    (status, Ok(ObservedState::ServiceStartType(state)))
                }
                Err(error) => (DesiredStateStatus::QueryError, Err(error)),
            },
        };
        observations.push(DesiredStateObservation {
            operation_index,
            status,
            observed,
        });
    }
    observations
}

/// Identifies which read-only system query failed while retaining its original error.
#[cfg(windows)]
#[derive(Debug)]
pub enum WindowsQueryError {
    Registry(crate::engine::registry::windows::ReadError),
    Service(crate::engine::services::QueryError),
}

/// Observe the desired state of caller-supplied operations using live Windows queries.
///
/// This performs queries only. It does not load or validate a catalog, execute
/// operations, cache status, or establish whether a tweak was previously applied.
#[cfg(windows)]
pub fn observe_catalog_desired_state(
    operations: &[CatalogOperation],
) -> Vec<DesiredStateObservation<WindowsQueryError>> {
    compare_catalog_desired_state(
        operations,
        |target| {
            crate::engine::registry::windows::read(target).map_err(WindowsQueryError::Registry)
        },
        |target| {
            use crate::engine::services::ConfigResult;
            crate::engine::services::query_config(&target.name)
                .map(|result| match result {
                    ConfigResult::Missing => None,
                    ConfigResult::Present { start_type, .. } => Some(start_type),
                })
                .map_err(WindowsQueryError::Service)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::registry::{Hive, RegistryValue, RegistryView};
    use std::cell::Cell;

    fn registry_target(name: &str) -> ValueTarget {
        ValueTarget {
            hive: Hive::CurrentUser,
            view: RegistryView::Native,
            key_path: "Software\\Example".into(),
            value_name: name.into(),
        }
    }

    fn service_target() -> ServiceTarget {
        ServiceTarget {
            name: "Demo".into(),
        }
    }

    fn desired_value() -> RegistryValue {
        RegistryValue {
            native_type: 4,
            bytes: vec![1, 0, 0, 0],
        }
    }

    fn set_value() -> CatalogOperation {
        CatalogOperation::SetRegistryValue {
            target: registry_target("Enabled"),
            value: desired_value(),
        }
    }

    #[test]
    fn registry_set_requires_exact_type_and_bytes_and_missing_state_is_mismatch() {
        for (observed, expected) in [
            (
                ValueState::Present(desired_value()),
                DesiredStateStatus::Match,
            ),
            (
                ValueState::Present(RegistryValue {
                    native_type: 3,
                    bytes: vec![1, 0, 0, 0],
                }),
                DesiredStateStatus::Mismatch,
            ),
            (
                ValueState::Present(RegistryValue {
                    native_type: 4,
                    bytes: vec![1, 0, 0, 1],
                }),
                DesiredStateStatus::Mismatch,
            ),
            (ValueState::KeyMissing, DesiredStateStatus::Mismatch),
            (ValueState::ValueMissing, DesiredStateStatus::Mismatch),
        ] {
            let result = compare_catalog_desired_state(
                &[set_value()],
                |_| Ok::<_, ()>(observed.clone()),
                |_| Ok::<_, ()>(None),
            );
            assert_eq!(result[0].status, expected);
            assert_eq!(result[0].observed, Ok(ObservedState::Registry(observed)));
        }
    }

    #[test]
    fn registry_delete_matches_both_absences_and_preserves_observation() {
        let operation = CatalogOperation::DeleteRegistryValue {
            target: registry_target("Gone"),
        };
        for (observed, expected) in [
            (ValueState::KeyMissing, DesiredStateStatus::Match),
            (ValueState::ValueMissing, DesiredStateStatus::Match),
            (
                ValueState::Present(desired_value()),
                DesiredStateStatus::Mismatch,
            ),
        ] {
            let result = compare_catalog_desired_state(
                std::slice::from_ref(&operation),
                |_| Ok::<_, ()>(observed.clone()),
                |_| Ok::<_, ()>(None),
            );
            assert_eq!(result[0].status, expected);
            assert_eq!(result[0].observed, Ok(ObservedState::Registry(observed)));
        }
    }

    #[test]
    fn service_comparison_handles_exact_different_missing_and_query_error() {
        let operation = CatalogOperation::SetServiceStartType {
            target: service_target(),
            start_type: 4,
        };
        for (query, expected) in [
            (Ok(Some(4)), DesiredStateStatus::Match),
            (Ok(Some(3)), DesiredStateStatus::Mismatch),
            (Ok(None), DesiredStateStatus::TargetMissing),
            (Err("unavailable"), DesiredStateStatus::QueryError),
        ] {
            let result = compare_catalog_desired_state(
                std::slice::from_ref(&operation),
                |_| Ok::<_, &str>(ValueState::ValueMissing),
                |_| query,
            );
            assert_eq!(result[0].status, expected);
            match query {
                Ok(state) => assert_eq!(
                    result[0].observed,
                    Ok(ObservedState::ServiceStartType(state))
                ),
                Err(error) => assert_eq!(result[0].observed, Err(error)),
            }
        }
    }

    #[test]
    fn conditional_service_compares_desired_value_not_expected_value() {
        let operation = CatalogOperation::SetServiceStartTypeIfExpected {
            target: service_target(),
            start_type: 4,
            expected_start_type: 3,
        };
        let result = compare_catalog_desired_state(
            &[operation],
            |_| Ok::<_, ()>(ValueState::ValueMissing),
            |_| Ok::<_, ()>(Some(3)),
        );
        assert_eq!(result[0].status, DesiredStateStatus::Mismatch);
        assert_eq!(
            result[0].observed,
            Ok(ObservedState::ServiceStartType(Some(3)))
        );
    }

    #[test]
    fn empty_input_returns_empty_without_querying() {
        let calls = Cell::new(0);
        let result = compare_catalog_desired_state::<(), _, _>(
            &[],
            |_| {
                calls.set(calls.get() + 1);
                Ok(ValueState::ValueMissing)
            },
            |_| {
                calls.set(calls.get() + 1);
                Ok(None)
            },
        );
        assert!(result.is_empty());
        assert_eq!(calls.get(), 0);
    }

    #[cfg(windows)]
    #[test]
    fn windows_observer_reads_product_name_and_event_log_without_mutation() {
        use crate::engine::registry::{Hive, RegistryView};

        let operations = [
            CatalogOperation::DeleteRegistryValue {
                target: ValueTarget {
                    hive: Hive::LocalMachine,
                    view: RegistryView::Native,
                    key_path: "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion".into(),
                    value_name: "ProductName".into(),
                },
            },
            CatalogOperation::SetServiceStartType {
                target: ServiceTarget {
                    name: "EventLog".into(),
                },
                start_type: 0,
            },
        ];

        let observations = observe_catalog_desired_state(&operations);
        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].operation_index, 0);
        assert!(matches!(
            &observations[0].observed,
            Ok(ObservedState::Registry(_))
        ));
        assert!(!matches!(
            &observations[0].status,
            DesiredStateStatus::QueryError
        ));
        assert_eq!(observations[1].operation_index, 1);
        assert!(matches!(
            &observations[1].observed,
            Ok(ObservedState::ServiceStartType(_))
        ));
        assert!(!matches!(
            &observations[1].status,
            DesiredStateStatus::QueryError
        ));
    }
}
