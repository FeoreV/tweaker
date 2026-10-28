//! Read-only status observer for the source-audited UTC registry declaration.
//!
//! This reports the configured QWORD value only. It does not write, cache, or
//! claim that the declaration changes RTC behavior or is supported by an OS.

use crate::engine::{
    registry::{Hive, RegistryView, ValueState, ValueTarget},
    toggle_status::{RegistryReadFailure, ToggleStatus},
};
use serde::Serialize;

pub const SUPPORTED_TWEAK_ID: &str = "WPFTweaksUTC";
const KEY_PATH: &str = "SYSTEM\\CurrentControlSet\\Control\\TimeZoneInformation";
const VALUE_NAME: &str = "RealTimeIsUniversal";
const REG_QWORD: u32 = 11;
const DESIRED_VALUE: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UtcReadState {
    KeyMissing,
    ValueMissing,
    Present,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UtcRecordOutcome {
    MatchesDesired,
    DefaultedToOriginalOff,
    DifferentQword,
    UnsupportedRegistryType { native_type: u32 },
    InvalidQwordLength { observed_length: usize },
    QueryFailed { error: RegistryReadFailure },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UtcStatusReport {
    pub report_kind: &'static str,
    pub basis: &'static str,
    pub executable: bool,
    pub tweak_id: String,
    pub registry_view: RegistryView,
    pub target: ValueTarget,
    pub read_state: UtcReadState,
    pub outcome: UtcRecordOutcome,
    pub status: ToggleStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedTweakId;

impl std::fmt::Display for UnsupportedTweakId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported source-audited tweak ID")
    }
}

impl std::error::Error for UnsupportedTweakId {}

/// Observe the exact allowlisted declaration through an injected read-only API.
pub fn observe_utc(
    tweak_id: &str,
    view: RegistryView,
    mut read: impl FnMut(&ValueTarget) -> Result<ValueState, RegistryReadFailure>,
) -> Result<UtcStatusReport, UnsupportedTweakId> {
    if tweak_id != SUPPORTED_TWEAK_ID {
        return Err(UnsupportedTweakId);
    }

    let target = ValueTarget {
        hive: Hive::LocalMachine,
        view: view.clone(),
        key_path: KEY_PATH.into(),
        value_name: VALUE_NAME.into(),
    };
    let (read_state, outcome) = match read(&target) {
        Ok(ValueState::KeyMissing) => (
            UtcReadState::KeyMissing,
            UtcRecordOutcome::DefaultedToOriginalOff,
        ),
        Ok(ValueState::ValueMissing) => (
            UtcReadState::ValueMissing,
            UtcRecordOutcome::DefaultedToOriginalOff,
        ),
        Ok(ValueState::Present(value)) if value.native_type != REG_QWORD => (
            UtcReadState::Present,
            UtcRecordOutcome::UnsupportedRegistryType {
                native_type: value.native_type,
            },
        ),
        Ok(ValueState::Present(value)) if value.bytes.len() != 8 => (
            UtcReadState::Present,
            UtcRecordOutcome::InvalidQwordLength {
                observed_length: value.bytes.len(),
            },
        ),
        Ok(ValueState::Present(value)) => {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&value.bytes);
            if u64::from_le_bytes(bytes) == DESIRED_VALUE {
                (UtcReadState::Present, UtcRecordOutcome::MatchesDesired)
            } else {
                (UtcReadState::Present, UtcRecordOutcome::DifferentQword)
            }
        }
        Err(error) => (
            UtcReadState::Failed,
            UtcRecordOutcome::QueryFailed { error },
        ),
    };

    let status = match &outcome {
        UtcRecordOutcome::MatchesDesired => ToggleStatus::Checked,
        UtcRecordOutcome::DefaultedToOriginalOff | UtcRecordOutcome::DifferentQword => {
            ToggleStatus::Unchecked
        }
        UtcRecordOutcome::UnsupportedRegistryType { .. }
        | UtcRecordOutcome::InvalidQwordLength { .. }
        | UtcRecordOutcome::QueryFailed { .. } => ToggleStatus::Unknown,
    };

    Ok(UtcStatusReport {
        report_kind: "source_audited_tweak_status",
        basis: "source_subset_only",
        executable: false,
        tweak_id: SUPPORTED_TWEAK_ID.into(),
        registry_view: view,
        target,
        read_state,
        outcome,
        status,
    })
}

