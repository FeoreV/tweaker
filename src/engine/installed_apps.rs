//! Pure models and bounded decoders for uninstall-registry inventory.
//!
//! Uninstall registry entries are heuristic installed-software registrations;
//! they do not represent WinGet or Chocolatey installed-ID state. Results keep
//! every hive/view scope and duplicate entry rather than deduplicating claims.

use crate::engine::registry::{Hive, RegistryValue, RegistryView};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Maximum encoded registry string size accepted for an individual field.
pub const MAX_REGISTRY_STRING_BYTES: usize = 1024 * 1024;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InventoryScope {
    pub hive: Hive,
    pub view: RegistryView,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InstalledApplication {
    /// Registry subkey name; intentionally not treated as a package ID.
    pub subkey: String,
    pub subkey_error: Option<u32>,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub field_errors: BTreeMap<String, InstalledFieldError>,
    /// Unrecognized source values retained verbatim by the caller when known.
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeStatus {
    Complete,
    MissingKey,
    Partial(u32),
    Failed(u32),
    LimitReached,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeReport {
    pub scope: InventoryScope,
    pub status: ScopeStatus,
    pub applications: Vec<InstalledApplication>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct InventoryReport {
    pub scopes: Vec<ScopeReport>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegistryStringError {
    UnsupportedType(u32),
    OddByteLength,
    InvalidUtf16,
    InteriorNul,
    TooLarge,
}

/// A field can fail during the registry read or during local decoding.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum InstalledFieldError {
    RegistryReadFailed(u32),
    InvalidString(RegistryStringError),
}

/// Decode REG_SZ or REG_EXPAND_SZ bytes as UTF-16 without environment expansion.
/// One trailing NUL is optional; interior NULs and malformed UTF-16 are rejected.
pub fn decode_registry_string(value: &RegistryValue) -> Result<String, RegistryStringError> {
    if value.native_type != REG_SZ && value.native_type != REG_EXPAND_SZ {
        return Err(RegistryStringError::UnsupportedType(value.native_type));
    }
    if value.bytes.len() > MAX_REGISTRY_STRING_BYTES {
        return Err(RegistryStringError::TooLarge);
    }
    if value.bytes.len() & 1 != 0 {
        return Err(RegistryStringError::OddByteLength);
    }
    let mut units: Vec<u16> = value
        .bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.last() == Some(&0) {
        units.pop();
    }
    if units.contains(&0) {
        return Err(RegistryStringError::InteriorNul);
    }
    String::from_utf16(&units).map_err(|_| RegistryStringError::InvalidUtf16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(kind: u32, units: &[u16]) -> RegistryValue {
        RegistryValue {
            native_type: kind,
            bytes: units.iter().flat_map(|u| u.to_le_bytes()).collect(),
        }
    }

    #[test]
    fn accepts_string_types_and_optional_terminator_without_expansion() {
        assert_eq!(decode_registry_string(&value(REG_SZ, &[65])).unwrap(), "A");
        assert_eq!(
            decode_registry_string(&value(REG_SZ, &[65, 0])).unwrap(),
            "A"
        );
        assert_eq!(
            decode_registry_string(&value(REG_EXPAND_SZ, &[37, 80, 37, 0])).unwrap(),
            "%P%"
        );
    }

    #[test]
    fn rejects_interior_nul_and_malformed_utf16() {
        assert_eq!(
            decode_registry_string(&value(REG_SZ, &[65, 0, 66])),
            Err(RegistryStringError::InteriorNul)
        );
        assert_eq!(
            decode_registry_string(&value(REG_SZ, &[0xd800])),
            Err(RegistryStringError::InvalidUtf16)
        );
    }

    #[test]
    fn rejects_wrong_type_odd_length_and_oversize() {
        assert_eq!(
            decode_registry_string(&value(3, &[65])),
            Err(RegistryStringError::UnsupportedType(3))
        );
        assert_eq!(
            decode_registry_string(&RegistryValue {
                native_type: REG_SZ,
                bytes: vec![65]
            }),
            Err(RegistryStringError::OddByteLength)
        );
        assert_eq!(
            decode_registry_string(&RegistryValue {
                native_type: REG_SZ,
                bytes: vec![0; MAX_REGISTRY_STRING_BYTES + 2]
            }),
            Err(RegistryStringError::TooLarge)
        );
    }

    #[test]
    fn report_round_trip_preserves_unknown_metadata_and_duplicates() {
        let app = InstalledApplication {
            subkey: "same-key".into(),
            subkey_error: None,
            display_name: Some("Example".into()),
            version: None,
            publisher: None,
            field_errors: BTreeMap::new(),
            metadata: BTreeMap::from([("VendorField".into(), serde_json::json!({"raw": 7}))]),
        };
        let report = InventoryReport {
            scopes: vec![ScopeReport {
                scope: InventoryScope {
                    hive: Hive::CurrentUser,
                    view: RegistryView::Native,
                },
                status: ScopeStatus::Complete,
                applications: vec![app.clone(), app],
            }],
        };
        let encoded = serde_json::to_string(&report).unwrap();
        let decoded: InventoryReport = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, report);
    }

    #[test]
    fn field_errors_preserve_read_error_codes_and_decode_reasons() {
        let errors = BTreeMap::from([
            (
                "DisplayName".into(),
                InstalledFieldError::RegistryReadFailed(5),
            ),
            (
                "DisplayVersion".into(),
                InstalledFieldError::InvalidString(RegistryStringError::InvalidUtf16),
            ),
        ]);
        let encoded = serde_json::to_string(&errors).unwrap();
        let decoded: BTreeMap<String, InstalledFieldError> =
            serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, errors);
    }
}
