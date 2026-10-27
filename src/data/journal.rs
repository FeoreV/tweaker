//! Versioned registry and service journal record encoding. This module does not perform I/O.
//! Integrity, durability, unique IDs and Windows-compatible path encoding must
//! be established before this format is used to authorize real mutations.

use crate::engine::registry::{ValueState, ValueTarget};
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 2;
pub const SERVICE_FORMAT_VERSION: u32 = 1;
pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryRecord {
    pub version: u32,
    pub transaction_id: String,
    pub sequence: u64,
    pub target: ValueTarget,
    pub before: ValueState,
    pub intended: ValueState,
}

/// Exact internal name of an existing local Windows service.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceTarget {
    pub name: String,
}

/// A journal record for changing only an existing service's startup type.
/// Values are kept as native SCM integers so snapshots preserve the exact
/// queried value; the Win32 backend remains responsible for API validation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceRecord {
    pub version: u32,
    pub transaction_id: String,
    pub sequence: u64,
    pub target: ServiceTarget,
    pub before_start_type: u32,
    pub intended_start_type: u32,
}

/// Discriminated record type used when scanning mixed-resource transactions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "record_type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TransactionRecord {
    Registry(RegistryRecord),
    Service(ServiceRecord),
}

impl TransactionRecord {
    pub fn transaction_id(&self) -> &str {
        match self {
            Self::Registry(record) => &record.transaction_id,
            Self::Service(record) => &record.transaction_id,
        }
    }

    pub fn sequence(&self) -> u64 {
        match self {
            Self::Registry(record) => record.sequence,
            Self::Service(record) => record.sequence,
        }
    }

    fn validate(&self) -> Result<(), RecordError> {
        match self {
            Self::Registry(record) => record.validate(),
            Self::Service(record) => record.validate(),
        }
    }
}

#[derive(Debug)]
pub enum RecordError {
    TooLarge,
    InvalidJson(serde_json::Error),
    UnsupportedVersion(u32),
    InvalidTransactionId,
    InvalidTarget,
}

impl RegistryRecord {
    fn validate(&self) -> Result<(), RecordError> {
        if self.version != FORMAT_VERSION {
            return Err(RecordError::UnsupportedVersion(self.version));
        }
        if !valid_transaction_id(&self.transaction_id) {
            return Err(RecordError::InvalidTransactionId);
        }
        if self.target.key_path.contains('\0') || self.target.value_name.contains('\0') {
            return Err(RecordError::InvalidTarget);
        }
        Ok(())
    }
}

impl ServiceRecord {
    fn validate(&self) -> Result<(), RecordError> {
        if self.version != SERVICE_FORMAT_VERSION {
            return Err(RecordError::UnsupportedVersion(self.version));
        }
        if !valid_transaction_id(&self.transaction_id) {
            return Err(RecordError::InvalidTransactionId);
        }
        if self.target.name.is_empty() || self.target.name.contains('\0') {
            return Err(RecordError::InvalidTarget);
        }
        Ok(())
    }
}

pub(crate) fn valid_transaction_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

pub fn encode(record: &RegistryRecord) -> Result<Vec<u8>, RecordError> {
    record.validate()?;
    let bytes = serde_json::to_vec(record).map_err(RecordError::InvalidJson)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(RecordError::TooLarge);
    }
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<RegistryRecord, RecordError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(RecordError::TooLarge);
    }
    let record: RegistryRecord = serde_json::from_slice(bytes).map_err(RecordError::InvalidJson)?;
    record.validate()?;
    Ok(record)
}

/// Encode the legacy untagged service record representation. Mixed-resource
/// journals use `encode_transaction_record`; neither codec authorizes service
/// mutations without the recovery and backend gates.
pub fn encode_service(record: &ServiceRecord) -> Result<Vec<u8>, RecordError> {
    record.validate()?;
    let bytes = serde_json::to_vec(record).map_err(RecordError::InvalidJson)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(RecordError::TooLarge);
    }
    Ok(bytes)
}

/// Decode and validate one standalone service start-type record.
pub fn decode_service(bytes: &[u8]) -> Result<ServiceRecord, RecordError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(RecordError::TooLarge);
    }
    let record: ServiceRecord = serde_json::from_slice(bytes).map_err(RecordError::InvalidJson)?;
    record.validate()?;
    Ok(record)
}

/// Encode a tagged registry or service record for a mixed-resource journal.
pub fn encode_transaction_record(record: &TransactionRecord) -> Result<Vec<u8>, RecordError> {
    record.validate()?;
    let bytes = serde_json::to_vec(record).map_err(RecordError::InvalidJson)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(RecordError::TooLarge);
    }
    Ok(bytes)
}

