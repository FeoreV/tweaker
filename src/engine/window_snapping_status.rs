//! Read-only observer for the source-declared window snapping toggle.
//!
//! This reports only the configured `WindowArrangementActive` value and its
//! configured default. It does not write the registry or claim a Windows effect.

use crate::engine::{
    registry::{Hive, RegistryView, ValueState, ValueTarget},
    toggle_status::{RegistryReadFailure, ToggleStatus},
};
use serde::Serialize;

pub const SUPPORTED_TOGGLE_ID: &str = "WPFToggleWindowSnapping";
const KEY_PATH: &str = "Control Panel\\Desktop";
const VALUE_NAME: &str = "WindowArrangementActive";
const DESIRED_VALUE: &str = "1";
const REG_SZ: u32 = 1;
const MAX_REG_SZ_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowSnappingReadState {
    KeyMissing,
    ValueMissing,
    Present,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowSnappingOutcome {
    MatchesDesired,
    DefaultedToChecked,
    DifferentString,
    UnsupportedRegistryType { native_type: u32 },
    InvalidStringLength { bytes: usize },
    InvalidUtf16,
    EmbeddedNul,
    QueryFailed { error: RegistryReadFailure },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WindowSnappingObservation {
    pub target: ValueTarget,
    pub read_state: WindowSnappingReadState,
    pub outcome: WindowSnappingOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WindowSnappingStatusReport {
    pub report_kind: &'static str,
    pub basis: &'static str,
    pub executable: bool,
    pub toggle_id: String,
    pub registry_view: RegistryView,
    pub status: ToggleStatus,
    pub observations: Vec<WindowSnappingObservation>,
}

/// Observe the exact source declaration using a caller-supplied read-only query.
pub fn observe_window_snapping(
    view: RegistryView,
    mut read: impl FnMut(&ValueTarget) -> Result<ValueState, RegistryReadFailure>,
) -> WindowSnappingStatusReport {
    let target = ValueTarget {
        hive: Hive::CurrentUser,
        view: view.clone(),
        key_path: KEY_PATH.into(),
        value_name: VALUE_NAME.into(),
    };
    let (read_state, outcome) = match read(&target) {
        Ok(ValueState::KeyMissing) => (
            WindowSnappingReadState::KeyMissing,
            WindowSnappingOutcome::DefaultedToChecked,
        ),
        Ok(ValueState::ValueMissing) => (
            WindowSnappingReadState::ValueMissing,
            WindowSnappingOutcome::DefaultedToChecked,
        ),
        Ok(ValueState::Present(value)) => {
            let outcome = if value.native_type != REG_SZ {
                WindowSnappingOutcome::UnsupportedRegistryType {
                    native_type: value.native_type,
                }
            } else if value.bytes.len() > MAX_REG_SZ_BYTES || value.bytes.len() % 2 != 0 {
                WindowSnappingOutcome::InvalidStringLength {
                    bytes: value.bytes.len(),
                }
            } else {
                let units = value
                    .bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
                match char::decode_utf16(units).collect::<Result<String, _>>() {
                    Err(_) => WindowSnappingOutcome::InvalidUtf16,
                    Ok(mut decoded) => {
                        if decoded.ends_with('\0') {
                            decoded.pop();
                        }
                        if decoded.contains('\0') {
                            WindowSnappingOutcome::EmbeddedNul
                        } else if decoded.eq_ignore_ascii_case(DESIRED_VALUE) {
                            WindowSnappingOutcome::MatchesDesired
                        } else {
                            WindowSnappingOutcome::DifferentString
                        }
                    }
                }
            };
            (WindowSnappingReadState::Present, outcome)
        }
        Err(error) => (
            WindowSnappingReadState::Failed,
            WindowSnappingOutcome::QueryFailed { error },
        ),
    };
    let status = match outcome {
        WindowSnappingOutcome::MatchesDesired | WindowSnappingOutcome::DefaultedToChecked => {
            ToggleStatus::Checked
        }
        WindowSnappingOutcome::DifferentString => ToggleStatus::Unchecked,
        _ => ToggleStatus::Unknown,
    };
    WindowSnappingStatusReport {
        report_kind: "source_audited_toggle_status",
        basis: "source_subset_only",
        executable: false,
        toggle_id: SUPPORTED_TOGGLE_ID.into(),
        registry_view: view,
        status,
        observations: vec![WindowSnappingObservation {
            target,
            read_state,
            outcome,
        }],
    }
}

/// Observe the source declaration through the Windows read-only registry API.
#[cfg(windows)]
pub fn observe_windows(view: RegistryView) -> WindowSnappingStatusReport {
    observe_window_snapping(view, |target| {
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

    fn string_state(value: &str) -> ValueState {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        ValueState::Present(RegistryValue {
            native_type: REG_SZ,
            bytes: units.into_iter().flat_map(u16::to_le_bytes).collect(),
        })
    }

    fn observe(state: Result<ValueState, RegistryReadFailure>) -> WindowSnappingStatusReport {
        observe_window_snapping(RegistryView::Registry64, |_| state.clone())
    }

    #[test]
    fn missing_targets_default_to_checked_and_exact_value_matches() {
        for state in [ValueState::KeyMissing, ValueState::ValueMissing] {
            let report = observe(Ok(state));
            assert_eq!(report.status, ToggleStatus::Checked);
            assert_eq!(
                report.observations[0].outcome,
                WindowSnappingOutcome::DefaultedToChecked
            );
        }
        assert_eq!(observe(Ok(string_state("1"))).status, ToggleStatus::Checked);
    }

    #[test]
    fn mismatch_is_unchecked_and_uninterpretable_data_is_unknown() {
        assert_eq!(
            observe(Ok(string_state("0"))).status,
            ToggleStatus::Unchecked
        );
        let wrong_type = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1, 0, 0, 0],
        });
        let malformed = ValueState::Present(RegistryValue {
            native_type: REG_SZ,
            bytes: vec![0x00],
        });
        let invalid_utf16 = ValueState::Present(RegistryValue {
            native_type: REG_SZ,
            bytes: vec![0x00, 0xD8],
        });
        for state in [wrong_type, malformed, invalid_utf16] {
            assert_eq!(observe(Ok(state)).status, ToggleStatus::Unknown);
        }
        assert_eq!(
            observe(Err(RegistryReadFailure::Win32 { code: 5 })).status,
            ToggleStatus::Unknown
        );
    }

    #[test]
    fn reports_exact_target_without_exposing_registry_bytes() {
        let report = observe(Ok(string_state("1")));
        let observation = &report.observations[0];
        assert_eq!(observation.target.hive, Hive::CurrentUser);
        assert_eq!(observation.target.key_path, KEY_PATH);
        assert_eq!(observation.target.value_name, VALUE_NAME);
        assert_eq!(observation.target.view, RegistryView::Registry64);
        assert_eq!(report.basis, "source_subset_only");
        assert!(!report.executable);
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("bytes"));
    }
}