/// Observe the allowlisted declaration through the read-only Windows registry API.
#[cfg(windows)]
pub fn observe_windows(
    tweak_id: &str,
    view: RegistryView,
) -> Result<UtcStatusReport, UnsupportedTweakId> {
    observe_utc(tweak_id, view, |target| {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::registry::RegistryValue;

    fn qword(value: u64) -> ValueState {
        ValueState::Present(RegistryValue {
            native_type: REG_QWORD,
            bytes: value.to_le_bytes().to_vec(),
        })
    }

    #[test]
    fn checked_report_uses_exact_target_and_selected_view() {
        let report = observe_utc("WPFTweaksUTC", RegistryView::Registry32, |target| {
            assert_eq!(target.hive, Hive::LocalMachine);
            assert_eq!(target.view, RegistryView::Registry32);
            assert_eq!(target.key_path, KEY_PATH);
            assert_eq!(target.value_name, VALUE_NAME);
            Ok(qword(1))
        })
        .unwrap();
        assert_eq!(report.status, ToggleStatus::Checked);
        assert_eq!(report.read_state, UtcReadState::Present);
        assert_eq!(report.outcome, UtcRecordOutcome::MatchesDesired);
        assert!(!report.executable);
        assert_eq!(report.basis, "source_subset_only");
    }

    #[test]
    fn missing_key_or_value_and_other_qwords_are_unchecked() {
        for state in [
            ValueState::KeyMissing,
            ValueState::ValueMissing,
            qword(0),
            qword(2),
            qword(u64::MAX),
        ] {
            let report =
                observe_utc("WPFTweaksUTC", RegistryView::Native, |_| Ok(state.clone())).unwrap();
            assert_eq!(report.status, ToggleStatus::Unchecked);
        }
    }

    #[test]
    fn wrong_native_type_and_non_qword_lengths_are_unknown() {
        for state in [
            ValueState::Present(RegistryValue {
                native_type: 4,
                bytes: 1u64.to_le_bytes().to_vec(),
            }),
            ValueState::Present(RegistryValue {
                native_type: REG_QWORD,
                bytes: vec![1; 7],
            }),
            ValueState::Present(RegistryValue {
                native_type: REG_QWORD,
                bytes: vec![1; 9],
            }),
        ] {
            let report =
                observe_utc("WPFTweaksUTC", RegistryView::Native, |_| Ok(state.clone())).unwrap();
            assert_eq!(report.status, ToggleStatus::Unknown);
        }
    }

    #[test]
    fn read_errors_remain_unknown_and_keep_the_failure_kind() {
        for error in [
            RegistryReadFailure::Win32 { code: 5 },
            RegistryReadFailure::InvalidName,
            RegistryReadFailure::RootKeyUnsupported,
            RegistryReadFailure::InvalidHandle,
            RegistryReadFailure::ValueTooLarge,
            RegistryReadFailure::ChangedTooOften,
        ] {
            let report =
                observe_utc("WPFTweaksUTC", RegistryView::Registry64, |_| Err(error)).unwrap();
            assert_eq!(report.status, ToggleStatus::Unknown);
            assert_eq!(report.read_state, UtcReadState::Failed);
            assert_eq!(report.outcome, UtcRecordOutcome::QueryFailed { error });
        }
    }

    #[test]
    fn only_exact_identifier_is_accepted() {
        assert!(observe_utc("WPFTweaksUTC", RegistryView::Native, |_| Ok(qword(1))).is_ok());
        assert!(observe_utc("wpftweaksutc", RegistryView::Native, |_| unreachable!()).is_err());
        assert!(observe_utc("WPFToggleNumLock", RegistryView::Native, |_| unreachable!()).is_err());
    }

    #[test]
    fn serialization_omits_raw_registry_bytes() {
        let report = observe_utc("WPFTweaksUTC", RegistryView::Native, |_| Ok(qword(1))).unwrap();
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["tweak_id"], SUPPORTED_TWEAK_ID);
        assert_eq!(json["executable"], false);
        assert_eq!(json["target"]["hive"], "LocalMachine");
        assert_eq!(json["target"]["key_path"], KEY_PATH);
        assert_eq!(json["outcome"], "matches_desired");
        assert!(json.get("bytes").is_none());
        assert!(!json.to_string().contains("[1,0,0,0,0,0,0,0]"));
    }
}
