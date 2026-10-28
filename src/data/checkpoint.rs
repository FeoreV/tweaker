//! Checkpoint encoding for recovery. CRC detects accidental corruption only;
//! it is not authentication, durable storage, or a lock.
use super::journal::{
    encode_existing_transaction_record, valid_transaction_id, RecordError, RegistryRecord,
    TransactionRecord,
};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 4;
pub const MAX_CHECKPOINT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    version: u32,
    transaction_id: String,
    recovery_transaction_id: String,
    next_exclusive: u64,
    records: Vec<TransactionRecord>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: Checkpoint,
    crc32: u32,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyCheckpointV3 {
    version: u32,
    transaction_id: String,
    recovery_transaction_id: String,
    next_exclusive: u64,
    records: Vec<RegistryRecord>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyEnvelopeV3 {
    payload: LegacyCheckpointV3,
    crc32: u32,
}
#[derive(Debug)]
pub enum CheckpointError {
    InvalidRecords,
    Record(RecordError),
    InvalidCheckpoint,
    Corrupt,
    Json(serde_json::Error),
    TooLarge,
}

fn validate_records(records: &[TransactionRecord]) -> Result<(), CheckpointError> {
    let first = records.first().ok_or(CheckpointError::InvalidRecords)?;
    for (index, record) in records.iter().enumerate() {
        if record.sequence() != index as u64 || record.transaction_id() != first.transaction_id() {
            return Err(CheckpointError::InvalidRecords);
        }
        encode_existing_transaction_record(record).map_err(CheckpointError::Record)?;
    }
    Ok(())
}

// CRC-32/ISO-HDLC, for accidental damage detection only.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}
impl Checkpoint {
    pub fn new(
        records: &[RegistryRecord],
        recovery_transaction_id: String,
        next_exclusive: usize,
    ) -> Result<Self, CheckpointError> {
        let typed: Vec<_> = records
            .iter()
            .cloned()
            .map(TransactionRecord::Registry)
            .collect();
        Self::new_typed(&typed, recovery_transaction_id, next_exclusive)
    }

    pub fn new_typed(
        records: &[TransactionRecord],
        recovery_transaction_id: String,
        next_exclusive: usize,
    ) -> Result<Self, CheckpointError> {
        validate_records(records)?;
        if next_exclusive > records.len()
            || !valid_transaction_id(&recovery_transaction_id)
            || recovery_transaction_id == records[0].transaction_id()
        {
            return Err(CheckpointError::InvalidCheckpoint);
        }
        Ok(Self {
            version: VERSION,
            transaction_id: records[0].transaction_id().to_owned(),
            recovery_transaction_id,
            next_exclusive: next_exclusive as u64,
            records: records.to_vec(),
        })
    }
    pub fn validate(&self, records: &[RegistryRecord]) -> Result<usize, CheckpointError> {
        let typed: Vec<_> = records
            .iter()
            .cloned()
            .map(TransactionRecord::Registry)
            .collect();
        self.validate_typed(&typed)
    }

    pub fn validate_typed(&self, records: &[TransactionRecord]) -> Result<usize, CheckpointError> {
        validate_records(records)?;
        if self.version != VERSION
            || self.transaction_id != records[0].transaction_id()
            || !valid_transaction_id(&self.recovery_transaction_id)
            || self.recovery_transaction_id == self.transaction_id
            || self.next_exclusive > records.len() as u64
            || self.records != records
        {
            return Err(CheckpointError::InvalidCheckpoint);
        }
        usize::try_from(self.next_exclusive).map_err(|_| CheckpointError::InvalidCheckpoint)
    }

    pub fn recovery_transaction_id(&self) -> &str {
        &self.recovery_transaction_id
    }
}
pub fn encode(checkpoint: &Checkpoint) -> Result<Vec<u8>, CheckpointError> {
    // Refuse invalid checkpoints even if they were decoded from untrusted input.
    checkpoint.validate_typed(&checkpoint.records)?;
    let payload = serde_json::to_vec(checkpoint).map_err(CheckpointError::Json)?;
    if payload.len() > MAX_CHECKPOINT_BYTES {
        return Err(CheckpointError::TooLarge);
    }
    let envelope = serde_json::json!({"payload": checkpoint, "crc32": crc32(&payload)});
    let bytes = serde_json::to_vec(&envelope).map_err(CheckpointError::Json)?;
    if bytes.len() > MAX_CHECKPOINT_BYTES {
        return Err(CheckpointError::TooLarge);
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<Checkpoint, CheckpointError> {
    if bytes.len() > MAX_CHECKPOINT_BYTES {
        return Err(CheckpointError::TooLarge);
    }
    let envelope: Envelope = match serde_json::from_slice(bytes) {
        Ok(envelope) => envelope,
        Err(typed_error) => {
            let legacy: LegacyEnvelopeV3 =
                serde_json::from_slice(bytes).map_err(|_| CheckpointError::Json(typed_error))?;
            let payload = serde_json::to_vec(&legacy.payload).map_err(CheckpointError::Json)?;
            if crc32(&payload) != legacy.crc32 || legacy.payload.version != 3 {
                return Err(CheckpointError::Corrupt);
            }
            let records: Vec<_> = legacy
                .payload
                .records
                .into_iter()
                .map(TransactionRecord::Registry)
                .collect();
            let checkpoint = Checkpoint {
                version: VERSION,
                transaction_id: legacy.payload.transaction_id,
                recovery_transaction_id: legacy.payload.recovery_transaction_id,
                next_exclusive: legacy.payload.next_exclusive,
                records,
            };
            checkpoint.validate_typed(&checkpoint.records)?;
            return Ok(checkpoint);
        }
    };
    let payload = serde_json::to_vec(&envelope.payload).map_err(CheckpointError::Json)?;
    if crc32(&payload) != envelope.crc32 {
        return Err(CheckpointError::Corrupt);
    }
    envelope.payload.validate_typed(&envelope.payload.records)?;
    Ok(envelope.payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::journal::{ServiceRecord, ServiceTarget, SERVICE_FORMAT_VERSION};
    use crate::engine::registry::{Hive, RegistryValue, RegistryView, ValueState, ValueTarget};
    fn record(sequence: u64) -> RegistryRecord {
        RegistryRecord {
            version: super::super::journal::FORMAT_VERSION,
            transaction_id: "tx".into(),
            sequence,
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "v".into(),
            },
            before: ValueState::KeyMissing,
            intended: ValueState::Present(RegistryValue {
                native_type: 4,
                bytes: vec![sequence as u8],
            }),
        }
    }
    #[test]
    fn checkpoint_round_trips_previously_accepted_service_record() {
        let bytes = br#"{"record_type":"service","payload":{"version":1,"transaction_id":"tx","sequence":0,"target":{"name":"old service"},"before_start_type":5,"intended_start_type":3}}"#;
        let record = super::super::journal::decode_transaction_record(bytes).unwrap();
        let checkpoint =
            Checkpoint::new_typed(std::slice::from_ref(&record), "recovery".into(), 1).unwrap();
        assert_eq!(
            decode(&encode(&checkpoint).unwrap())
                .unwrap()
                .validate_typed(&[record])
                .unwrap(),
            1
        );
    }

    #[test]
    fn crc_standard_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
    #[test]
    fn round_trip_and_bind_to_exact_ordered_records() {
        let records = vec![record(0), record(1)];
        let restored =
            decode(&encode(&Checkpoint::new(&records, "undo_tx".into(), 1).unwrap()).unwrap())
                .unwrap();
        assert_eq!(restored.validate(&records).unwrap(), 1);
        assert_eq!(restored.recovery_transaction_id(), "undo_tx");
        let mut changed = records.clone();
        changed[0].target.value_name = "other".into();
        assert!(matches!(
            restored.validate(&changed),
            Err(CheckpointError::InvalidCheckpoint)
        ));
        changed.swap(0, 1);
        assert!(restored.validate(&changed).is_err());
    }
    #[test]
    fn rejects_changed_progress_without_matching_crc() {
        let bytes = encode(&Checkpoint::new(&[record(0)], "undo_tx".into(), 1).unwrap()).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["payload"]["next_exclusive"] = serde_json::json!(0);
        assert!(matches!(
            decode(&serde_json::to_vec(&value).unwrap()),
            Err(CheckpointError::Corrupt)
        ));
    }
    #[test]
    fn reject_wrong_transaction_sequence_and_progress() {
        let mut records = vec![record(0), record(1)];
        assert!(Checkpoint::new(&records, "undo_tx".into(), 3).is_err());
        records[1].transaction_id = "different".into();
        assert!(matches!(
            Checkpoint::new(&records, "undo_tx".into(), 1),
            Err(CheckpointError::InvalidRecords)
        ));
        records[1].transaction_id = "tx".into();
        records[1].sequence = 7;
        assert!(matches!(
            Checkpoint::new(&records, "undo_tx".into(), 1),
            Err(CheckpointError::InvalidRecords)
        ));
    }
    #[test]
    fn reject_truncated_and_oversized_checkpoint() {
        let bytes = encode(&Checkpoint::new(&[record(0)], "undo_tx".into(), 1).unwrap()).unwrap();
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err());
        }
        assert!(matches!(
            decode(&vec![0; MAX_CHECKPOINT_BYTES + 1]),
            Err(CheckpointError::TooLarge)
        ));
    }

    #[test]
    fn rejects_invalid_or_source_transaction_as_recovery_identity() {
        let records = [record(0)];
        assert!(Checkpoint::new(&records, "../bad".into(), 1).is_err());
        assert!(Checkpoint::new(&records, "tx".into(), 1).is_err());
    }

    #[test]
    fn typed_checkpoint_round_trips_registry_and_service_records() {
        let records = vec![
            TransactionRecord::Registry(record(0)),
            TransactionRecord::Service(ServiceRecord {
                version: SERVICE_FORMAT_VERSION,
                transaction_id: "tx".into(),
                sequence: 1,
                target: ServiceTarget {
                    name: "ExampleService".into(),
                },
                before_start_type: 3,
                intended_start_type: 4,
            }),
        ];
        let checkpoint = Checkpoint::new_typed(&records, "undo_tx".into(), 1).unwrap();
        let restored = decode(&encode(&checkpoint).unwrap()).unwrap();
        assert_eq!(restored.validate_typed(&records).unwrap(), 1);
        assert_eq!(restored.records, records);
    }

    #[test]
    fn v3_registry_checkpoint_remains_readable() {
        let legacy = LegacyCheckpointV3 {
            version: 3,
            transaction_id: "tx".into(),
            recovery_transaction_id: "undo_tx".into(),
            next_exclusive: 1,
            records: vec![record(0)],
        };
        let payload = serde_json::to_vec(&legacy).unwrap();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "payload": legacy,
            "crc32": crc32(&payload),
        }))
        .unwrap();

        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.validate(&[record(0)]).unwrap(), 1);
        assert_eq!(decoded.version, VERSION);
    }
}
