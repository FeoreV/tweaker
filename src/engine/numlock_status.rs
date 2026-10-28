//! Read-only status observer for the source-audited Num Lock startup toggle.
//!
//! This reports only the two configured registry values and their configured
//! default. It does not write values, cache observations, or claim an effect or
//! platform compatibility.

use crate::engine::{
    registry::{Hive, RegistryView, ValueState, ValueTarget},
    toggle_status::{RegistryReadFailure, ToggleStatus},
};
use serde::Serialize;

pub const SUPPORTED_TOGGLE_ID: &str = "WPFToggleNumLock";
const VALUE_NAME: &str = "InitialKeyboardIndicators";
const DESIRED_VALUE: &str = "2";
const REG_SZ: u32 = 1;
const MAX_STRING_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NumLockRecordOutcome {
    MatchesDesired,
    DefaultedToOriginalOff,
    DifferentString,
    UnsupportedRegistryType { native_type: u32 },
    InvalidStringLength { bytes: usize },
    InvalidUtf16,
    EmbeddedNul,
    QueryFailed { error: RegistryReadFailure },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NumLockReadState {
    KeyMissing,
    ValueMissing,
    Present,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NumLockObservation {
    /// Carries hive and key path because both records use the same value name.
    pub target: ValueTarget,
    /// Reports presence without serializing registry bytes into CLI output.
    pub read_state: NumLockReadState,
    pub outcome: NumLockRecordOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NumLockStatusReport {
    pub report_kind: &'static str,
    pub basis: &'static str,
    pub executable: bool,
    pub toggle_id: String,
    pub registry_view: RegistryView,
    pub status: ToggleStatus,
    pub observations: Vec<NumLockObservation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedToggleId;

impl std::fmt::Display for UnsupportedToggleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported source-audited toggle ID")
    }
}

impl std::error::Error for UnsupportedToggleId {}

/// Observe this exact source entry through a caller-supplied read-only function.
///
/// Unknown data does not become checked or unchecked by coercion. A definite
/// mismatch in either record establishes `Unchecked`, even if the other read
/// failed or returned malformed data.
pub fn observe_numlock(
    toggle_id: &str,
    view: RegistryView,
    mut read: impl FnMut(&ValueTarget) -> Result<ValueState, RegistryReadFailure>,
) -> Result<NumLockStatusReport, UnsupportedToggleId> {
    if !toggle_id.eq_ignore_ascii_case(SUPPORTED_TOGGLE_ID) {
        return Err(UnsupportedToggleId);
    }

    let targets = [
        ValueTarget {
            hive: Hive::Users,
            view: view.clone(),
            key_path: ".Default\\Control Panel\\Keyboard".into(),
            value_name: VALUE_NAME.into(),
        },
        ValueTarget {
            hive: Hive::CurrentUser,
            view: view.clone(),
            key_path: "Control Panel\\Keyboard".into(),
            value_name: VALUE_NAME.into(),
        },
    ];
    let mut observations = Vec::with_capacity(targets.len());
    let mut saw_unknown = false;
    let mut saw_unchecked = false;

    for target in targets {
        let (read_state, outcome) = match read(&target) {
            Ok(state @ (ValueState::KeyMissing | ValueState::ValueMissing)) => {
                let read_state = match state {
                    ValueState::KeyMissing => NumLockReadState::KeyMissing,
                    ValueState::ValueMissing => NumLockReadState::ValueMissing,
                    ValueState::Present(_) => unreachable!(),
                };
                (read_state, NumLockRecordOutcome::DefaultedToOriginalOff)
            }
            Ok(state @ ValueState::Present(_)) => {
                let read_state = NumLockReadState::Present;
                let outcome = match &state {
                    ValueState::Present(value) if value.native_type != REG_SZ => {
                        NumLockRecordOutcome::UnsupportedRegistryType {
                            native_type: value.native_type,
                        }
                    }
                    ValueState::Present(value) if value.bytes.len() > MAX_STRING_BYTES => {
                        NumLockRecordOutcome::InvalidStringLength {
                            bytes: value.bytes.len(),
                        }
                    }
                    ValueState::Present(value) if value.bytes.len() % 2 != 0 => {
                        NumLockRecordOutcome::InvalidStringLength {
                            bytes: value.bytes.len(),
                        }
                    }
                    ValueState::Present(value) => {
                        let units = value
                            .bytes
                            .chunks_exact(2)
                            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
                        match char::decode_utf16(units).collect::<Result<String, _>>() {
                            Err(_) => NumLockRecordOutcome::InvalidUtf16,
                            Ok(mut decoded) => {
                                if decoded.ends_with('\0') {
                                    decoded.pop();
                                }
                                if decoded.contains('\0') {
                                    NumLockRecordOutcome::EmbeddedNul
                                } else if decoded == DESIRED_VALUE {
                                    NumLockRecordOutcome::MatchesDesired
                                } else {
                                    NumLockRecordOutcome::DifferentString
                                }
                            }
                        }
                    }
                    _ => unreachable!(),
                };
                (read_state, outcome)
            }
            Err(error) => (
                NumLockReadState::Failed,
                NumLockRecordOutcome::QueryFailed { error },
            ),
        };

        match outcome {
            NumLockRecordOutcome::MatchesDesired => {}
            NumLockRecordOutcome::DefaultedToOriginalOff
            | NumLockRecordOutcome::DifferentString => saw_unchecked = true,
            NumLockRecordOutcome::UnsupportedRegistryType { .. }
            | NumLockRecordOutcome::InvalidStringLength { .. }
            | NumLockRecordOutcome::InvalidUtf16
            | NumLockRecordOutcome::EmbeddedNul
            | NumLockRecordOutcome::QueryFailed { .. } => saw_unknown = true,
        }
        observations.push(NumLockObservation {
            target,
            read_state,
            outcome,
        });
    }

    let status = if saw_unchecked {
        ToggleStatus::Unchecked
    } else if saw_unknown {
        ToggleStatus::Unknown
    } else {
        ToggleStatus::Checked
    };

    Ok(NumLockStatusReport {
        report_kind: "source_audited_toggle_status",
        basis: "source_subset_only",
        executable: false,
        toggle_id: SUPPORTED_TOGGLE_ID.into(),
        registry_view: view,
        status,
        observations,
    })
}

/// Observe the source entry through the Windows read-only registry API.
#[cfg(windows)]
pub fn observe_windows(
    toggle_id: &str,
    view: RegistryView,
) -> Result<NumLockStatusReport, UnsupportedToggleId> {
    if !toggle_id.eq_ignore_ascii_case(SUPPORTED_TOGGLE_ID) {
        return Err(UnsupportedToggleId);
    }

    observe_numlock(toggle_id, view, |target| {
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
    use std::cell::RefCell;

    fn string_state(value: &str, trailing_nul: bool) -> ValueState {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        if trailing_nul {
            units.push(0);
        }
        ValueState::Present(crate::engine::registry::RegistryValue {
            native_type: REG_SZ,
            bytes: units.into_iter().flat_map(u16::to_le_bytes).collect(),
        })
    }

    fn observe(
        states: Vec<Result<ValueState, RegistryReadFailure>>,
    ) -> (NumLockStatusReport, Vec<ValueTarget>) {
        let states = RefCell::new(states.into_iter());
        let targets = RefCell::new(Vec::new());
        let report = observe_numlock(SUPPORTED_TOGGLE_ID, RegistryView::Registry32, |target| {
            targets.borrow_mut().push(target.clone());
            states.borrow_mut().next().expect("one state per query")
        })
        .unwrap();
        (report, targets.into_inner())
    }

    #[test]
    fn queries_exact_targets_in_source_order_and_preserves_view() {
        let (report, targets) = observe(vec![
            Ok(string_state("2", false)),
            Ok(string_state("2", true)),
        ]);
        assert_eq!(report.status, ToggleStatus::Checked);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].hive, Hive::Users);
        assert_eq!(targets[0].key_path, ".Default\\Control Panel\\Keyboard");
        assert_eq!(targets[1].hive, Hive::CurrentUser);
        assert_eq!(targets[1].key_path, "Control Panel\\Keyboard");
        assert!(targets.iter().all(|target| {
            target.value_name == VALUE_NAME && target.view == RegistryView::Registry32
        }));
        assert_eq!(report.observations[0].target, targets[0]);
        assert_eq!(report.observations[1].target, targets[1]);
        assert!(!report.executable);
    }

    #[test]
    fn missing_and_other_valid_strings_are_unchecked() {
        for state in [
            ValueState::KeyMissing,
            ValueState::ValueMissing,
            string_state("0", false),
            string_state("1", true),
            string_state("20", false),
            string_state("", false),
        ] {
            let (report, _) = observe(vec![Ok(state), Ok(string_state("2", false))]);
            assert_eq!(report.status, ToggleStatus::Unchecked);
            assert_eq!(report.observations.len(), 2);
        }
        let (report, _) = observe(vec![
            Ok(string_state("0", false)),
            Ok(string_state("2", false)),
        ]);
        assert_eq!(
            report.observations[0].outcome,
            NumLockRecordOutcome::DifferentString
        );
        assert_eq!(report.observations[0].read_state, NumLockReadState::Present);
    }

    #[test]
    fn wrong_type_odd_bytes_invalid_utf16_and_nuls_are_unknown() {
        let wrong_type = ValueState::Present(crate::engine::registry::RegistryValue {
            native_type: 4,
            bytes: vec![50, 0],
        });
        let odd = ValueState::Present(crate::engine::registry::RegistryValue {
            native_type: REG_SZ,
            bytes: vec![50],
        });
        let malformed = ValueState::Present(crate::engine::registry::RegistryValue {
            native_type: REG_SZ,
            bytes: vec![0x00, 0xD8],
        });
        let oversized = ValueState::Present(crate::engine::registry::RegistryValue {
            native_type: REG_SZ,
            bytes: vec![0; MAX_STRING_BYTES + 2],
        });
        let embedded = string_state("2\0x", false);
        let double_terminator = string_state("2\0", true);
        for state in [
            wrong_type,
            odd,
            malformed,
            oversized,
            embedded,
            double_terminator,
        ] {
            let (report, _) = observe(vec![Ok(state), Ok(string_state("2", false))]);
            assert_eq!(report.status, ToggleStatus::Unknown);
        }
    }

    #[test]
    fn query_errors_remain_unknown_but_definite_mismatch_wins() {
        let (unknown, _) = observe(vec![
            Err(RegistryReadFailure::ChangedTooOften),
            Ok(string_state("2", false)),
        ]);
        assert_eq!(unknown.status, ToggleStatus::Unknown);
        assert_eq!(unknown.observations[0].read_state, NumLockReadState::Failed);
        assert_eq!(
            unknown.observations[0].outcome,
            NumLockRecordOutcome::QueryFailed {
                error: RegistryReadFailure::ChangedTooOften
            }
        );

        let (mismatch, _) = observe(vec![
            Err(RegistryReadFailure::ValueTooLarge),
            Ok(string_state("0", false)),
        ]);
        assert_eq!(mismatch.status, ToggleStatus::Unchecked);
        assert_eq!(mismatch.observations.len(), 2);
    }

    #[test]
    fn report_serialization_contains_target_identity_and_is_non_executable() {
        let (report, _) = observe(vec![
            Ok(ValueState::ValueMissing),
            Ok(string_state("2", false)),
        ]);
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["toggle_id"], SUPPORTED_TOGGLE_ID);
        assert_eq!(json["executable"], false);
        assert_eq!(json["observations"][0]["target"]["hive"], "Users");
        assert_eq!(
            json["observations"][0]["target"]["key_path"],
            ".Default\\Control Panel\\Keyboard"
        );
        assert_eq!(json["observations"][1]["target"]["hive"], "CurrentUser");
        assert_eq!(json["observations"][1]["read_state"], "present");
        assert!(json["observations"][1].get("observed").is_none());
    }

    #[test]
    fn only_allowlisted_toggle_id_is_allowed_case_insensitively() {
        assert!(
            observe_numlock("WPFToggleNumLock", RegistryView::Native, |_| {
                Ok(ValueState::ValueMissing)
            })
            .is_ok()
        );
        assert!(
            observe_numlock("wpftogglenumlock", RegistryView::Native, |_| {
                Ok(ValueState::ValueMissing)
            })
            .is_ok()
        );
        assert!(
            observe_numlock("WPFToggleOther", RegistryView::Native, |_| unreachable!()).is_err()
        );
    }
}
