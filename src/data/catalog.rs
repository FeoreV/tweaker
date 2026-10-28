//! Versioned declarative tweak catalog schema.
//!
//! Catalog operations describe intended values only. They do not contain
//! rollback values, executable scripts, or an API that applies changes. A
//! caller must capture exact pre-state in the managed transaction layer.

use super::journal::ServiceTarget;
use crate::engine::registry::{RegistryValue, ValueTarget};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const MAX_CATALOG_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TweakCatalog {
    pub schema_version: u32,
    pub source: SourceRevision,
    pub tweaks: Vec<TweakDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRevision {
    /// Repository-relative source path; never resolved relative to the process CWD.
    pub path: String,
    /// Uppercase or lowercase SHA-256 hex digest of the source file.
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TweakDefinition {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub source_id: Option<String>,
    pub support: SupportEvidence,
    operations: Vec<CatalogOperation>,
}

impl TweakDefinition {
    /// Return operations only when the entry explicitly carries verified support metadata.
    /// This is a policy gate, not proof that the attached evidence is truthful or sufficient.
    pub fn verified_operations(&self) -> Option<&[CatalogOperation]> {
        match self.support {
            SupportEvidence::Unverified => None,
            SupportEvidence::Verified { .. } => Some(&self.operations),
        }
    }
}

/// OS support is unverified unless a concrete target range and evidence are supplied.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum SupportEvidence {
    Unverified,
    Verified {
        min_ntddi: u32,
        max_ntddi: Option<u32>,
        evidence: Vec<String>,
    },
}

/// Only declarative registry and service startup-type operations are representable.
/// There is deliberately no script, shell command, package, file, or guessed undo variant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogOperation {
    SetRegistryValue {
        target: ValueTarget,
        value: RegistryValue,
    },
    DeleteRegistryValue {
        target: ValueTarget,
    },
    SetServiceStartType {
        target: ServiceTarget,
        start_type: u32,
    },
    SetServiceStartTypeIfExpected {
        target: ServiceTarget,
        start_type: u32,
        expected_start_type: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogError {
    UnsupportedSchemaVersion(u32),
    InvalidSourcePath,
    InvalidSourceHash,
    InvalidTweakId(String),
    DuplicateTweakId(String),
    EmptyTitle(String),
    EmptySummary(String),
    NoOperations(String),
    InvalidSourceId(String),
    InvalidSupportRange(String),
    MissingSupportEvidence(String),
    InvalidOperationTarget(String),
    InvalidServiceStartType(String),
}

#[derive(Debug)]
pub enum CatalogLoadError {
    TooLarge,
    InvalidJson(serde_json::Error),
    InvalidCatalog(CatalogError),
}

/// Decode and validate catalog bytes. This function never executes an operation.
pub fn decode_catalog(bytes: &[u8]) -> Result<TweakCatalog, CatalogLoadError> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(CatalogLoadError::TooLarge);
    }
    let catalog: TweakCatalog =
        serde_json::from_slice(bytes).map_err(CatalogLoadError::InvalidJson)?;
    catalog
        .validate()
        .map_err(CatalogLoadError::InvalidCatalog)?;
    Ok(catalog)
}