/// Decode a tagged record and legacy untagged registry records. Legacy support
/// is required to reopen existing registry journals after the format upgrade.
pub fn decode_transaction_record(bytes: &[u8]) -> Result<TransactionRecord, RecordError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(RecordError::TooLarge);
    }
    if let Ok(record) = decode(bytes) {
        return Ok(TransactionRecord::Registry(record));
    }
    let record: TransactionRecord =
        serde_json::from_slice(bytes).map_err(RecordError::InvalidJson)?;
    record.validate()?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::registry::{Hive, RegistryValue, RegistryView};

    fn record() -> RegistryRecord {
        RegistryRecord {
            version: FORMAT_VERSION,
            transaction_id: "tx_123".into(),
            sequence: 42,
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Test".into(),
                value_name: String::new(),
            },
            before: ValueState::KeyMissing,
            intended: ValueState::Present(RegistryValue {
                native_type: 7,
                bytes: vec![0, 255, 0],
            }),
        }
    }
    fn service_record() -> ServiceRecord {
        ServiceRecord {
            version: SERVICE_FORMAT_VERSION,
            transaction_id: "tx_123".into(),
            sequence: 3,
            target: ServiceTarget {
                name: "ExampleService".into(),
            },
            before_start_type: 3,
            intended_start_type: 4,
        }
    }
    #[test]
    fn round_trip_preserves_absence_type_bytes_and_view() {
        let record = record();
        assert_eq!(decode(&encode(&record).unwrap()).unwrap(), record);
    }
    #[test]
    fn key_missing_value_missing_and_empty_are_distinct_in_journal() {
        let states = [
            ValueState::KeyMissing,
            ValueState::ValueMissing,
            ValueState::Present(RegistryValue {
                native_type: 3,
                bytes: vec![],
            }),
        ];
        for state in states {
            let mut entry = record();
            entry.before = state.clone();
            assert_eq!(decode(&encode(&entry).unwrap()).unwrap().before, state);
        }
    }
    #[test]
    fn rejects_truncation_version_and_trailing_data() {
        let data = encode(&record()).unwrap();
        for end in 0..data.len() {
            assert!(decode(&data[..end]).is_err());
        }
        let mut version = record();
        version.version += 1;
        assert!(matches!(
            encode(&version),
            Err(RecordError::UnsupportedVersion(_))
        ));
        let mut trailing = data;
        trailing.extend_from_slice(b"{}");
        assert!(decode(&trailing).is_err());
    }
    #[test]
    fn rejects_unknown_fields_invalid_ids_and_oversized_input() {
        let mut value = serde_json::to_value(record()).unwrap();
        value["extra"] = serde_json::json!(true);
        assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut bad = record();
        bad.transaction_id = "../bad".into();
        assert!(matches!(
            encode(&bad),
            Err(RecordError::InvalidTransactionId)
        ));
        assert!(matches!(
            decode(&vec![b' '; MAX_RECORD_BYTES + 1]),
            Err(RecordError::TooLarge)
        ));
    }

    #[test]
    fn service_start_type_record_round_trip_preserves_identity_and_values() {
        let record = service_record();
        assert_eq!(
            decode_service(&encode_service(&record).unwrap()).unwrap(),
            record
        );
    }

    #[test]
    fn service_record_rejects_unsupported_versions_invalid_names_and_unknown_fields() {
        let mut bad = service_record();
        bad.version += 1;
        assert!(matches!(
            encode_service(&bad),
            Err(RecordError::UnsupportedVersion(_))
        ));
        bad = service_record();
        bad.target.name.clear();
        assert!(matches!(
            encode_service(&bad),
            Err(RecordError::InvalidTarget)
        ));
        bad.target.name = "bad\0service".into();
        assert!(matches!(
            encode_service(&bad),
            Err(RecordError::InvalidTarget)
        ));
        let mut json: serde_json::Value =
            serde_json::from_slice(&encode_service(&service_record()).unwrap()).unwrap();
        json["unexpected"] = serde_json::json!(true);
        assert!(decode_service(&serde_json::to_vec(&json).unwrap()).is_err());
        assert!(decode_service(&encode(&record()).unwrap()).is_err());
        assert!(decode(&encode_service(&service_record()).unwrap()).is_err());
        bad = service_record();
        bad.transaction_id = "../bad".into();
        assert!(matches!(
            encode_service(&bad),
            Err(RecordError::InvalidTransactionId)
        ));
    }

    #[test]
    fn tagged_transaction_records_round_trip_both_kinds_and_read_legacy_registry_records() {
        let registry = TransactionRecord::Registry(record());
        let service = TransactionRecord::Service(service_record());
        assert_eq!(
            decode_transaction_record(&encode_transaction_record(&registry).unwrap()).unwrap(),
            registry
        );
        assert_eq!(
            decode_transaction_record(&encode_transaction_record(&service).unwrap()).unwrap(),
            service
        );
        assert_eq!(
            decode_transaction_record(&encode(&record()).unwrap()).unwrap(),
            TransactionRecord::Registry(record())
        );
    }

    #[test]
    fn tagged_transaction_record_rejects_unknown_envelope_fields() {
        let record = TransactionRecord::Service(service_record());
        let mut json: serde_json::Value =
            serde_json::from_slice(&encode_transaction_record(&record).unwrap()).unwrap();
        json["unrecognized"] = serde_json::json!(true);
        assert!(decode_transaction_record(&serde_json::to_vec(&json).unwrap()).is_err());
    }
}
