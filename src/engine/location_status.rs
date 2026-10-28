//! Read-only status observer for the source-audited Location declaration.
//!
//! This reports a bounded source subset. It does not apply the tweak, cache
//! results, or make claims about system effects or supported Windows versions.
//! Missing `lfsvc` is skipped to match the upstream current-system helper;
//! unreadable/malformed values conservatively become unknown. The string
//! comparison is ASCII case-insensitive, not full PowerShell coercion/culture
//! parity.

#[cfg(windows)]
use crate::engine::services::ConfigResult;
use crate::engine::{
    registry::{Hive, RegistryView, ValueState, ValueTarget},
    toggle_status::{RegistryReadFailure, ToggleStatus},
};
use serde::Serialize;

pub const SUPPORTED_TWEAK_ID: &str = "WPFTweaksLocation";
const SERVICE_NAME: &str = "lfsvc";
const SERVICE_DISABLED: u32 = 4;
const REG_SZ: u32 = 1;
const REG_DWORD: u32 = 4;
const MAX_REG_SZ_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationReadState {
    KeyMissing,
    ValueMissing,
    Present,
    Failed,
    SkippedMissingService,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationRecordKind {
    Registry,
    Service,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationQueryFailure {
    Registry(RegistryReadFailure),
    Service(ServiceReadFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceReadFailure {
    InvalidName,
    OpenManager { code: u32 },
    OpenService { code: u32 },
    Query { code: u32 },
    InvalidHandle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationRecordOutcome {
    MatchesDesired,
    DifferentFromDesired,
    DefaultedToOriginal,
    UnsupportedRegistryType { native_type: u32 },
    InvalidRegistryData { observed_length: usize },
    InvalidUtf16,
    EmbeddedNul,
    UnsupportedServiceStartType { observed: u32 },
    QueryFailed { error: LocationQueryFailure },
    SkippedMissingService,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "target_kind", rename_all = "snake_case")]
pub enum LocationTarget {
    Registry { target: ValueTarget },
    Service { name: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LocationObservation {
    pub kind: LocationRecordKind,
    pub target: LocationTarget,
    pub read_state: LocationReadState,
    pub outcome: LocationRecordOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LocationStatusReport {
    pub report_kind: &'static str,
    pub basis: &'static str,
    pub executable: bool,
    pub tweak_id: String,
    pub registry_view: RegistryView,
    pub observations: Vec<LocationObservation>,
    pub status: ToggleStatus,
    pub source_caveat: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedTweakId;

impl std::fmt::Display for UnsupportedTweakId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported source-audited tweak ID")
    }
}

impl std::error::Error for UnsupportedTweakId {}

fn registry_targets(view: &RegistryView) -> [ValueTarget; 3] {
    [
        ValueTarget {
            hive: Hive::LocalMachine,
            view: view.clone(),
            key_path:
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location"
                    .into(),
            value_name: "Value".into(),
        },
        ValueTarget {
            hive: Hive::LocalMachine,
            view: view.clone(),
            key_path: "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Sensor\\Overrides\\{BFA794E4-F964-4FDB-90F6-51056BFE4B44}".into(),
            value_name: "SensorPermissionState".into(),
        },
        ValueTarget {
            hive: Hive::LocalMachine,
            view: view.clone(),
            key_path: "SYSTEM\\Maps".into(),
            value_name: "AutoUpdateEnabled".into(),
        },
    ]
}

#[cfg(windows)]
fn map_registry_error(error: crate::engine::registry::windows::ReadError) -> RegistryReadFailure {
    match error {
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
    }
}

#[cfg(windows)]
fn map_service_error(error: crate::engine::services::QueryError) -> ServiceReadFailure {
    match error {
        crate::engine::services::QueryError::InvalidName => ServiceReadFailure::InvalidName,
        crate::engine::services::QueryError::OpenManager(code) => {
            ServiceReadFailure::OpenManager { code }
        }
        crate::engine::services::QueryError::OpenService(code) => {
            ServiceReadFailure::OpenService { code }
        }
        crate::engine::services::QueryError::Query(code) => ServiceReadFailure::Query { code },
        crate::engine::services::QueryError::InvalidHandle => ServiceReadFailure::InvalidHandle,
    }
}

fn inspect_registry(
    target: ValueTarget,
    read: &mut impl FnMut(&ValueTarget) -> Result<ValueState, RegistryReadFailure>,
) -> LocationObservation {
    let (read_state, outcome) = match read(&target) {
        Ok(ValueState::KeyMissing) => (
            LocationReadState::KeyMissing,
            LocationRecordOutcome::DefaultedToOriginal,
        ),
        Ok(ValueState::ValueMissing) => (
            LocationReadState::ValueMissing,
            LocationRecordOutcome::DefaultedToOriginal,
        ),
        Ok(ValueState::Present(value)) => {
            if target.value_name == "Value" {
                match inspect_string(&value.native_type, &value.bytes) {
                    StringValue::Matches => (
                        LocationReadState::Present,
                        LocationRecordOutcome::MatchesDesired,
                    ),
                    StringValue::Mismatch => (
                        LocationReadState::Present,
                        LocationRecordOutcome::DifferentFromDesired,
                    ),
                    StringValue::WrongType(native_type) => (
                        LocationReadState::Present,
                        LocationRecordOutcome::UnsupportedRegistryType { native_type },
                    ),
                    StringValue::InvalidLength(observed_length) => (
                        LocationReadState::Present,
                        LocationRecordOutcome::InvalidRegistryData { observed_length },
                    ),
                    StringValue::InvalidUtf16 => (
                        LocationReadState::Present,
                        LocationRecordOutcome::InvalidUtf16,
                    ),
                    StringValue::EmbeddedNul => (
                        LocationReadState::Present,
                        LocationRecordOutcome::EmbeddedNul,
                    ),
                }
            } else {
                match inspect_dword(&value.native_type, &value.bytes) {
                    DwordValue::Matches => (
                        LocationReadState::Present,
                        LocationRecordOutcome::MatchesDesired,
                    ),
                    DwordValue::Mismatch => (
                        LocationReadState::Present,
                        LocationRecordOutcome::DifferentFromDesired,
                    ),
                    DwordValue::WrongType(native_type) => (
                        LocationReadState::Present,
                        LocationRecordOutcome::UnsupportedRegistryType { native_type },
                    ),
                    DwordValue::InvalidLength(observed_length) => (
                        LocationReadState::Present,
                        LocationRecordOutcome::InvalidRegistryData { observed_length },
                    ),
                }
            }
        }
        Err(error) => (
            LocationReadState::Failed,
            LocationRecordOutcome::QueryFailed {
                error: LocationQueryFailure::Registry(error),
            },
        ),
    };
    LocationObservation {
        kind: LocationRecordKind::Registry,
        target: LocationTarget::Registry { target },
        read_state,
        outcome,
    }
}

enum StringValue {
    Matches,
    Mismatch,
    WrongType(u32),
    InvalidLength(usize),
    InvalidUtf16,
    EmbeddedNul,
}

fn inspect_string(native_type: &u32, bytes: &[u8]) -> StringValue {
    if *native_type != REG_SZ {
        return StringValue::WrongType(*native_type);
    }
    if bytes.len() > MAX_REG_SZ_BYTES || !bytes.len().is_multiple_of(2) {
        return StringValue::InvalidLength(bytes.len());
    }
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
    match char::decode_utf16(units).collect::<Result<String, _>>() {
        Err(_) => StringValue::InvalidUtf16,
        Ok(mut value) => {
            if value.ends_with('\0') {
                value.pop();
            }
            if value.contains('\0') {
                StringValue::EmbeddedNul
            } else if value.eq_ignore_ascii_case("Deny") {
                StringValue::Matches
            } else {
                StringValue::Mismatch
            }
        }
    }
}

enum DwordValue {
    Matches,
    Mismatch,
    WrongType(u32),
    InvalidLength(usize),
}

fn inspect_dword(native_type: &u32, bytes: &[u8]) -> DwordValue {
    if *native_type != REG_DWORD {
        return DwordValue::WrongType(*native_type);
    }
    if bytes.len() != 4 {
        return DwordValue::InvalidLength(bytes.len());
    }
    let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if value == 0 {
        DwordValue::Matches
    } else {
        DwordValue::Mismatch
    }
}

fn inspect_service(
    query: &mut impl FnMut(&str) -> Result<Option<u32>, ServiceReadFailure>,
) -> LocationObservation {
    let (read_state, outcome) = match query(SERVICE_NAME) {
        Ok(None) => (
            LocationReadState::SkippedMissingService,
            LocationRecordOutcome::SkippedMissingService,
        ),
        Ok(Some(start_type)) if start_type > 4 => (
            LocationReadState::Present,
            LocationRecordOutcome::UnsupportedServiceStartType {
                observed: start_type,
            },
        ),
        Ok(Some(SERVICE_DISABLED)) => (
            LocationReadState::Present,
            LocationRecordOutcome::MatchesDesired,
        ),
        Ok(Some(_)) => (
            LocationReadState::Present,
            LocationRecordOutcome::DifferentFromDesired,
        ),
        Err(error) => (
            LocationReadState::Failed,
            LocationRecordOutcome::QueryFailed {
                error: LocationQueryFailure::Service(error),
            },
        ),
    };
    LocationObservation {
        kind: LocationRecordKind::Service,
        target: LocationTarget::Service {
            name: SERVICE_NAME.into(),
        },
        read_state,
        outcome,
    }
}

/// Read each source status target in order using injected query functions.
///
/// Missing `lfsvc` is explicitly skipped to match the upstream current-system
/// helper. PowerShell's nonterminating `Get-Service` errors may also leave a
/// null service object; that edge is not fully specified by the source tests.
pub fn observe_location(
    tweak_id: &str,
    view: RegistryView,
    mut read: impl FnMut(&ValueTarget) -> Result<ValueState, RegistryReadFailure>,
    mut query_service: impl FnMut(&str) -> Result<Option<u32>, ServiceReadFailure>,
) -> Result<LocationStatusReport, UnsupportedTweakId> {
    if tweak_id != SUPPORTED_TWEAK_ID {
        return Err(UnsupportedTweakId);
    }

    let mut observations = registry_targets(&view)
        .into_iter()
        .map(|target| inspect_registry(target, &mut read))
        .collect::<Vec<_>>();
    observations.push(inspect_service(&mut query_service));

    let has_mismatch = observations.iter().any(|observation| {
        matches!(
            &observation.outcome,
            LocationRecordOutcome::DifferentFromDesired
                | LocationRecordOutcome::DefaultedToOriginal
        )
    });
    let has_unknown = observations.iter().any(|observation| {
        matches!(
            &observation.outcome,
            LocationRecordOutcome::UnsupportedRegistryType { .. }
                | LocationRecordOutcome::InvalidRegistryData { .. }
                | LocationRecordOutcome::InvalidUtf16
                | LocationRecordOutcome::EmbeddedNul
                | LocationRecordOutcome::UnsupportedServiceStartType { .. }
                | LocationRecordOutcome::QueryFailed { .. }
        )
    });
    let status = if has_mismatch {
        ToggleStatus::Unchecked
    } else if has_unknown {
        ToggleStatus::Unknown
    } else {
        ToggleStatus::Checked
    };

    Ok(LocationStatusReport {
        report_kind: "source_audited_tweak_status",
        basis: "source_subset_only",
        executable: false,
        tweak_id: SUPPORTED_TWEAK_ID.into(),
        registry_view: view,
        observations,
        status,
        source_caveat: "missing service is skipped as in the source helper; query errors and malformed values are unknown here; Deny comparison uses ASCII case-insensitive matching, not full PowerShell coercion or culture rules",
    })
}

/// Query the bounded source subset through read-only Win32 APIs.
#[cfg(windows)]
pub fn observe_windows(
    tweak_id: &str,
    view: RegistryView,
) -> Result<LocationStatusReport, UnsupportedTweakId> {
    observe_location(
        tweak_id,
        view,
        |target| crate::engine::registry::windows::read(target).map_err(map_registry_error),
        |name| {
            crate::engine::services::query_config(name)
                .map(|result| match result {
                    ConfigResult::Missing => None,
                    ConfigResult::Present { start_type, .. } => Some(start_type),
                })
                .map_err(map_service_error)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::registry::RegistryValue;
    use std::cell::RefCell;

    fn string_value(value: &str, ty: u32) -> ValueState {
        ValueState::Present(RegistryValue {
            native_type: ty,
            bytes: value.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        })
    }

    fn terminated_string_value(value: &str, ty: u32) -> ValueState {
        let mut bytes = value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        bytes.extend_from_slice(&[0, 0]);
        ValueState::Present(RegistryValue {
            native_type: ty,
            bytes,
        })
    }

    fn dword_value(value: u32, ty: u32) -> ValueState {
        ValueState::Present(RegistryValue {
            native_type: ty,
            bytes: value.to_le_bytes().to_vec(),
        })
    }

    fn match_states() -> Vec<Result<ValueState, RegistryReadFailure>> {
        vec![
            Ok(string_value("Deny", REG_SZ)),
            Ok(dword_value(0, REG_DWORD)),
            Ok(dword_value(0, REG_DWORD)),
        ]
    }

    fn observe(
        states: Vec<Result<ValueState, RegistryReadFailure>>,
        service: Result<Option<u32>, ServiceReadFailure>,
    ) -> (LocationStatusReport, Vec<ValueTarget>, Vec<String>) {
        let states = RefCell::new(states.into_iter());
        let targets = RefCell::new(Vec::new());
        let services = RefCell::new(Vec::new());
        let report = observe_location(
            SUPPORTED_TWEAK_ID,
            RegistryView::Registry64,
            |target| {
                targets.borrow_mut().push(target.clone());
                states.borrow_mut().next().expect("three registry reads")
            },
            |name| {
                services.borrow_mut().push(name.to_owned());
                service
            },
        )
        .unwrap();
        (report, targets.into_inner(), services.into_inner())
    }

    #[test]
    fn reports_all_targets_in_status_order_with_selected_view() {
        let (report, targets, services) = observe(match_states(), Ok(Some(SERVICE_DISABLED)));
        assert_eq!(report.status, ToggleStatus::Checked);
        assert_eq!(targets.len(), 3);
        assert_eq!(services, vec![SERVICE_NAME.to_owned()]);
        assert_eq!(
            targets
                .iter()
                .map(|target| (target.hive.clone(), target.key_path.as_str(), target.value_name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (
                    Hive::LocalMachine,
                    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location",
                    "Value"
                ),
                (
                    Hive::LocalMachine,
                    "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Sensor\\Overrides\\{BFA794E4-F964-4FDB-90F6-51056BFE4B44}",
                    "SensorPermissionState"
                ),
                (Hive::LocalMachine, "SYSTEM\\Maps", "AutoUpdateEnabled"),
            ]
        );
        assert!(targets
            .iter()
            .all(|target| target.view == RegistryView::Registry64));
        assert!(matches!(
            &report.observations[3].target,
            LocationTarget::Service { ref name } if name == "lfsvc"
        ));
        assert_eq!(report.observations[3].kind, LocationRecordKind::Service);
    }

    #[test]
    fn missing_service_is_skipped_and_does_not_prevent_a_match() {
        let (report, _, _) = observe(match_states(), Ok(None));
        assert_eq!(report.status, ToggleStatus::Checked);
        assert_eq!(
            report.observations[3].read_state,
            LocationReadState::SkippedMissingService
        );
        assert_eq!(
            report.observations[3].outcome,
            LocationRecordOutcome::SkippedMissingService
        );
    }

    #[test]
    fn source_string_comparison_is_case_insensitive_and_accepts_one_terminator() {
        let mut states = match_states();
        states[0] = Ok(terminated_string_value("dEnY", REG_SZ));
        let (report, _, _) = observe(states, Ok(Some(4)));
        assert_eq!(report.status, ToggleStatus::Checked);
    }

    #[test]
    fn service_start_type_matches_or_mismatches_exactly() {
        let (matched, _, _) = observe(match_states(), Ok(Some(4)));
        assert_eq!(matched.status, ToggleStatus::Checked);
        for start_type in [0, 1, 2, 3] {
            let (report, _, _) = observe(match_states(), Ok(Some(start_type)));
            assert_eq!(report.status, ToggleStatus::Unchecked);
        }
        let (invalid, _, _) = observe(match_states(), Ok(Some(5)));
        assert_eq!(invalid.status, ToggleStatus::Unknown);
        assert_eq!(
            invalid.observations[3].outcome,
            LocationRecordOutcome::UnsupportedServiceStartType { observed: 5 }
        );
    }

    #[test]
    fn each_registry_missing_state_defaults_to_original_and_mismatches() {
        for index in 0..3 {
            for missing in [ValueState::KeyMissing, ValueState::ValueMissing] {
                let mut states = match_states();
                states[index] = Ok(missing);
                let (report, _, _) = observe(states, Ok(Some(4)));
                assert_eq!(report.status, ToggleStatus::Unchecked);
                assert_eq!(
                    report.observations[index].outcome,
                    LocationRecordOutcome::DefaultedToOriginal
                );
            }
        }
    }

    #[test]
    fn present_registry_mismatches_are_definite() {
        let variants = [
            (0, string_value("Allow", REG_SZ)),
            (1, dword_value(1, REG_DWORD)),
            (2, dword_value(1, REG_DWORD)),
        ];
        for (index, mismatch) in variants {
            let mut states = match_states();
            states[index] = Ok(mismatch);
            let (report, _, _) = observe(states, Ok(Some(4)));
            assert_eq!(report.status, ToggleStatus::Unchecked);
        }
    }

    #[test]
    fn malformed_registry_data_is_unknown_and_mismatch_takes_precedence() {
        let malformed_string = ValueState::Present(RegistryValue {
            native_type: REG_SZ,
            bytes: vec![0x00, 0xD8],
        });
        let embedded_nul = string_value("De\0ny", REG_SZ);
        let odd = ValueState::Present(RegistryValue {
            native_type: REG_SZ,
            bytes: vec![b'D'],
        });
        let oversized = ValueState::Present(RegistryValue {
            native_type: REG_SZ,
            bytes: vec![0; MAX_REG_SZ_BYTES + 2],
        });
        let wrong_string_type = string_value("Deny", REG_DWORD);
        let double_terminator = terminated_string_value("Deny\0", REG_SZ);
        let bad_dword_length = ValueState::Present(RegistryValue {
            native_type: REG_DWORD,
            bytes: vec![0; 3],
        });
        for malformed in [
            malformed_string,
            embedded_nul,
            odd,
            oversized,
            wrong_string_type,
            double_terminator,
            bad_dword_length,
        ] {
            let mut states = match_states();
            states[0] = Ok(malformed);
            let (report, _, _) = observe(states.clone(), Ok(Some(4)));
            assert_eq!(report.status, ToggleStatus::Unknown);
            states[2] = Ok(dword_value(1, REG_DWORD));
            let (report, _, _) = observe(states, Ok(Some(4)));
            assert_eq!(report.status, ToggleStatus::Unchecked);
        }
    }

    #[test]
    fn query_errors_are_unknown_with_their_source() {
        let (registry_error, _, _) = observe(
            vec![
                Err(RegistryReadFailure::ValueTooLarge),
                match_states()[1].clone(),
                match_states()[2].clone(),
            ],
            Ok(Some(4)),
        );
        assert_eq!(registry_error.status, ToggleStatus::Unknown);
        assert!(matches!(
            registry_error.observations[0].outcome,
            LocationRecordOutcome::QueryFailed {
                error: LocationQueryFailure::Registry(RegistryReadFailure::ValueTooLarge)
            }
        ));

        for error in [
            ServiceReadFailure::InvalidName,
            ServiceReadFailure::OpenManager { code: 5 },
            ServiceReadFailure::OpenService { code: 1060 },
            ServiceReadFailure::Query { code: 5 },
            ServiceReadFailure::InvalidHandle,
        ] {
            let (report, _, _) = observe(match_states(), Err(error));
            assert_eq!(report.status, ToggleStatus::Unknown);
            assert_eq!(
                report.observations[3].outcome,
                LocationRecordOutcome::QueryFailed {
                    error: LocationQueryFailure::Service(error)
                }
            );
        }
    }

    #[test]
    fn allowlist_is_checked_before_querying() {
        assert!(observe_location(
            "WPFTweaksLocation",
            RegistryView::Native,
            |_| Ok(ValueState::ValueMissing),
            |_| Ok(None),
        )
        .is_ok());
        assert!(observe_location(
            "WPFTweaksOther",
            RegistryView::Native,
            |_| panic!("registry reader must not run"),
            |_| panic!("service reader must not run"),
        )
        .is_err());
    }

    #[test]
    fn serialization_has_no_raw_registry_data_and_marks_report_unavailable_for_execution() {
        let (report, _, _) = observe(match_states(), Ok(None));
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["tweak_id"], SUPPORTED_TWEAK_ID);
        assert_eq!(json["executable"], false);
        assert_eq!(json["basis"], "source_subset_only");
        assert_eq!(
            json["observations"][0]["target"]["target"]["value_name"],
            "Value"
        );
        assert_eq!(
            json["observations"][3]["read_state"],
            "skipped_missing_service"
        );
        assert!(json.get("bytes").is_none());
        assert!(!json.to_string().contains("[68,0,101,0,110,0,121,0]"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_query_errors_map_to_read_only_failure_categories() {
        use crate::engine::{registry::windows::ReadError, services::QueryError};

        assert_eq!(
            map_registry_error(ReadError::Win32(5)),
            RegistryReadFailure::Win32 { code: 5 }
        );
        assert_eq!(
            map_registry_error(ReadError::ChangedTooOften),
            RegistryReadFailure::ChangedTooOften
        );
        assert_eq!(
            map_service_error(QueryError::OpenManager(5)),
            ServiceReadFailure::OpenManager { code: 5 }
        );
        assert_eq!(
            map_service_error(QueryError::OpenService(1060)),
            ServiceReadFailure::OpenService { code: 1060 }
        );
        assert_eq!(
            map_service_error(QueryError::Query(5)),
            ServiceReadFailure::Query { code: 5 }
        );
        assert_eq!(
            map_service_error(QueryError::InvalidHandle),
            ServiceReadFailure::InvalidHandle
        );
    }
}