impl TweakCatalog {
    /// Validate structure and identifiers. This does not prove that an operation is safe or supported.
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedSchemaVersion(self.schema_version));
        }
        if !valid_relative_path(&self.source.path) {
            return Err(CatalogError::InvalidSourcePath);
        }
        if self.source.sha256.len() != 64
            || !self
                .source
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(CatalogError::InvalidSourceHash);
        }

        let mut ids = HashSet::with_capacity(self.tweaks.len());
        for tweak in &self.tweaks {
            if !valid_identifier(&tweak.id) {
                return Err(CatalogError::InvalidTweakId(tweak.id.clone()));
            }
            if !ids.insert(tweak.id.as_str()) {
                return Err(CatalogError::DuplicateTweakId(tweak.id.clone()));
            }
            if tweak.title.trim().is_empty() {
                return Err(CatalogError::EmptyTitle(tweak.id.clone()));
            }
            if tweak.summary.trim().is_empty() {
                return Err(CatalogError::EmptySummary(tweak.id.clone()));
            }
            if tweak.operations.is_empty() {
                return Err(CatalogError::NoOperations(tweak.id.clone()));
            }
            if let Some(source_id) = &tweak.source_id {
                if !valid_identifier(source_id) {
                    return Err(CatalogError::InvalidSourceId(tweak.id.clone()));
                }
            }
            validate_support(tweak)?;
            for operation in &tweak.operations {
                match operation {
                    CatalogOperation::SetServiceStartType { start_type, .. }
                        if *start_type > super::journal::SERVICE_START_TYPE_MAX =>
                    {
                        return Err(CatalogError::InvalidServiceStartType(tweak.id.clone()));
                    }
                    CatalogOperation::SetServiceStartTypeIfExpected {
                        start_type,
                        expected_start_type,
                        ..
                    } if *start_type > super::journal::SERVICE_START_TYPE_MAX
                        || *expected_start_type > super::journal::SERVICE_START_TYPE_MAX =>
                    {
                        return Err(CatalogError::InvalidServiceStartType(tweak.id.clone()));
                    }
                    _ if !valid_operation_target(operation) => {
                        return Err(CatalogError::InvalidOperationTarget(tweak.id.clone()));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_relative_path(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.contains('\0')
        && !value.starts_with(['/', '\\'])
        && !value.contains(':')
        && value
            .split(['/', '\\'])
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn validate_support(tweak: &TweakDefinition) -> Result<(), CatalogError> {
    if let SupportEvidence::Verified {
        min_ntddi,
        max_ntddi,
        evidence,
    } = &tweak.support
    {
        if max_ntddi.is_some_and(|maximum| maximum < *min_ntddi) {
            return Err(CatalogError::InvalidSupportRange(tweak.id.clone()));
        }
        if *min_ntddi == 0 {
            return Err(CatalogError::InvalidSupportRange(tweak.id.clone()));
        }
        if evidence.is_empty() || evidence.iter().any(|item| item.trim().is_empty()) {
            return Err(CatalogError::MissingSupportEvidence(tweak.id.clone()));
        }
    }
    Ok(())
}

fn valid_operation_target(operation: &CatalogOperation) -> bool {
    match operation {
        CatalogOperation::SetRegistryValue { target, .. }
        | CatalogOperation::DeleteRegistryValue { target } => {
            valid_relative_path(&target.key_path)
                && !target.key_path.contains('/')
                && !target.key_path.contains('\0')
                && !target.value_name.contains('\0')
        }
        CatalogOperation::SetServiceStartType { target, .. }
        | CatalogOperation::SetServiceStartTypeIfExpected { target, .. } => {
            super::journal::valid_service_name(&target.name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::registry::{Hive, RegistryView};

    const REVIEW_FIXTURE: &[u8] = include_bytes!("../../config/catalog.v1.json");
    const REGISTRY_REVIEW: &[u8] = include_bytes!("../../config/catalog.registry-review.v1.json");

    #[test]
    fn registry_review_decodes_without_authorizing_operations() {
        let catalog = decode_catalog(REGISTRY_REVIEW).unwrap();
        assert_eq!(catalog.tweaks.len(), 35);
        assert_eq!(
            catalog
                .tweaks
                .iter()
                .map(|t| t.operations.len())
                .sum::<usize>(),
            76
        );
        assert!(catalog
            .tweaks
            .iter()
            .all(|t| t.verified_operations().is_none()));
    }

    #[test]
    fn location_review_operations_decode_in_source_order() {
        let review: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../config/catalog.location-review.v1.json"
        ))
        .unwrap();
        assert_eq!(review["tweak"]["support"]["status"], "unverified");
        let operations: Vec<CatalogOperation> =
            serde_json::from_value(review["tweak"]["operations"].clone()).unwrap();
        assert_eq!(operations.len(), 4);
        assert!(
            matches!(&operations[0], CatalogOperation::SetServiceStartTypeIfExpected { target, start_type: 4, expected_start_type: 3 } if target.name == "lfsvc")
        );
        assert!(operations[1..]
            .iter()
            .all(|op| matches!(op, CatalogOperation::SetRegistryValue { .. })));
    }

    fn sample() -> TweakCatalog {
        TweakCatalog {
            schema_version: CATALOG_SCHEMA_VERSION,
            source: SourceRevision {
                path: "catalog/tweaks.json".into(),
                sha256: "A".repeat(64),
            },
            tweaks: vec![TweakDefinition {
                id: "ExampleToggle".into(),
                title: "Example toggle".into(),
                summary: "Sets one registry value.".into(),
                source_id: Some("WPFTweaksExample".into()),
                support: SupportEvidence::Unverified,
                operations: vec![CatalogOperation::SetRegistryValue {
                    target: ValueTarget {
                        hive: Hive::CurrentUser,
                        view: RegistryView::Native,
                        key_path: "Software\\Example".into(),
                        value_name: "Enabled".into(),
                    },
                    value: RegistryValue {
                        native_type: 4,
                        bytes: vec![1, 0, 0, 0],
                    },
                }],
            }],
        }
    }

    #[test]
    fn schema_round_trips_typed_bytes_and_unverified_support() {
        let catalog = sample();
        catalog.validate().unwrap();
        let json = serde_json::to_string(&catalog).unwrap();
        let decoded: TweakCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, catalog);
    }

    #[test]
    fn rejects_unknown_schema_fields_and_unknown_operation_kinds() {
        let unknown_field = serde_json::to_string(&sample()).unwrap().replace(
            "\"schema_version\":1",
            "\"schema_version\":1,\"execute_script\":\"unsafe\"",
        );
        assert!(serde_json::from_str::<TweakCatalog>(&unknown_field).is_err());

        let unknown_operation = serde_json::to_string(&sample())
            .unwrap()
            .replace("set_registry_value", "run_script");
        assert!(serde_json::from_str::<TweakCatalog>(&unknown_operation).is_err());
    }

    #[test]
    fn rejects_bad_versions_duplicate_ids_and_missing_verified_evidence() {
        let mut catalog = sample();
        catalog.schema_version += 1;
        assert_eq!(
            catalog.validate(),
            Err(CatalogError::UnsupportedSchemaVersion(
                CATALOG_SCHEMA_VERSION + 1
            ))
        );

        let mut catalog = sample();
        catalog.tweaks.push(catalog.tweaks[0].clone());
        assert_eq!(
            catalog.validate(),
            Err(CatalogError::DuplicateTweakId("ExampleToggle".into()))
        );

        let mut catalog = sample();
        catalog.tweaks[0].support = SupportEvidence::Verified {
            min_ntddi: 0x0601_0000,
            max_ntddi: None,
            evidence: Vec::new(),
        };
        assert_eq!(
            catalog.validate(),
            Err(CatalogError::MissingSupportEvidence("ExampleToggle".into()))
        );
    }

    #[test]
    fn rejects_invalid_paths_and_support_ranges() {
        let mut catalog = sample();
        catalog.source.sha256 = "not-a-hash".into();
        assert_eq!(catalog.validate(), Err(CatalogError::InvalidSourceHash));

        let mut catalog = sample();
        catalog.source.path = "../outside.json".into();
        assert_eq!(catalog.validate(), Err(CatalogError::InvalidSourcePath));

        let mut catalog = sample();
        catalog.tweaks[0].support = SupportEvidence::Verified {
            min_ntddi: 0x0A00_0000,
            max_ntddi: Some(0x0601_0000),
            evidence: vec!["test evidence".into()],
        };
        assert_eq!(
            catalog.validate(),
            Err(CatalogError::InvalidSupportRange("ExampleToggle".into()))
        );

        let mut catalog = sample();
        if let CatalogOperation::SetRegistryValue { target, .. } =
            &mut catalog.tweaks[0].operations[0]
        {
            target.key_path.clear();
        }
        assert_eq!(
            catalog.validate(),
            Err(CatalogError::InvalidOperationTarget("ExampleToggle".into()))
        );
    }

    #[test]
    fn rejects_invalid_service_names_in_catalog_operations() {
        for name in [
            "",
            "has\0nul",
            "has/forward",
            "has\\backslash",
            "has,comma",
            "has space",
        ] {
            let mut catalog = sample();
            catalog.tweaks[0].operations = vec![CatalogOperation::SetServiceStartType {
                target: ServiceTarget { name: name.into() },
                start_type: 3,
            }];
            assert_eq!(
                catalog.validate(),
                Err(CatalogError::InvalidOperationTarget("ExampleToggle".into())),
                "service name should be rejected: {name:?}"
            );
        }

        let mut catalog = sample();
        catalog.tweaks[0].operations = vec![CatalogOperation::SetServiceStartType {
            target: ServiceTarget {
                name: "A".repeat(super::super::journal::MAX_SERVICE_NAME_UTF16_UNITS + 1),
            },
            start_type: 3,
        }];
        assert_eq!(
            catalog.validate(),
            Err(CatalogError::InvalidOperationTarget("ExampleToggle".into()))
        );
    }

    #[test]
    fn review_fixture_decodes_as_unverified_data_only_catalog() {
        let catalog = decode_catalog(REVIEW_FIXTURE).unwrap();
        assert_eq!(catalog.schema_version, CATALOG_SCHEMA_VERSION);
        assert_eq!(catalog.tweaks.len(), 1);
        assert_eq!(catalog.tweaks[0].id, "WPFTweaksActivity");
        assert_eq!(catalog.tweaks[0].operations.len(), 3);
        assert_eq!(catalog.tweaks[0].support, SupportEvidence::Unverified);
        assert!(catalog.tweaks[0].verified_operations().is_none());
        assert_eq!(
            catalog.source.sha256,
            "CEEEAF210309390CE094A16BB3F0467852E915E9F946E7C7C723A9AF1FCBB2F9"
        );
        let expected_names = [
            "EnableActivityFeed",
            "PublishUserActivities",
            "UploadUserActivities",
        ];
        for (operation, expected_name) in catalog.tweaks[0].operations.iter().zip(expected_names) {
            let CatalogOperation::SetRegistryValue { target, value } = operation else {
                panic!("fixture must contain only registry set operations");
            };
            assert_eq!(target.hive, Hive::LocalMachine);
            assert_eq!(target.view, RegistryView::Registry64);
            assert_eq!(
                target.key_path,
                "SOFTWARE\\Policies\\Microsoft\\Windows\\System"
            );
            assert_eq!(target.value_name, expected_name);
            assert_eq!(value.native_type, 4);
            assert_eq!(value.bytes.as_slice(), &[0, 0, 0, 0]);
        }
    }

    #[test]
    fn review_fixture_mock_apply_and_undo_preserves_actual_prestate() {
        use crate::engine::recovery::{recover_one, RecoveryBackend, RecoveryRecord};
        use crate::engine::registry::ValueState;
        use crate::engine::transaction::{apply_one, Backend, Operation, SnapshotStore};
        use std::collections::HashMap;
        use std::convert::Infallible;

        #[derive(Default)]
        struct Registry(HashMap<ValueTarget, ValueState>);
        impl Backend for Registry {
            type Target = ValueTarget;
            type Change = ValueState;
            type Snapshot = ValueState;
            type Error = Infallible;
            fn query(&mut self, target: &ValueTarget) -> Result<ValueState, Infallible> {
                Ok(self
                    .0
                    .get(target)
                    .cloned()
                    .unwrap_or(ValueState::ValueMissing))
            }
            fn apply(
                &mut self,
                target: &ValueTarget,
                state: &ValueState,
            ) -> Result<(), Infallible> {
                self.0.insert(target.clone(), state.clone());
                Ok(())
            }
        }
        impl RecoveryBackend for Registry {
            type Target = ValueTarget;
            type State = ValueState;
            type Error = Infallible;
            fn query(&mut self, target: &ValueTarget) -> Result<ValueState, Infallible> {
                Backend::query(self, target)
            }
            fn restore(
                &mut self,
                target: &ValueTarget,
                state: &ValueState,
            ) -> Result<(), Infallible> {
                Backend::apply(self, target, state)
            }
        }
        #[derive(Default)]
        struct Journal(Vec<RecoveryRecord<ValueTarget, ValueState>>);
        impl SnapshotStore<ValueTarget, ValueState, ValueState> for Journal {
            type Error = Infallible;
            fn persist(
                &mut self,
                target: &ValueTarget,
                before: &ValueState,
                intended: &ValueState,
            ) -> Result<(), Infallible> {
                self.0.push(RecoveryRecord {
                    target: target.clone(),
                    before: before.clone(),
                    intended: intended.clone(),
                });
                Ok(())
            }
        }

        let catalog = decode_catalog(REVIEW_FIXTURE).unwrap();
        let operations = &catalog.tweaks[0].operations;
        let CatalogOperation::SetRegistryValue { target: first, .. } = &operations[0] else {
            panic!("unexpected fixture operation")
        };
        let original = ValueState::Present(RegistryValue {
            native_type: 7,
            bytes: vec![1, 0, 255, 0],
        });
        let mut registry = Registry::default();
        registry.0.insert(first.clone(), original.clone());
        let mut journal = Journal::default();
        for entry in operations {
            let CatalogOperation::SetRegistryValue { target, value } = entry else {
                panic!("unexpected fixture operation")
            };
            apply_one(
                &mut registry,
                &mut journal,
                &Operation {
                    target: target.clone(),
                    change: ValueState::Present(value.clone()),
                },
            )
            .unwrap();
            assert_eq!(registry.0[target], ValueState::Present(value.clone()));
        }
        let records = std::mem::take(&mut journal.0);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].before, original);
        assert_eq!(records[1].before, ValueState::ValueMissing);
        assert_eq!(records[2].before, ValueState::ValueMissing);
        for record in records.iter().rev() {
            recover_one(&mut registry, &mut journal, record).unwrap();
            assert_eq!(registry.0[&record.target], record.before);
        }
        assert_eq!(journal.0.len(), 3); // Recovery also snapshots before writing.
    }

    #[test]
    fn conditional_service_schema_is_distinct_and_validated() {
        let json = r#"{"operation":"set_service_start_type_if_expected","target":{"name":"lfsvc"},"start_type":4,"expected_start_type":3}"#;
        let operation: CatalogOperation = serde_json::from_str(json).unwrap();
        assert_eq!(
            serde_json::from_str::<CatalogOperation>(&serde_json::to_string(&operation).unwrap())
                .unwrap(),
            operation
        );
        let mut catalog = decode_catalog(REVIEW_FIXTURE).unwrap();
        catalog.tweaks[0].operations = vec![operation];
        assert!(catalog.validate().is_ok());
        catalog.tweaks[0].operations = vec![CatalogOperation::SetServiceStartTypeIfExpected {
            target: ServiceTarget {
                name: "lfsvc".into(),
            },
            start_type: 4,
            expected_start_type: 5,
        }];
        assert!(matches!(
            catalog.validate(),
            Err(CatalogError::InvalidServiceStartType(_))
        ));
        let legacy =
            r#"{"operation":"set_service_start_type","target":{"name":"lfsvc"},"start_type":4}"#;
        assert!(matches!(
            serde_json::from_str::<CatalogOperation>(legacy),
            Ok(CatalogOperation::SetServiceStartType { .. })
        ));
    }

    #[test]
    fn decode_catalog_rejects_oversized_input_before_parsing() {
        let input = vec![b' '; MAX_CATALOG_BYTES + 1];
        assert!(matches!(
            decode_catalog(&input),
            Err(CatalogLoadError::TooLarge)
        ));
    }

    #[test]
    fn decode_catalog_distinguishes_json_and_validation_errors() {
        assert!(matches!(
            decode_catalog(b"{"),
            Err(CatalogLoadError::InvalidJson(_))
        ));

        let mut catalog = sample();
        catalog.schema_version += 1;
        let bytes = serde_json::to_vec(&catalog).unwrap();
        assert!(matches!(
            decode_catalog(&bytes),
            Err(CatalogLoadError::InvalidCatalog(
                CatalogError::UnsupportedSchemaVersion(_)
            ))
        ));
    }
}
