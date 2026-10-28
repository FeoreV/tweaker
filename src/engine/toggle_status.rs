//! Read-only status observer for the single source-audited BSoD toggle.
//!
//! The PowerShell source defines two HKLM `REG_DWORD` values with desired
//! value 1 and `DefaultState=false`/`OriginalValue=0`. Missing values therefore
//! report unchecked. This module does not write values, use the static original
//! values as rollback state, share PowerShell's cache, or claim Windows support.

use crate::engine::registry::{Hive, RegistryView, ValueState, ValueTarget};
use serde::Serialize;

pub const SUPPORTED_TOGGLE_ID: &str = "WPFToggleDetailedBSoD";
const KEY_PATH: &str = "SYSTEM\\CurrentControlSet\\Control\\CrashControl";
const VALUE_NAMES: [&str; 2] = ["DisplayParameters", "DisableEmoticon"];
const REG_DWORD: u32 = 4;
const DESIRED_VALUE: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToggleStatus {
    Checked,
    Unchecked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToggleRecordOutcome {
    MatchesDesired,
    DefaultedToOriginalOff,
    DifferentDword { observed: u32 },
    UnsupportedRegistryType { native_type: u32 },
    InvalidDwordLength { bytes: usize },
    QueryFailed { error: RegistryReadFailure },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryReadFailure {
    Win32 { code: u32 },
    InvalidName,
    RootKeyUnsupported,
    InvalidHandle,
    ValueTooLarge,
    ChangedTooOften,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToggleRecordObservation {
    pub value_name: String,
    pub observed: Option<ValueState>,
    pub outcome: ToggleRecordOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToggleStatusReport {
    pub report_kind: &'static str,
    pub basis: &'static str,
    pub executable: bool,
    pub toggle_id: String,
    pub registry_view: RegistryView,
    pub status: ToggleStatus,
    pub observations: Vec<ToggleRecordObservation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedToggleId;

impl std::fmt::Display for UnsupportedToggleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported source-audited toggle ID")
    }
}

impl std::error::Error for UnsupportedToggleId {}

/// Observe a supported toggle using an injected read function.
///
/// Only DWORD values are interpreted. Wrong types and query errors yield
/// `unknown`, rather than being coerced like PowerShell's `-ne` operator.
/// Missing keys/values reproduce this entry's configured false default.
pub fn observe_detailed_bsod(
    view: RegistryView,
    mut read: impl FnMut(&ValueTarget) -> Result<ValueState, RegistryReadFailure>,
) -> ToggleStatusReport {
    let mut observations = Vec::with_capacity(VALUE_NAMES.len());
    let mut saw_unknown = false;
    let mut status = ToggleStatus::Checked;

    for value_name in VALUE_NAMES {
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: view.clone(),
            key_path: KEY_PATH.to_owned(),
            value_name: value_name.to_owned(),
        };
        let (observed, outcome) = match read(&target) {
            Ok(state @ (ValueState::KeyMissing | ValueState::ValueMissing)) => {
                (Some(state), ToggleRecordOutcome::DefaultedToOriginalOff)
            }
            Ok(state @ ValueState::Present(_)) => {
                let outcome = match &state {
                    ValueState::Present(value) if value.native_type != REG_DWORD => {
                        ToggleRecordOutcome::UnsupportedRegistryType {
                            native_type: value.native_type,
                        }
                    }
                    ValueState::Present(value) if value.bytes.len() != 4 => {
                        ToggleRecordOutcome::InvalidDwordLength {
                            bytes: value.bytes.len(),
                        }
                    }
                    ValueState::Present(value) => {
                        let observed = u32::from_le_bytes(value.bytes[..4].try_into().unwrap());
                        if observed == DESIRED_VALUE {
                            ToggleRecordOutcome::MatchesDesired
                        } else {
                            ToggleRecordOutcome::DifferentDword { observed }
                        }
                    }
                    _ => unreachable!(),
                };
                (Some(state), outcome)
            }
            Err(error) => (None, ToggleRecordOutcome::QueryFailed { error }),
        };

        match outcome {
            ToggleRecordOutcome::MatchesDesired => {}
            ToggleRecordOutcome::DefaultedToOriginalOff
            | ToggleRecordOutcome::DifferentDword { .. } => status = ToggleStatus::Unchecked,
            ToggleRecordOutcome::UnsupportedRegistryType { .. }
            | ToggleRecordOutcome::InvalidDwordLength { .. }
            | ToggleRecordOutcome::QueryFailed { .. } => saw_unknown = true,
        }
        observations.push(ToggleRecordObservation {
            value_name: value_name.to_owned(),
            observed,
            outcome,
        });
        if status == ToggleStatus::Unchecked {
            break;
        }
    }
    if status != ToggleStatus::Unchecked && saw_unknown {
        status = ToggleStatus::Unknown;
    }

    ToggleStatusReport {
        report_kind: "source_audited_toggle_status",
        basis: "source_subset_only",
        executable: false,
        toggle_id: SUPPORTED_TOGGLE_ID.to_owned(),
        registry_view: view,
        status,
        observations,
    }
}

/// Observe the supported source entry through the Windows read-only registry API.
#[cfg(windows)]
pub fn observe_windows(
    toggle_id: &str,
    view: RegistryView,
) -> Result<ToggleStatusReport, UnsupportedToggleId> {
    if !toggle_id.eq_ignore_ascii_case(SUPPORTED_TOGGLE_ID) {
        return Err(UnsupportedToggleId);
    }

    Ok(observe_detailed_bsod(view, |target| {
        crate::engine::registry::windows::read(target).map_err(|error| match error {
            crate::engine::registry::windows::ReadError::Win32(code) => {
                RegistryReadFailure::Win32 { code }
            }
            crate::engine::registry::windows::ReadError::InvalidName => {
                RegistryReadFailure::InvalidName
            }
            crate::engine::registry::windows::ReadError::RootKeyUnsupported => {
                RegistryReadFailure::RootKeyUnsupported
            }
            crate::engine::registry::windows::ReadError::InvalidHandle => {
                RegistryReadFailure::InvalidHandle
            }
            crate::engine::registry::windows::ReadError::ValueTooLarge => {
                RegistryReadFailure::ValueTooLarge
            }
            crate::engine::registry::windows::ReadError::ChangedTooOften => {
                RegistryReadFailure::ChangedTooOften
            }
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::registry::RegistryValue;
    use std::cell::RefCell;

    fn dword(value: u32) -> ValueState {
        ValueState::Present(RegistryValue {
            native_type: REG_DWORD,
            bytes: value.to_le_bytes().to_vec(),
        })
    }

    fn observe(
        states: Vec<Result<ValueState, RegistryReadFailure>>,
    ) -> (ToggleStatusReport, Vec<ValueTarget>) {
        let states = RefCell::new(states.into_iter());
        let targets = RefCell::new(Vec::new());
        let report = observe_detailed_bsod(RegistryView::Registry64, |target| {
            targets.borrow_mut().push(target.clone());
            states.borrow_mut().next().expect("one state per query")
        });
        (report, targets.into_inner())
    }

    #[test]
    fn reports_checked_only_when_both_upstream_values_match() {
        let (report, targets) = observe(vec![Ok(dword(1)), Ok(dword(1))]);
        assert_eq!(report.status, ToggleStatus::Checked);
        assert_eq!(report.observations.len(), 2);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].hive, Hive::LocalMachine);
        assert_eq!(targets[0].view, RegistryView::Registry64);
        assert_eq!(targets[0].key_path, KEY_PATH);
        assert_eq!(targets[0].value_name, VALUE_NAMES[0]);
        assert_eq!(targets[1].value_name, VALUE_NAMES[1]);
        assert!(!report.executable);
    }

    #[test]
    fn missing_values_and_different_dwords_mean_unchecked_and_short_circuit() {
        for state in [
            ValueState::KeyMissing,
            ValueState::ValueMissing,
            dword(0),
            dword(2),
        ] {
            let (report, targets) = observe(vec![Ok(state)]);
            assert_eq!(report.status, ToggleStatus::Unchecked);
            assert_eq!(report.observations.len(), 1);
            assert_eq!(targets.len(), 1);
        }
    }

    #[test]
    fn later_missing_record_means_unchecked_after_first_record_matches() {
        let (report, targets) = observe(vec![Ok(dword(1)), Ok(ValueState::ValueMissing)]);
        assert_eq!(report.status, ToggleStatus::Unchecked);
        assert_eq!(report.observations.len(), 2);
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn query_errors_and_uninterpretable_values_mean_unknown() {
        let unsupported = ValueState::Present(RegistryValue {
            native_type: 1,
            bytes: vec![b'1', 0, 0, 0],
        });
        let malformed = ValueState::Present(RegistryValue {
            native_type: REG_DWORD,
            bytes: vec![1, 0],
        });
        for (state, expected_outcome) in [
            (
                Err(RegistryReadFailure::Win32 { code: 5 }),
                ToggleRecordOutcome::QueryFailed {
                    error: RegistryReadFailure::Win32 { code: 5 },
                },
            ),
            (
                Ok(unsupported),
                ToggleRecordOutcome::UnsupportedRegistryType { native_type: 1 },
            ),
            (
                Ok(malformed),
                ToggleRecordOutcome::InvalidDwordLength { bytes: 2 },
            ),
        ] {
            let (report, targets) = observe(vec![state, Ok(dword(1))]);
            assert_eq!(report.status, ToggleStatus::Unknown);
            assert_eq!(report.observations[0].outcome, expected_outcome);
            assert_eq!(targets.len(), 2);
        }
    }

    #[test]
    fn definite_mismatch_overrides_an_unknown_observation() {
        let (report, targets) =
            observe(vec![Err(RegistryReadFailure::ValueTooLarge), Ok(dword(0))]);
        assert_eq!(report.status, ToggleStatus::Unchecked);
        assert_eq!(report.observations.len(), 2);
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn query_error_serialization_preserves_error_kind_and_api_code() {
        let outcome = ToggleRecordOutcome::QueryFailed {
            error: RegistryReadFailure::Win32 { code: 5 },
        };
        assert_eq!(
            serde_json::to_value(outcome).unwrap(),
            serde_json::json!({"query_failed": {"error": {"win32": {"code": 5}}}})
        );
        let outcome = ToggleRecordOutcome::QueryFailed {
            error: RegistryReadFailure::ValueTooLarge,
        };
        assert_eq!(
            serde_json::to_value(outcome).unwrap(),
            serde_json::json!({"query_failed": {"error": "value_too_large"}})
        );
    }
}
