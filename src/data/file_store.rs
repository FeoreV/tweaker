//! Windows filesystem adapter for the registry journal.
//!
//! This module supplies exclusive record creation, explicit file flushing,
//! bounded rereads, staged commit-marker publication, and per-transaction
//! validation after restart. Append-only recovery checkpoint generations are bound to the exact
//! committed prefix and share the transaction lock. Its locked adapter holds
//! an OS-level exclusive transaction handle, and the root can enumerate IDs
//! from file names. Root ACL and ancestor checks are fail-closed, but external
//! privileged namespace changes and power-loss guarantees remain unresolved.

use super::checkpoint::{self, Checkpoint, MAX_CHECKPOINT_BYTES};
use super::journal::{
    decode_transaction_record, valid_transaction_id, RegistryRecord, TransactionRecord,
    MAX_RECORD_BYTES,
};
use super::store::{
    JournalError, JournalScan, JournalStorage, RegistryJournal, TypedJournal, TypedState,
    TypedTarget,
};
use crate::engine::recovery::{
    BatchRecoveryError, JournalRecoveryError, JournalRecoveryResult, RecoveryBackend,
    RecoveryCheckpoint, RecoveryError,
};
use crate::engine::registry::{ValueState, ValueTarget};
use crate::engine::transaction::{Backend, Operation, TransactionError};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::File;
#[cfg(test)]
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
#[cfg(test)]
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Wdk::Storage::FileSystem::{
    FileIdBothDirectoryInformation, FileRenameInformation, NtQueryDirectoryFile,
    NtSetInformationFile, FILE_CREATE, FILE_ID_BOTH_DIR_INFORMATION, FILE_NON_DIRECTORY_FILE,
    FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION, FILE_RENAME_INFORMATION_0,
};
use windows_sys::Win32::Foundation::{
    RtlNtStatusToDosError, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION, HANDLE,
    STATUS_NO_MORE_FILES,
};
use windows_sys::Win32::Storage::FileSystem::{
    FileAttributeTagInfo, GetFileInformationByHandleEx, DELETE, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

const COMMIT_MARKER_VERSION: u32 = 1;
const MAX_TRANSACTION_RECORDS: usize = 4096;
const MAX_TRANSACTION_BYTES: usize = 64 * 1024 * 1024;
const MAX_COMMIT_MARKER_BYTES: usize = 1024;
const MAX_CHECKPOINT_GENERATIONS: usize = MAX_TRANSACTION_RECORDS + 1;
const MAX_RECOVERY_ID_ATTEMPTS: usize = 32;
const REGISTRY_MUTATION_LOCK_FILE: &str = ".registry-mutations.lock";
const TRANSACTION_DISCOVERY_LOCK_FILE: &str = ".transaction-discovery.lock";

/// A validated candidate fits an empty journal but not this committed prefix.
#[derive(Debug)]
struct SnapshotCapacity;

impl std::fmt::Display for SnapshotCapacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("journal capacity exhausted")
    }
}
impl std::error::Error for SnapshotCapacity {}

fn capacity_error() -> io::Error {
    io::Error::other(SnapshotCapacity)
}

fn is_snapshot_capacity<B>(
    result: &JournalRecoveryResult<B, JournalError<io::Error>, io::Error>,
) -> bool {
    matches!(result, Err(JournalRecoveryError::Recovery(BatchRecoveryError::Operation {
        error: RecoveryError::Persist(JournalError::Preflight(error)), ..
    })) if error.get_ref().is_some_and(|source| source.is::<SnapshotCapacity>()))
}

static NEXT_RECOVERY_ID: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IoBoundary {
    Preflight,
    RecordCreate,
    RecordWrite,
    RecordFlush,
    RecordSyncFlush,
    RecordReadback,
    CommitMarkerCreate,
    CommitMarkerWrite,
    CommitMarkerFlush,
    CommitMarkerReadback,
    CommitMarkerPublish,
    CommitMarkerFinalReadback,
    CheckpointCreate,
    CheckpointWrite,
    CheckpointFlush,
    CheckpointReadback,
    CheckpointMarkerCreate,
    CheckpointMarkerWrite,
    CheckpointMarkerFlush,
    CheckpointMarkerReadback,
    CheckpointMarkerPublish,
    CheckpointMarkerFinalReadback,
}

#[cfg(test)]
trait IoFaultInjector: std::fmt::Debug + Send + Sync {
    fn fail_at(&self, boundary: IoBoundary) -> bool;
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CommitMarker {
    version: u32,
    transaction_id: String,
    sequence: u64,
    record_crc32: u32,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CheckpointMarker {
    version: u32,
    transaction_id: String,
    generation: u64,
    checkpoint_crc32: u32,
}

/// Persistent registry record storage rooted at a protected local directory.
#[derive(Debug)]
pub struct FileJournalStorage {
    root: PathBuf,
    _path_guards: Vec<File>,
    #[cfg(test)]
    fault_injector: Option<Arc<dyn IoFaultInjector>>,
    #[cfg(test)]
    preflight_limits: Option<(usize, usize)>,
}

/// Exclusive per-transaction lock. Dropping the guard closes its Windows file
/// handle and releases the lock, including during normal error unwinding.
#[derive(Debug)]
struct TransactionLock {
    _file: File,
}

/// Adapter that owns the exclusive lock for one journal transaction.
/// Construct it through [`FileJournalStorage::lock_transaction`].
#[derive(Debug)]
pub struct LockedJournalStorage {
    storage: FileJournalStorage,
    transaction: String,
    _lock: TransactionLock,
}

/// Exclusive root-wide guard for cooperating system mutation/recovery
/// sequences. It serializes registry aliases and can also be shared by service
/// writers. Keep it alive from the first query through final verification.
#[derive(Debug)]
pub struct RegistryMutationLock {
    _lock: TransactionLock,
}

/// A disk-backed apply session. It owns its journal lock and acquires the
/// registry barrier around each complete query -> persist -> apply operation.
pub struct FileTransactionSession {
    root_storage: FileJournalStorage,
    transaction: String,
    journal: RegistryJournal<LockedJournalStorage>,
}

/// Managed apply session for mixed registry/service operations. It owns the
/// transaction lock and takes the shared barrier for each whole operation.
pub struct FileTypedTransactionSession {
    root_storage: FileJournalStorage,
    transaction: String,
    journal: TypedJournal<LockedJournalStorage>,
}

#[derive(Debug)]
pub enum FileTransactionError<B, S> {
    RegistryLock(io::Error),
    Operation(TransactionError<B, S>),
}

#[derive(Debug)]
pub enum FileTypedTransactionError<B, S> {
    MutationLock(io::Error),
    Operation(TransactionError<B, S>),
}

impl FileJournalStorage {
    /// Create a new root with a protected SYSTEM/OWNER RIGHTS DACL.
    ///
    /// The requested leaf must not exist. Every ancestor is checked before
    /// creation; existing ACLs are never repaired or modified.
    pub fn create_private(root: impl AsRef<Path>) -> io::Result<Self> {
        let (root, path_guards) = super::windows_security::create_private_root(root.as_ref())?;
        Ok(Self {
            root,
            _path_guards: path_guards,
            #[cfg(test)]
            fault_injector: None,
            #[cfg(test)]
            preflight_limits: None,
        })
    }

    /// Open an existing root only when its owner, protected DACL, and ancestor
    /// ACLs meet the journal storage security policy.
    pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
        let (root, path_guards) = super::windows_security::open_private_root(root.as_ref())?;
        Ok(Self {
            root,
            _path_guards: path_guards,
            #[cfg(test)]
            fault_injector: None,
            #[cfg(test)]
            preflight_limits: None,
        })
    }

    /// Duplicate the retained namespace handles without resolving `root` again.
    /// File::try_clone duplicates the underlying kernel handles; each resulting
    /// RAII owner closes only its duplicate, while both remain bound to the
    /// exact same directory objects even if their display paths are renamed.
    fn try_clone(&self) -> io::Result<Self> {
        let path_guards = self
            ._path_guards
            .iter()
            .map(File::try_clone)
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            root: self.root.clone(),
            _path_guards: path_guards,
            #[cfg(test)]
            fault_injector: self.fault_injector.clone(),
            #[cfg(test)]
            preflight_limits: self.preflight_limits,
        })
    }

    #[cfg(test)]
    fn open_with_fault_injector(
        root: impl AsRef<Path>,
        fault_injector: Arc<dyn IoFaultInjector>,
    ) -> io::Result<Self> {
        let mut storage = Self::open(root)?;
        storage.fault_injector = Some(fault_injector);
        Ok(storage)
    }

    #[cfg(test)]
    fn fail_at(&self, boundary: IoBoundary) -> io::Result<()> {
        if self
            .fault_injector
            .as_ref()
            .is_some_and(|injector| injector.fail_at(boundary))
        {
            return Err(io::Error::other(format!(
                "injected I/O failure at {boundary:?}"
            )));
        }
        Ok(())
    }

    fn root_directory(&self) -> &File {
        self._path_guards
            .last()
            .expect("validated root handle retained")
    }

    fn open_child(
        &self,
        name: &std::ffi::OsStr,
        access: u32,
        share: u32,
        disposition: u32,
        options: u32,
    ) -> io::Result<File> {
        let file = super::windows_security::open_relative_file(
            self.root_directory(),
            name,
            access | FILE_READ_ATTRIBUTES,
            share,
            disposition,
            options | FILE_OPEN_REPARSE_POINT | FILE_NON_DIRECTORY_FILE,
            std::ptr::null(),
        )?;
        let mut tag = FILE_ATTRIBUTE_TAG_INFO {
            FileAttributes: 0,
            ReparseTag: 0,
        };
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle() as HANDLE,
                FileAttributeTagInfo,
                (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
            || tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal child is not a regular non-reparse file",
            ));
        }
        Ok(file)
    }

    fn enumerate_root(&self) -> io::Result<Vec<(OsString, u32)>> {
        enumerate_directory(self.root_directory())
    }

    fn read_limited(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid journal filename")
        })?;
        let file = self.open_child(
            name,
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            0,
        )?;
        if file.metadata()?.len() > limit as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal file exceeds its size limit",
            ));
        }
        let mut bytes = Vec::new();
        file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal file grew beyond its size limit while reading",
            ));
        }
        Ok(bytes)
    }

    fn publish_marker(&self, pending: &Path, committed: &Path) -> io::Result<()> {
        let source = pending.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid pending marker filename",
            )
        })?;
        let destination = committed.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid committed marker filename",
            )
        })?;
        let name: Vec<u16> = destination.encode_wide().collect();
        if name.is_empty() || name.len() > (u32::MAX as usize / 2) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid marker destination",
            ));
        }
        let file = self.open_child(
            source,
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            0,
        )?;
        let name_offset = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
        let byte_len = name_offset
            .checked_add(name.len() * 2)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "marker name too long"))?;
        let mut storage = vec![0u64; byte_len.div_ceil(std::mem::size_of::<u64>())];
        let info = FILE_RENAME_INFORMATION {
            Anonymous: FILE_RENAME_INFORMATION_0 { ReplaceIfExists: 0 },
            RootDirectory: self.root_directory().as_raw_handle() as HANDLE,
            FileNameLength: (name.len() * 2) as u32,
            FileName: [0],
        };
        unsafe {
            let base = storage.as_mut_ptr().cast::<u8>();
            std::ptr::write(base.cast::<FILE_RENAME_INFORMATION>(), info);
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                base.add(name_offset).cast::<u16>(),
                name.len(),
            );
        }
        let mut io_status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        let status = unsafe {
            NtSetInformationFile(
                file.as_raw_handle() as HANDLE,
                &mut io_status,
                storage.as_ptr().cast(),
                byte_len as u32,
                FileRenameInformation,
            )
        };
        if status < 0 {
            let code = unsafe { RtlNtStatusToDosError(status) };
            Err(io::Error::from_raw_os_error(code as i32))
        } else {
            Ok(())
        }
    }

    /// List transaction IDs represented by lock, record, or commit-marker files.
    /// The root discovery lock serializes this enumeration with creation of a
    /// transaction lock file, so a transaction ID cannot appear halfway through
    /// the scan. The caller must still acquire each transaction lock before
    /// scanning its log.
    pub fn discover_transactions(&self) -> io::Result<Vec<String>> {
        let _discovery_lock = self.lock_transaction_discovery()?;
        let mut transactions = BTreeSet::new();
        for (name, attributes) in self.enumerate_root()? {
            if attributes & FILE_ATTRIBUTE_DIRECTORY != 0
                || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal root contains a non-regular entry",
                ));
            }
            if name == REGISTRY_MUTATION_LOCK_FILE || name == TRANSACTION_DISCOVERY_LOCK_FILE {
                continue;
            }
            let name = name.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal filename is not Unicode",
                )
            })?;
            transactions.insert(parse_transaction_filename(name)?);
        }
        Ok(transactions.into_iter().collect())
    }

    fn record_path(&self, transaction: &str, sequence: u64) -> io::Result<PathBuf> {
        if !valid_transaction_id(transaction) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid journal transaction ID",
            ));
        }
        Ok(self.root.join(format!(
            "{:03}-{transaction}-{sequence:020}.json",
            transaction.len()
        )))
    }

    fn commit_path(&self, transaction: &str, sequence: u64) -> io::Result<PathBuf> {
        let record = self.record_path(transaction, sequence)?;
        let name = record
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid journal path"))?
            .to_os_string();
        let mut name = name.to_string_lossy().into_owned();
        name.truncate(name.len() - ".json".len());
        name.push_str(".commit");
        Ok(self.root.join(name))
    }

    fn commit_pending_path(&self, transaction: &str, sequence: u64) -> io::Result<PathBuf> {
        let mut path = self.commit_path(transaction, sequence)?;
        path.set_extension("commit.pending");
        Ok(path)
    }

    fn lock_path(&self, transaction: &str) -> io::Result<PathBuf> {
        let _ = self.record_path(transaction, 0)?;
        Ok(self
            .root
            .join(format!("{:03}-{transaction}.lock", transaction.len())))
    }

    fn checkpoint_path(&self, transaction: &str, generation: u64) -> io::Result<PathBuf> {
        let _ = self.record_path(transaction, 0)?;
        Ok(self.root.join(format!(
            "{:03}-{transaction}-recovery-{generation:020}.json",
            transaction.len()
        )))
    }

    fn checkpoint_marker_path(&self, transaction: &str, generation: u64) -> io::Result<PathBuf> {
        let path = self.checkpoint_path(transaction, generation)?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid checkpoint path"))?
            .to_string_lossy();
        let mut name = name.into_owned();
        name.truncate(name.len() - ".json".len());
        name.push_str(".commit");
        Ok(self.root.join(name))
    }

    fn checkpoint_marker_pending_path(
        &self,
        transaction: &str,
        generation: u64,
    ) -> io::Result<PathBuf> {
        let mut path = self.checkpoint_marker_path(transaction, generation)?;
        path.set_extension("commit.pending");
        Ok(path)
    }

    /// Hold an exclusive, OS-managed lock for a transaction.
    ///
    /// Keep the returned guard alive across journal writes, target mutations,
    /// and recovery. The lock file may remain after a process exits; the open
    /// handle, not the file's existence, represents ownership.
    fn acquire_transaction_lock(&self, transaction: &str) -> io::Result<TransactionLock> {
        let path = self.lock_path(transaction)?;
        self.acquire_exclusive_file_lock(&path)
    }

    fn acquire_exclusive_file_lock(&self, path: &Path) -> io::Result<TransactionLock> {
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid lock filename"))?;
        let file = match self.open_child(name, FILE_GENERIC_WRITE, 0, FILE_CREATE, 0) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => self
                .open_child(name, FILE_GENERIC_WRITE, 0, FILE_OPEN, 0)
                .map_err(|error| {
                    if matches!(
                        error.raw_os_error().map(|code| code as u32),
                        Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION)
                    ) {
                        io::Error::new(io::ErrorKind::WouldBlock, "transaction is locked")
                    } else {
                        error
                    }
                })?,
            Err(error) => return Err(error),
        };
        Ok(TransactionLock { _file: file })
    }

    fn lock_transaction_discovery(&self) -> io::Result<TransactionLock> {
        self.acquire_exclusive_file_lock(&self.root.join(TRANSACTION_DISCOVERY_LOCK_FILE))
    }

    fn create_transaction_lock(&self, transaction: &str) -> io::Result<TransactionLock> {
        let path = self.lock_path(transaction)?;
        if !self.sequences_for(transaction, ".json")?.is_empty()
            || !self.sequences_for(transaction, ".commit")?.is_empty()
            || !self
                .checkpoint_sequences_for(transaction, ".json")?
                .is_empty()
            || !self
                .checkpoint_sequences_for(transaction, ".commit")?
                .is_empty()
            || !self
                .sequences_for(transaction, ".commit.pending")?
                .is_empty()
            || !self
                .checkpoint_sequences_for(transaction, ".commit.pending")?
                .is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "transaction ID already has journal data",
            ));
        }
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid transaction lock filename",
            )
        })?;
        let file = self.open_child(name, FILE_GENERIC_WRITE, 0, FILE_CREATE, 0)?;
        Ok(TransactionLock { _file: file })
    }

    /// Return a journal adapter that holds the exclusive transaction lock.
    /// The lock remains held while the adapter or its `RegistryJournal` is alive.
    pub fn lock_transaction(
        self,
        transaction: impl Into<String>,
    ) -> io::Result<LockedJournalStorage> {
        let transaction = transaction.into();
        let discovery_lock = self.lock_transaction_discovery()?;
        let lock = self.acquire_transaction_lock(&transaction)?;
        drop(discovery_lock);
        Ok(LockedJournalStorage {
            storage: self,
            transaction,
            _lock: lock,
        })
    }

    /// Create and exclusively reserve a transaction ID that has never existed
    /// in this root. Use this for a new recovery snapshot journal; reopen an
    /// existing ID with [`Self::lock_transaction`] and `RegistryJournal::resume`.
    pub fn create_new_transaction(
        self,
        transaction: impl Into<String>,
    ) -> io::Result<LockedJournalStorage> {
        let transaction = transaction.into();
        let discovery_lock = self.lock_transaction_discovery()?;
        let lock = self.create_transaction_lock(&transaction)?;
        drop(discovery_lock);
        Ok(LockedJournalStorage {
            storage: self,
            transaction,
            _lock: lock,
        })
    }

    /// Acquire the shared root-wide mutation barrier. Hold it across querying a
    /// target, persisting its backup, applying/restoring it, and verification.
    /// Its single root-wide scope covers registry aliases and cooperating
    /// resource kinds without relying on lossy target hashing.
    pub fn lock_registry_mutations(&self) -> io::Result<RegistryMutationLock> {
        self.lock_system_mutations()
    }

    /// Shared name for the root-wide barrier used by all managed resource
    /// mutation sessions. The historical registry method remains an alias.
    pub fn lock_system_mutations(&self) -> io::Result<RegistryMutationLock> {
        let path = self.root.join(REGISTRY_MUTATION_LOCK_FILE);
        let lock = self.acquire_exclusive_file_lock(&path)?;
        Ok(RegistryMutationLock { _lock: lock })
    }

    /// Start a new persistent apply transaction. The transaction lock is held
    /// for the session; each operation also obtains the registry barrier.
    pub fn start_transaction(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
    ) -> io::Result<FileTransactionSession> {
        FileTransactionSession::start(root, transaction)
    }

    /// Start a persistent typed apply transaction. Every operation is
    /// serialized through the shared mutation barrier and has a typed backup.
    pub fn start_typed_transaction(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
    ) -> io::Result<FileTypedTransactionSession> {
        FileTypedTransactionSession::start(root, transaction)
    }

    fn sequences_for(&self, transaction: &str, extension: &str) -> io::Result<BTreeSet<u64>> {
        // Length-prefixing makes this transaction's filename prefix unique,
        // even when another ID starts with the same characters.
        let prefix = format!("{:03}-{transaction}-", transaction.len());
        let mut sequences = BTreeSet::new();
        for (entry, attributes) in self.enumerate_root()? {
            let Some(name) = entry.to_str() else { continue };
            if !name.starts_with(&prefix) || !name.ends_with(extension) {
                continue;
            }
            let tail = &name[prefix.len()..];
            if tail.starts_with("recovery-") {
                continue;
            }
            if tail.len() != 20 + extension.len() || !tail.ends_with(extension) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed journal filename",
                ));
            }
            let digits = &tail[..20];
            if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed journal sequence filename",
                ));
            }
            let sequence = digits.parse::<u64>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal sequence is out of range",
                )
            })?;
            if attributes & FILE_ATTRIBUTE_DIRECTORY != 0
                || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal entry is not a regular file",
                ));
            }
            if !sequences.insert(sequence) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate journal sequence",
                ));
            }
        }
        Ok(sequences)
    }

    fn checkpoint_sequences_for(
        &self,
        transaction: &str,
        extension: &str,
    ) -> io::Result<BTreeSet<u64>> {
        let prefix = format!("{:03}-{transaction}-recovery-", transaction.len());
        let mut sequences = BTreeSet::new();
        for (entry, attributes) in self.enumerate_root()? {
            let Some(name) = entry.to_str() else { continue };
            if !name.starts_with(&prefix) || !name.ends_with(extension) {
                continue;
            }
            let tail = &name[prefix.len()..];
            if tail.len() != 20 + extension.len() || !tail.ends_with(extension) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed recovery checkpoint filename",
                ));
            }
            let digits = &tail[..20];
            if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed recovery checkpoint generation",
                ));
            }
            let generation = digits.parse::<u64>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "recovery checkpoint generation is out of range",
                )
            })?;
            if attributes & FILE_ATTRIBUTE_DIRECTORY != 0
                || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "recovery checkpoint is not a regular file",
                ));
            }
            if !sequences.insert(generation) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate recovery checkpoint generation",
                ));
            }
        }
        Ok(sequences)
    }

    fn open_for_sync(&self, transaction: &str, sequence: u64) -> io::Result<File> {
        let path = self.record_path(transaction, sequence)?;
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid record filename")
        })?;
        self.open_child(
            name,
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            0,
        )
    }
}

impl JournalStorage for LockedJournalStorage {
    type Error = io::Error;

    fn preflight(&mut self, transaction: &str, sequence: u64, data: &[u8]) -> io::Result<()> {
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::Preflight)?;
        #[cfg(test)]
        if let Some((transaction_limit, checkpoint_limit)) = self.storage.preflight_limits {
            return self.preflight_with_limits(
                transaction,
                sequence,
                data,
                transaction_limit,
                checkpoint_limit,
            );
        }
        self.preflight_with_limits(
            transaction,
            sequence,
            data,
            MAX_TRANSACTION_BYTES,
            MAX_CHECKPOINT_BYTES,
        )
    }

    fn create_new(
        &mut self,
        transaction: &str,
        sequence: u64,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.check_transaction(transaction)?;
        if data.len() > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal record exceeds the format limit",
            ));
        }
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::RecordCreate)?;
        let path = self.storage.record_path(transaction, sequence)?;
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid record filename")
        })?;
        let mut file = self.storage.open_child(
            name,
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            0,
        )?;
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::RecordWrite)?;
        file.write_all(data)?;
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::RecordFlush)?;
        file.sync_all()
    }

    fn sync(&mut self, transaction: &str, sequence: u64) -> Result<(), Self::Error> {
        self.check_transaction(transaction)?;
        if sequence >= MAX_TRANSACTION_RECORDS as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "transaction has too many committed records",
            ));
        }
        let committed = self.storage.sequences_for(transaction, ".commit")?;
        if committed.len() != sequence as usize
            || committed
                .iter()
                .enumerate()
                .any(|(expected, actual)| *actual != expected as u64)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal records must be committed in sequence order",
            ));
        }
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::RecordSyncFlush)?;
        let sync_file = self.storage.open_for_sync(transaction, sequence)?;
        sync_file.sync_all()?;

        #[cfg(test)]
        self.storage.fail_at(IoBoundary::RecordReadback)?;
        let record_bytes = self.read(transaction, sequence)?;
        let record = decode_transaction_record(&record_bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid journal record"))?;
        // Decoding accepts historical v1 records for recovery. Publishing a
        // new record must independently enforce the current write policy.
        super::journal::encode_transaction_record(&record).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "unsupported new journal record")
        })?;
        if record.transaction_id() != transaction || record.sequence() != sequence {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal record identity does not match its filename",
            ));
        }
        // Validate the bytes read from disk against the committed prefix at the
        // publication boundary. Direct callers may have skipped preflight, or
        // the staged record may have changed since that earlier check.
        #[cfg(test)]
        let (transaction_limit, checkpoint_limit) = self
            .storage
            .preflight_limits
            .unwrap_or((MAX_TRANSACTION_BYTES, MAX_CHECKPOINT_BYTES));
        #[cfg(not(test))]
        let (transaction_limit, checkpoint_limit) = (MAX_TRANSACTION_BYTES, MAX_CHECKPOINT_BYTES);
        self.validate_candidate(
            transaction,
            sequence,
            &record_bytes,
            transaction_limit,
            checkpoint_limit,
            Some(sequence),
        )?;
        let marker = CommitMarker {
            version: COMMIT_MARKER_VERSION,
            transaction_id: transaction.to_owned(),
            sequence,
            record_crc32: crc32(&record_bytes),
        };
        let marker_bytes = serde_json::to_vec(&marker).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "cannot encode commit marker")
        })?;
        let marker_path = self.storage.commit_path(transaction, sequence)?;
        let pending_marker_path = self.storage.commit_pending_path(transaction, sequence)?;
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::CommitMarkerCreate)?;
        let pending_name = pending_marker_path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid pending marker filename",
            )
        })?;
        let mut marker_file = self.storage.open_child(
            pending_name,
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            0,
        )?;
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::CommitMarkerWrite)?;
        marker_file.write_all(&marker_bytes)?;
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::CommitMarkerFlush)?;
        marker_file.sync_all()?;
        drop(marker_file);
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::CommitMarkerReadback)?;
        if self
            .storage
            .read_limited(&pending_marker_path, MAX_COMMIT_MARKER_BYTES)?
            != marker_bytes
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "commit marker readback mismatch",
            ));
        }
        #[cfg(test)]
        self.storage.fail_at(IoBoundary::CommitMarkerPublish)?;
        self.storage
            .publish_marker(&pending_marker_path, &marker_path)?;
        #[cfg(test)]
        self.storage
            .fail_at(IoBoundary::CommitMarkerFinalReadback)?;
        if self
            .storage
            .read_limited(&marker_path, MAX_COMMIT_MARKER_BYTES)?
            != marker_bytes
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "published commit marker readback mismatch",
            ));
        }
        Ok(())
    }

    fn read(&mut self, transaction: &str, sequence: u64) -> Result<Vec<u8>, Self::Error> {
        self.check_transaction(transaction)?;
        self.storage.read_limited(
            &self.storage.record_path(transaction, sequence)?,
            MAX_RECORD_BYTES,
        )
    }

    fn scan(&mut self, transaction: &str) -> Result<JournalScan, Self::Error> {
        self.check_transaction(transaction)?;
        // Validate the ID before constructing the enumeration prefix.
        let _ = self.storage.record_path(transaction, 0)?;
        let records = self.storage.sequences_for(transaction, ".json")?;
        let markers = self.storage.sequences_for(transaction, ".commit")?;
        let pending_markers = self.storage.sequences_for(transaction, ".commit.pending")?;
        if markers.len() > MAX_TRANSACTION_RECORDS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "transaction has too many committed records",
            ));
        }
        for (expected, sequence) in markers.iter().enumerate() {
            if *sequence != expected as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "committed journal sequence has a gap",
                ));
            }
        }
        if markers.iter().any(|sequence| !records.contains(sequence)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "committed marker has no journal record",
            ));
        }
        let next = markers.len() as u64;
        let uncommitted: Vec<_> = records.difference(&markers).copied().collect();
        if uncommitted.iter().any(|sequence| *sequence != next) || uncommitted.len() > 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal has a gap or multiple incomplete tail records",
            ));
        }
        if pending_markers.len() > 1
            || pending_markers.iter().any(|sequence| {
                !records.contains(sequence)
                    || *sequence != next
                    || uncommitted.first() != Some(sequence)
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pending commit marker does not match the incomplete journal tail",
            ));
        }

        let mut scan = JournalScan {
            committed: Vec::with_capacity(markers.len()),
            service_committed: Vec::with_capacity(markers.len()),
            typed_committed: Vec::with_capacity(markers.len()),
            incomplete_tail: uncommitted.first().copied(),
        };
        let mut total_bytes = 0usize;
        for sequence in markers {
            let record_bytes = self.read(transaction, sequence)?;
            total_bytes = total_bytes.checked_add(record_bytes.len()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "transaction size overflow")
            })?;
            if total_bytes > MAX_TRANSACTION_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "transaction exceeds the recovery scan size limit",
                ));
            }
            let record = decode_transaction_record(&record_bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid journal record")
            })?;
            if record.transaction_id() != transaction || record.sequence() != sequence {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal record identity does not match its filename",
                ));
            }
            let marker_bytes = self.storage.read_limited(
                &self.storage.commit_path(transaction, sequence)?,
                MAX_COMMIT_MARKER_BYTES,
            )?;
            let marker: CommitMarker = serde_json::from_slice(&marker_bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid journal commit marker")
            })?;
            if marker.version != COMMIT_MARKER_VERSION
                || marker.transaction_id != transaction
                || marker.sequence != sequence
                || marker.record_crc32 != crc32(&record_bytes)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "journal commit marker does not match its record",
                ));
            }
            if let TransactionRecord::Registry(registry_record) = &record {
                scan.committed.push(registry_record.clone());
            } else if let TransactionRecord::Service(service_record) = &record {
                scan.service_committed.push(service_record.clone());
            }
            scan.typed_committed.push(record);
        }
        Ok(scan)
    }
}

impl LockedJournalStorage {
    fn preflight_with_limits(
        &mut self,
        transaction: &str,
        sequence: u64,
        data: &[u8],
        transaction_limit: usize,
        checkpoint_limit: usize,
    ) -> io::Result<()> {
        self.validate_candidate(
            transaction,
            sequence,
            data,
            transaction_limit,
            checkpoint_limit,
            None,
        )
    }

    fn validate_candidate(
        &mut self,
        transaction: &str,
        sequence: u64,
        data: &[u8],
        transaction_limit: usize,
        checkpoint_limit: usize,
        expected_tail: Option<u64>,
    ) -> io::Result<()> {
        self.check_transaction(transaction)?;
        let scan = self.scan(transaction)?;
        if scan.incomplete_tail != expected_tail || scan.typed_committed.len() as u64 != sequence {
            return Err(invalid_checkpoint(
                "candidate does not follow a complete journal prefix",
            ));
        }
        let candidate = decode_transaction_record(data)
            .map_err(|_| invalid_checkpoint("invalid candidate journal record"))?;
        if candidate.transaction_id() != transaction || candidate.sequence() != sequence {
            return Err(invalid_checkpoint(
                "candidate journal identity does not match",
            ));
        }
        ensure_recovery_record_fits(&candidate, super::journal::MAX_RECORD_BYTES)?;
        // Prove the candidate also fits a fresh journal, including the longest
        // possible newly reserved ID. Failure here is not recoverable by rotation.
        let mut fresh = candidate.clone();
        let fresh_id = "a".repeat(128);
        match &mut fresh {
            TransactionRecord::Registry(record) => {
                record.transaction_id = fresh_id.clone();
                record.sequence = 0;
            }
            TransactionRecord::Service(record) => {
                record.transaction_id = fresh_id.clone();
                record.sequence = 0;
            }
        }
        let fresh_bytes = super::journal::encode_transaction_record(&fresh)
            .map_err(|_| invalid_checkpoint("candidate cannot fit a fresh journal"))?;
        let fresh_checkpoint = Checkpoint::new_typed(&[fresh], "b".repeat(128), 1)
            .map_err(|_| invalid_checkpoint("candidate cannot form a fresh checkpoint"))?;
        let fresh_checkpoint_bytes = checkpoint::encode(&fresh_checkpoint)
            .map_err(|_| invalid_checkpoint("candidate cannot fit a fresh checkpoint"))?;
        if fresh_bytes.len() > transaction_limit
            || fresh_checkpoint_bytes
                .len()
                .checked_add(9)
                .is_none_or(|size| size > checkpoint_limit)
        {
            return Err(invalid_checkpoint("candidate exceeds fresh journal limits"));
        }
        if sequence >= MAX_TRANSACTION_RECORDS as u64 {
            return Err(capacity_error());
        }
        let mut total = data.len();
        for index in 0..sequence {
            total = total
                .checked_add(self.read(transaction, index)?.len())
                .ok_or_else(|| invalid_checkpoint("transaction size overflow"))?;
        }
        if total > transaction_limit {
            return Err(capacity_error());
        }
        let mut records = scan.typed_committed;
        records.push(candidate);
        // The longest permitted ID bounds all future recovery IDs. IDs contain
        // only unescaped ASCII, so equal lengths have equal serialized sizes.
        let mut recovery_id = "a".repeat(128);
        if recovery_id == transaction {
            recovery_id = "b".repeat(128);
        }
        let checkpoint = Checkpoint::new_typed(&records, recovery_id, records.len())
            .map_err(|_| invalid_checkpoint("candidate cannot form a recovery checkpoint"))?;
        let bytes = match checkpoint::encode(&checkpoint) {
            Ok(bytes) => bytes,
            Err(checkpoint::CheckpointError::TooLarge) => return Err(capacity_error()),
            Err(_) => return Err(invalid_checkpoint("candidate cannot encode checkpoint")),
        };
        // The CRC's decimal representation may be longer for a different
        // recovery ID or progress value (at most ten digits for u32).
        if bytes
            .len()
            .checked_add(9)
            .is_none_or(|size| size > checkpoint_limit)
        {
            return Err(capacity_error());
        }
        Ok(())
    }

    fn check_transaction(&self, transaction: &str) -> io::Result<()> {
        if self.transaction == transaction {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal lock belongs to another transaction",
            ))
        }
    }

    /// Start or resume recovery while retaining this transaction's lock.
    /// The journal is rescanned and compared to the supplied committed prefix.
    pub fn open_recovery_checkpoint(
        self,
        records: Vec<RegistryRecord>,
        initial_recovery_transaction_id: Option<String>,
    ) -> io::Result<FileRecoveryCheckpoint> {
        FileRecoveryCheckpoint::open(
            self,
            records
                .into_iter()
                .map(TransactionRecord::Registry)
                .collect(),
            initial_recovery_transaction_id,
        )
    }

    /// Start or resume a typed recovery checkpoint bound to every record kind.
    /// This persists progress only; resource-specific restore orchestration is
    /// still required before any typed transaction can be mutated.
    pub fn open_typed_recovery_checkpoint(
        self,
        records: Vec<TransactionRecord>,
        initial_recovery_transaction_id: Option<String>,
    ) -> io::Result<FileRecoveryCheckpoint> {
        FileRecoveryCheckpoint::open(self, records, initial_recovery_transaction_id)
    }

    fn has_committed_recovery_checkpoint(&self) -> io::Result<bool> {
        Ok(!self
            .storage
            .checkpoint_sequences_for(&self.transaction, ".commit")?
            .is_empty())
    }
}

// Recovery snapshots reverse the observed intended state and the saved before
// state. Reserve for the longest legal identity and sequence, and for the
// tagged representation used by typed recovery even for legacy records.
fn ensure_recovery_record_fits(record: &TransactionRecord, limit: usize) -> io::Result<()> {
    let mut recovery = record.clone();
    match &mut recovery {
        TransactionRecord::Registry(record) => {
            record.transaction_id = "a".repeat(128);
            record.sequence = (MAX_TRANSACTION_RECORDS - 1) as u64;
            std::mem::swap(&mut record.before, &mut record.intended);
        }
        TransactionRecord::Service(record) => {
            record.transaction_id = "a".repeat(128);
            record.sequence = (MAX_TRANSACTION_RECORDS - 1) as u64;
            std::mem::swap(
                &mut record.before_start_type,
                &mut record.intended_start_type,
            );
        }
    }
    let bytes = super::journal::encode_transaction_record(&recovery)
        .map_err(|_| invalid_checkpoint("recovery snapshot record exceeds format limit"))?;
    if bytes.len() > limit {
        return Err(invalid_checkpoint(
            "recovery snapshot record exceeds format limit",
        ));
    }
    Ok(())
}

/// A committed legacy checkpoint, preserving its exact on-disk representation.
#[derive(Debug)]
struct ValidatedLegacyCheckpoint {
    generation: u64,
    next_exclusive: usize,
    recovery_transaction_id: String,
    payload_bytes: Vec<u8>,
    marker_bytes: Vec<u8>,
}

#[derive(Debug)]
struct ValidatedLegacyPrefix {
    committed: Option<ValidatedLegacyCheckpoint>,
    next_generation: u64,
}

impl LockedJournalStorage {
    // Requires the caller to already hold the source lock. Does not reserve,
    // publish, or alter files; staging entries have no authority.
    fn validate_legacy_checkpoint_prefix(
        &mut self,
        records: &[TransactionRecord],
    ) -> io::Result<ValidatedLegacyPrefix> {
        let transaction = self.transaction.clone();
        if records.is_empty()
            || records
                .iter()
                .any(|record| record.transaction_id() != transaction)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "recovery checkpoint requires records for its locked transaction",
            ));
        }
        let scan = self.scan(&transaction)?;
        if scan.incomplete_tail.is_some()
            || scan.typed_committed.len() != records.len()
            || scan.typed_committed != records
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recovery records do not match the committed journal prefix",
            ));
        }

        let checkpoints = self
            .storage
            .checkpoint_sequences_for(&transaction, ".json")?;
        let markers = self
            .storage
            .checkpoint_sequences_for(&transaction, ".commit")?;
        let pending_markers = self
            .storage
            .checkpoint_sequences_for(&transaction, ".commit.pending")?;
        if checkpoints.len() > MAX_CHECKPOINT_GENERATIONS
            || markers.len() > MAX_CHECKPOINT_GENERATIONS
            || pending_markers.len() > MAX_CHECKPOINT_GENERATIONS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "too many recovery checkpoint generations",
            ));
        }
        if markers
            .iter()
            .any(|generation| !checkpoints.contains(generation))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recovery checkpoint marker has no checkpoint",
            ));
        }
        let highest_generation = checkpoints.last().copied();
        if highest_generation.is_some_and(|generation| {
            generation >= MAX_CHECKPOINT_GENERATIONS as u64
                || (0..=generation).any(|expected| !checkpoints.contains(&expected))
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recovery checkpoint files have a missing generation",
            ));
        }
        if pending_markers
            .iter()
            .any(|generation| !checkpoints.contains(generation) || markers.contains(generation))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pending checkpoint marker has no uncommitted checkpoint",
            ));
        }

        let mut current_next = records.len();
        let mut committed = None;
        for generation in &markers {
            let bytes = self.storage.read_limited(
                &self.storage.checkpoint_path(&transaction, *generation)?,
                MAX_CHECKPOINT_BYTES,
            )?;
            let checkpoint = checkpoint::decode(&bytes)
                .map_err(|_| invalid_checkpoint("invalid recovery checkpoint"))?;
            let checkpoint_recovery_id = checkpoint.recovery_transaction_id().to_owned();
            let next = checkpoint
                .validate_typed(records)
                .map_err(|_| invalid_checkpoint("checkpoint does not match journal records"))?;
            let marker_bytes = self.storage.read_limited(
                &self
                    .storage
                    .checkpoint_marker_path(&transaction, *generation)?,
                MAX_COMMIT_MARKER_BYTES,
            )?;
            let marker: CheckpointMarker = serde_json::from_slice(&marker_bytes)
                .map_err(|_| invalid_checkpoint("invalid recovery checkpoint marker"))?;
            if marker.version != COMMIT_MARKER_VERSION
                || marker.transaction_id != transaction
                || marker.generation != *generation
                || marker.checkpoint_crc32 != crc32(&bytes)
                || next > current_next
            {
                return Err(invalid_checkpoint(
                    "recovery checkpoint marker or progress is inconsistent",
                ));
            }
            if committed
                .as_ref()
                .is_some_and(|last: &ValidatedLegacyCheckpoint| {
                    last.recovery_transaction_id != checkpoint_recovery_id && next != current_next
                })
            {
                return Err(invalid_checkpoint(
                    "recovery transaction identity changed while progress changed",
                ));
            }
            committed = Some(ValidatedLegacyCheckpoint {
                generation: *generation,
                next_exclusive: next,
                recovery_transaction_id: checkpoint_recovery_id,
                payload_bytes: bytes,
                marker_bytes,
            });
            current_next = next;
        }

        let next_generation = checkpoints
            .last()
            .copied()
            .map(|generation| generation.checked_add(1))
            .unwrap_or(Some(0))
            .ok_or_else(|| invalid_checkpoint("checkpoint generation exhausted"))?;
        if next_generation > MAX_CHECKPOINT_GENERATIONS as u64 {
            return Err(invalid_checkpoint("too many recovery checkpoints"));
        }
        Ok(ValidatedLegacyPrefix {
            committed,
            next_generation,
        })
    }
}

/// Append-only durable progress for reverse recovery. It owns the original
/// transaction lock so the journal cannot be concurrently extended while the
/// checkpoint is in use.
#[derive(Debug)]
pub struct FileRecoveryCheckpoint {
    storage: LockedJournalStorage,
    records: Vec<TransactionRecord>,
    recovery_transaction_id: String,
    next_generation: u64,
    next_exclusive: usize,
    poisoned: bool,
}

impl FileRecoveryCheckpoint {
    fn open(
        mut storage: LockedJournalStorage,
        records: Vec<TransactionRecord>,
        initial_recovery_transaction_id: Option<String>,
    ) -> io::Result<Self> {
        let transaction = storage.transaction.clone();
        if records.is_empty()
            || records
                .iter()
                .any(|record| record.transaction_id() != transaction)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "recovery checkpoint requires records for its locked transaction",
            ));
        }
        if initial_recovery_transaction_id
            .as_ref()
            .is_some_and(|id| !valid_transaction_id(id) || id == &transaction)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid recovery snapshot transaction ID",
            ));
        }
        let validated = storage.validate_legacy_checkpoint_prefix(&records)?;
        let no_committed = validated.committed.is_none();
        let (recovery_transaction_id, current_next) = match validated.committed {
            Some(committed) => {
                if initial_recovery_transaction_id
                    .as_ref()
                    .is_some_and(|candidate| candidate != &committed.recovery_transaction_id)
                {
                    return Err(invalid_checkpoint(
                        "requested recovery ID differs from persisted checkpoint identity",
                    ));
                }
                let _ = (
                    committed.generation,
                    committed.payload_bytes,
                    committed.marker_bytes,
                );
                (committed.recovery_transaction_id, committed.next_exclusive)
            }
            None => (
                initial_recovery_transaction_id.ok_or_else(|| {
                    invalid_checkpoint(
                    "new recovery requires a snapshot transaction ID; no committed identity exists",
                )
                })?,
                records.len(),
            ),
        };
        let next_generation = validated.next_generation;
        let mut store = Self {
            storage,
            records,
            recovery_transaction_id,
            next_generation,
            next_exclusive: current_next,
            poisoned: false,
        };
        if no_committed {
            store.persist_next(current_next)?;
        }
        Ok(store)
    }

    /// Return the last committed reverse-recovery index.
    pub fn next_exclusive(&self) -> usize {
        self.next_exclusive
    }

    pub fn recovery_transaction_id(&self) -> &str {
        &self.recovery_transaction_id
    }

    fn rotate_snapshot_transaction(&mut self) -> io::Result<(String, LockedJournalStorage)> {
        let (new_id, snapshot_storage) =
            reserve_new_recovery_transaction(&self.storage.storage, None)?;
        self.poisoned = true;
        self.recovery_transaction_id = new_id.clone();
        self.persist_generation(self.next_exclusive)?;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or_else(|| invalid_checkpoint("checkpoint generation exhausted"))?;
        self.poisoned = false;
        Ok((new_id, snapshot_storage))
    }

    fn rotate_snapshot_journal(&mut self) -> io::Result<RegistryJournal<LockedJournalStorage>> {
        let (new_id, storage) = self.rotate_snapshot_transaction()?;
        Ok(RegistryJournal::new(storage, new_id))
    }

    fn rotate_typed_snapshot_journal(&mut self) -> io::Result<TypedJournal<LockedJournalStorage>> {
        let (new_id, storage) = self.rotate_snapshot_transaction()?;
        Ok(TypedJournal::new(storage, new_id))
    }

    fn persist_generation(&mut self, next_exclusive: usize) -> io::Result<()> {
        let transaction = self.storage.transaction.clone();
        let generation = self.next_generation;
        if generation >= MAX_CHECKPOINT_GENERATIONS as u64 {
            return Err(invalid_checkpoint("too many recovery checkpoints"));
        }
        let checkpoint = Checkpoint::new_typed(
            &self.records,
            self.recovery_transaction_id.clone(),
            next_exclusive,
        )
        .map_err(|_| invalid_checkpoint("cannot encode recovery checkpoint"))?;
        let bytes = checkpoint::encode(&checkpoint)
            .map_err(|_| invalid_checkpoint("cannot encode recovery checkpoint"))?;
        let path = self
            .storage
            .storage
            .checkpoint_path(&transaction, generation)?;
        #[cfg(test)]
        self.storage.storage.fail_at(IoBoundary::CheckpointCreate)?;
        let checkpoint_name = path
            .file_name()
            .ok_or_else(|| invalid_checkpoint("invalid checkpoint filename"))?;
        let mut file = self.storage.storage.open_child(
            checkpoint_name,
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            0,
        )?;
        #[cfg(test)]
        self.storage.storage.fail_at(IoBoundary::CheckpointWrite)?;
        file.write_all(&bytes)?;
        #[cfg(test)]
        self.storage.storage.fail_at(IoBoundary::CheckpointFlush)?;
        file.sync_all()?;
        drop(file);
        #[cfg(test)]
        self.storage
            .storage
            .fail_at(IoBoundary::CheckpointReadback)?;
        if self
            .storage
            .storage
            .read_limited(&path, MAX_CHECKPOINT_BYTES)?
            != bytes
        {
            return Err(invalid_checkpoint("checkpoint readback mismatch"));
        }
        let marker = CheckpointMarker {
            version: COMMIT_MARKER_VERSION,
            transaction_id: transaction.clone(),
            generation,
            checkpoint_crc32: crc32(&bytes),
        };
        let marker_bytes = serde_json::to_vec(&marker)
            .map_err(|_| invalid_checkpoint("cannot encode checkpoint marker"))?;
        let marker_path = self
            .storage
            .storage
            .checkpoint_marker_path(&transaction, generation)?;
        let pending_marker_path = self
            .storage
            .storage
            .checkpoint_marker_pending_path(&transaction, generation)?;
        #[cfg(test)]
        self.storage
            .storage
            .fail_at(IoBoundary::CheckpointMarkerCreate)?;
        let pending_name = pending_marker_path
            .file_name()
            .ok_or_else(|| invalid_checkpoint("invalid checkpoint marker filename"))?;
        let mut marker_file = self.storage.storage.open_child(
            pending_name,
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            0,
        )?;
        #[cfg(test)]
        self.storage
            .storage
            .fail_at(IoBoundary::CheckpointMarkerWrite)?;
        marker_file.write_all(&marker_bytes)?;
        #[cfg(test)]
        self.storage
            .storage
            .fail_at(IoBoundary::CheckpointMarkerFlush)?;
        marker_file.sync_all()?;
        drop(marker_file);
        #[cfg(test)]
        self.storage
            .storage
            .fail_at(IoBoundary::CheckpointMarkerReadback)?;
        if self
            .storage
            .storage
            .read_limited(&pending_marker_path, MAX_COMMIT_MARKER_BYTES)?
            != marker_bytes
        {
            return Err(invalid_checkpoint("checkpoint marker readback mismatch"));
        }
        #[cfg(test)]
        self.storage
            .storage
            .fail_at(IoBoundary::CheckpointMarkerPublish)?;
        self.storage
            .storage
            .publish_marker(&pending_marker_path, &marker_path)?;
        #[cfg(test)]
        self.storage
            .storage
            .fail_at(IoBoundary::CheckpointMarkerFinalReadback)?;
        if self
            .storage
            .storage
            .read_limited(&marker_path, MAX_COMMIT_MARKER_BYTES)?
            != marker_bytes
        {
            return Err(invalid_checkpoint(
                "published checkpoint marker readback mismatch",
            ));
        }
        Ok(())
    }
}

impl RecoveryCheckpoint for FileRecoveryCheckpoint {
    type Error = io::Error;

    fn next_exclusive(&self) -> usize {
        self.next_exclusive
    }

    fn persist_next(&mut self, next_exclusive: usize) -> Result<(), Self::Error> {
        if self.poisoned {
            return Err(invalid_checkpoint("recovery checkpoint writer is poisoned"));
        }
        if next_exclusive > self.next_exclusive || next_exclusive > self.records.len() {
            return Err(invalid_checkpoint(
                "recovery checkpoint progress cannot move forward",
            ));
        }
        self.poisoned = true;
        self.persist_generation(next_exclusive)?;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or_else(|| invalid_checkpoint("checkpoint generation exhausted"))?;
        self.next_exclusive = next_exclusive;
        self.poisoned = false;
        Ok(())
    }
}

fn invalid_checkpoint(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn generated_recovery_transaction_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_RECOVERY_ID.fetch_add(1, Ordering::Relaxed);
    format!(
        "recovery-{timestamp:x}-{:x}-{sequence:x}",
        std::process::id()
    )
}

fn reserve_new_recovery_transaction(
    storage: &FileJournalStorage,
    requested_id: Option<String>,
) -> io::Result<(String, LockedJournalStorage)> {
    if let Some(id) = requested_id {
        let locked = storage.try_clone()?.create_new_transaction(id.clone())?;
        return Ok((id, locked));
    }

    for _ in 0..MAX_RECOVERY_ID_ATTEMPTS {
        let id = generated_recovery_transaction_id();
        match storage.try_clone()?.create_new_transaction(id.clone()) {
            Ok(locked) => return Ok((id, locked)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a unique recovery transaction ID",
    ))
}

/// Recovery coordinator retaining the original transaction lock, separate
/// recovery-snapshot lock, and registry mutation barrier for the session.
pub struct FileRecoverySession {
    records: Vec<RegistryRecord>,
    checkpoint: FileRecoveryCheckpoint,
    snapshots: RegistryJournal<LockedJournalStorage>,
    _root_storage: FileJournalStorage,
    _registry_lock: RegistryMutationLock,
}

#[derive(Debug)]
pub enum FileRecoverySessionError {
    Storage(io::Error),
    SnapshotJournal(JournalError<io::Error>),
}

impl From<io::Error> for FileRecoverySessionError {
    fn from(error: io::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<JournalError<io::Error>> for FileRecoverySessionError {
    fn from(error: JournalError<io::Error>) -> Self {
        Self::SnapshotJournal(error)
    }
}

impl FileRecoverySession {
    /// Start recovery with a freshly generated, exclusively reserved snapshot ID.
    pub fn start(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
    ) -> Result<Self, FileRecoverySessionError> {
        Self::open(root, transaction, None)
    }

    /// Start recovery with an explicit snapshot ID, useful for deterministic callers.
    pub fn start_with_id(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
        recovery_transaction_id: impl Into<String>,
    ) -> Result<Self, FileRecoverySessionError> {
        Self::open(root, transaction, Some(recovery_transaction_id.into()))
    }

    /// Resume a previously started recovery session.
    pub fn resume(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
    ) -> Result<Self, FileRecoverySessionError> {
        Self::open(root, transaction, None)
    }

    /// Open a recovery session for a committed transaction.
    ///
    /// Supply `Some(new_id)` for an explicit fresh attempt. `None` generates
    /// an ID for a fresh attempt or resumes the persisted ID after restart.
    /// IDs are exclusively reserved and persisted before any restore.
    pub fn open(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
        new_recovery_transaction: Option<String>,
    ) -> Result<Self, FileRecoverySessionError> {
        let transaction = transaction.into();
        let root_storage = FileJournalStorage::open(root.as_ref())?;
        let mut original = root_storage
            .try_clone()?
            .lock_transaction(transaction.clone())?;
        let registry_lock = root_storage.try_clone()?.lock_registry_mutations()?;
        let scan = original.scan(&transaction)?;
        if scan.incomplete_tail.is_some()
            || scan.committed.is_empty()
            || scan.typed_committed.len() != scan.committed.len()
        {
            return Err(FileRecoverySessionError::Storage(invalid_checkpoint(
                "recovery requires a nonempty fully committed transaction",
            )));
        }
        let records = scan.committed;
        if original.has_committed_recovery_checkpoint()? {
            if new_recovery_transaction.is_some() {
                return Err(FileRecoverySessionError::Storage(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "recovery checkpoint already exists; resume without a new ID",
                )));
            }
            let mut checkpoint = original.open_recovery_checkpoint(records.clone(), None)?;
            let recovery_id = checkpoint.recovery_transaction_id().to_owned();
            let snapshot_storage = root_storage
                .try_clone()?
                .lock_transaction(recovery_id.clone())?;
            let snapshots = match RegistryJournal::resume(snapshot_storage, recovery_id) {
                Ok(snapshots) => snapshots,
                Err(JournalError::IncompleteTail(_)) => checkpoint.rotate_snapshot_journal()?,
                Err(error) => return Err(error.into()),
            };
            Ok(Self {
                records,
                checkpoint,
                snapshots,
                _root_storage: root_storage,
                _registry_lock: registry_lock,
            })
        } else {
            let (recovery_id, snapshot_storage) =
                reserve_new_recovery_transaction(&root_storage, new_recovery_transaction)?;
            let checkpoint =
                original.open_recovery_checkpoint(records.clone(), Some(recovery_id.clone()))?;
            let snapshots = RegistryJournal::new(snapshot_storage, recovery_id);
            Ok(Self {
                records,
                checkpoint,
                snapshots,
                _root_storage: root_storage,
                _registry_lock: registry_lock,
            })
        }
    }

    pub fn recovery_transaction_id(&self) -> &str {
        self.checkpoint.recovery_transaction_id()
    }

    pub fn next_exclusive(&self) -> usize {
        self.checkpoint.next_exclusive()
    }

    fn finish_run<B>(
        &mut self,
        result: JournalRecoveryResult<B, JournalError<io::Error>, io::Error>,
    ) -> JournalRecoveryResult<B, JournalError<io::Error>, io::Error> {
        if !is_snapshot_capacity(&result) {
            return result;
        }
        match self.checkpoint.rotate_snapshot_journal() {
            Ok(snapshots) => {
                self.snapshots = snapshots;
                Err(JournalRecoveryError::SnapshotRotatedRetryRequired)
            }
            Err(error) => {
                self.checkpoint.poisoned = true;
                Err(JournalRecoveryError::SnapshotRotation(error))
            }
        }
    }

    /// Resume reverse recovery from the loaded checkpoint. The supplied
    /// backend must still provide exclusive target ownership for each call.
    pub fn run<B>(
        &mut self,
        backend: &mut B,
    ) -> JournalRecoveryResult<B::Error, JournalError<io::Error>, io::Error>
    where
        B: RecoveryBackend<
            Target = crate::engine::registry::ValueTarget,
            State = crate::engine::registry::ValueState,
        >,
    {
        if self.checkpoint.poisoned {
            return Err(JournalRecoveryError::SnapshotRotation(invalid_checkpoint(
                "reopen poisoned recovery session",
            )));
        }
        let result = crate::engine::recovery::recover_journal_prefix(
            backend,
            &mut self.snapshots,
            &mut self.checkpoint,
            &self.records,
        );
        self.finish_run(result)
    }

    /// Run recovery with the guarded Win32 registry backend. The session owns
    /// the registry barrier and both journal locks for the entire call.
    #[cfg(windows)]
    pub fn run_registry(
        &mut self,
    ) -> JournalRecoveryResult<
        crate::engine::registry::windows::WriteError,
        JournalError<io::Error>,
        io::Error,
    > {
        if self.checkpoint.poisoned {
            return Err(JournalRecoveryError::SnapshotRotation(invalid_checkpoint(
                "reopen poisoned recovery session",
            )));
        }
        let mut backend =
            crate::engine::registry::windows::WindowsRegistryBackend::new(&self._registry_lock);
        let result = crate::engine::recovery::recover_journal_prefix(
            &mut backend,
            &mut self.snapshots,
            &mut self.checkpoint,
            &self.records,
        );
        drop(backend);
        self.finish_run(result)
    }
}

/// Managed recovery session for typed registry/service journals. It retains
/// the source lock, typed checkpoint, separate snapshot-journal lock, and the
/// shared mutation barrier until dropped. Callers can supply a mock backend or,
/// on Windows, use the guarded typed registry/SCM backend through `run_windows`.
pub struct FileTypedRecoverySession {
    records: Vec<TransactionRecord>,
    checkpoint: FileRecoveryCheckpoint,
    snapshots: TypedJournal<LockedJournalStorage>,
    _root_storage: FileJournalStorage,
    _mutation_lock: RegistryMutationLock,
}

#[derive(Debug)]
pub enum FileTypedRecoverySessionError {
    Storage(io::Error),
    SnapshotJournal(JournalError<io::Error>),
}

impl From<io::Error> for FileTypedRecoverySessionError {
    fn from(error: io::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<JournalError<io::Error>> for FileTypedRecoverySessionError {
    fn from(error: JournalError<io::Error>) -> Self {
        Self::SnapshotJournal(error)
    }
}

impl FileTypedRecoverySession {
    pub fn start(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
    ) -> Result<Self, FileTypedRecoverySessionError> {
        Self::open(root, transaction, None)
    }

    pub fn start_with_id(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
        recovery_transaction_id: impl Into<String>,
    ) -> Result<Self, FileTypedRecoverySessionError> {
        Self::open(root, transaction, Some(recovery_transaction_id.into()))
    }

    pub fn resume(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
    ) -> Result<Self, FileTypedRecoverySessionError> {
        Self::open(root, transaction, None)
    }

    /// Open a fresh or interrupted typed recovery session. The checkpoint
    /// persists its snapshot-journal ID before any backend restore can run.
    pub fn open(
        root: impl AsRef<Path>,
        transaction: impl Into<String>,
        new_recovery_transaction: Option<String>,
    ) -> Result<Self, FileTypedRecoverySessionError> {
        let transaction = transaction.into();
        let root_storage = FileJournalStorage::open(root.as_ref())?;
        let mut original = root_storage
            .try_clone()?
            .lock_transaction(transaction.clone())?;
        let mutation_lock = root_storage.try_clone()?.lock_system_mutations()?;
        let scan = original.scan(&transaction)?;
        if scan.incomplete_tail.is_some() || scan.typed_committed.is_empty() {
            return Err(FileTypedRecoverySessionError::Storage(invalid_checkpoint(
                "typed recovery requires a nonempty fully committed transaction",
            )));
        }
        let records = scan.typed_committed;

        if original.has_committed_recovery_checkpoint()? {
            if new_recovery_transaction.is_some() {
                return Err(FileTypedRecoverySessionError::Storage(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "typed recovery checkpoint already exists; resume without a new ID",
                )));
            }
            let mut checkpoint = original.open_typed_recovery_checkpoint(records.clone(), None)?;
            let recovery_id = checkpoint.recovery_transaction_id().to_owned();
            let snapshot_storage = root_storage
                .try_clone()?
                .lock_transaction(recovery_id.clone())?;
            let snapshots = match TypedJournal::resume(snapshot_storage, recovery_id) {
                Ok(snapshots) => snapshots,
                Err(JournalError::IncompleteTail(_)) => {
                    checkpoint.rotate_typed_snapshot_journal()?
                }
                Err(error) => return Err(error.into()),
            };
            Ok(Self {
                records,
                checkpoint,
                snapshots,
                _root_storage: root_storage,
                _mutation_lock: mutation_lock,
            })
        } else {
            let (recovery_id, snapshot_storage) =
                reserve_new_recovery_transaction(&root_storage, new_recovery_transaction)?;
            let checkpoint = original
                .open_typed_recovery_checkpoint(records.clone(), Some(recovery_id.clone()))?;
            let snapshots = TypedJournal::new(snapshot_storage, recovery_id);
            Ok(Self {
                records,
                checkpoint,
                snapshots,
                _root_storage: root_storage,
                _mutation_lock: mutation_lock,
            })
        }
    }

    pub fn recovery_transaction_id(&self) -> &str {
        self.checkpoint.recovery_transaction_id()
    }

    pub fn next_exclusive(&self) -> usize {
        self.checkpoint.next_exclusive()
    }

    fn finish_run<B>(
        &mut self,
        result: JournalRecoveryResult<B, JournalError<io::Error>, io::Error>,
    ) -> JournalRecoveryResult<B, JournalError<io::Error>, io::Error> {
        if !is_snapshot_capacity(&result) {
            return result;
        }
        match self.checkpoint.rotate_typed_snapshot_journal() {
            Ok(snapshots) => {
                self.snapshots = snapshots;
                Err(JournalRecoveryError::SnapshotRotatedRetryRequired)
            }
            Err(error) => {
                self.checkpoint.poisoned = true;
                Err(JournalRecoveryError::SnapshotRotation(error))
            }
        }
    }

    pub fn run<B>(
        &mut self,
        backend: &mut B,
    ) -> JournalRecoveryResult<B::Error, JournalError<io::Error>, io::Error>
    where
        B: crate::engine::recovery::TypedRecoveryBackend,
    {
        if self.checkpoint.poisoned {
            return Err(JournalRecoveryError::SnapshotRotation(invalid_checkpoint(
                "reopen poisoned recovery session",
            )));
        }
        let result = crate::engine::recovery::recover_typed_journal_prefix(
            backend,
            &mut self.snapshots,
            &mut self.checkpoint,
            &self.records,
        );
        self.finish_run(result)
    }

    /// Run recovery with the guarded Win32 registry/SCM backend. The session
    /// retains the source, snapshot and checkpoint locks plus mutation barrier.
    #[cfg(windows)]
    pub fn run_windows(
        &mut self,
    ) -> JournalRecoveryResult<
        crate::engine::services::TypedBackendError,
        JournalError<io::Error>,
        io::Error,
    > {
        if self.checkpoint.poisoned {
            return Err(JournalRecoveryError::SnapshotRotation(invalid_checkpoint(
                "reopen poisoned recovery session",
            )));
        }
        let mut backend = crate::engine::services::WindowsTypedBackend::new(&self._mutation_lock);
        let result = crate::engine::recovery::recover_typed_journal_prefix(
            &mut backend,
            &mut self.snapshots,
            &mut self.checkpoint,
            &self.records,
        );
        drop(backend);
        self.finish_run(result)
    }
}

impl FileTransactionSession {
    fn start(root: impl AsRef<Path>, transaction: impl Into<String>) -> io::Result<Self> {
        let transaction = transaction.into();
        let storage = FileJournalStorage::open(root)?;
        let root_storage = storage.try_clone()?;
        let journal_storage = storage.create_new_transaction(transaction.clone())?;
        let journal = RegistryJournal::new(journal_storage, transaction.clone());
        Ok(Self {
            root_storage,
            transaction,
            journal,
        })
    }

    pub fn transaction_id(&self) -> &str {
        &self.transaction
    }

    /// Run one backed-up operation while excluding every other cooperating
    /// registry mutation in this journal root.
    pub fn apply<B>(
        &mut self,
        backend: &mut B,
        operation: &Operation<ValueTarget, ValueState>,
    ) -> Result<(), FileTransactionError<B::Error, JournalError<io::Error>>>
    where
        B: Backend<Target = ValueTarget, Change = ValueState, Snapshot = ValueState>,
    {
        let _registry_lock = self
            .root_storage
            .try_clone()
            .and_then(|storage| storage.lock_registry_mutations())
            .map_err(FileTransactionError::RegistryLock)?;
        self.apply_under_registry_lock(backend, operation)
    }

    #[cfg(windows)]
    pub fn apply_registry(
        &mut self,
        operation: &Operation<ValueTarget, ValueState>,
    ) -> Result<
        (),
        FileTransactionError<crate::engine::registry::windows::WriteError, JournalError<io::Error>>,
    > {
        let registry_lock = self
            .root_storage
            .try_clone()
            .and_then(|storage| storage.lock_registry_mutations())
            .map_err(FileTransactionError::RegistryLock)?;
        let mut backend =
            crate::engine::registry::windows::WindowsRegistryBackend::new(&registry_lock);
        self.apply_under_registry_lock(&mut backend, operation)
    }

    fn apply_under_registry_lock<B>(
        &mut self,
        backend: &mut B,
        operation: &Operation<ValueTarget, ValueState>,
    ) -> Result<(), FileTransactionError<B::Error, JournalError<io::Error>>>
    where
        B: Backend<Target = ValueTarget, Change = ValueState, Snapshot = ValueState>,
    {
        crate::engine::transaction::apply_one(backend, &mut self.journal, operation)
            .map_err(FileTransactionError::Operation)
    }
}

impl FileTypedTransactionSession {
    fn start(root: impl AsRef<Path>, transaction: impl Into<String>) -> io::Result<Self> {
        let transaction = transaction.into();
        let storage = FileJournalStorage::open(root)?;
        let root_storage = storage.try_clone()?;
        let journal_storage = storage.create_new_transaction(transaction.clone())?;
        let journal = TypedJournal::new(journal_storage, transaction.clone());
        Ok(Self {
            root_storage,
            transaction,
            journal,
        })
    }

    pub fn transaction_id(&self) -> &str {
        &self.transaction
    }

    pub fn apply<B>(
        &mut self,
        backend: &mut B,
        operation: &Operation<TypedTarget, TypedState>,
    ) -> Result<(), FileTypedTransactionError<B::Error, JournalError<io::Error>>>
    where
        B: Backend<Target = TypedTarget, Change = TypedState, Snapshot = TypedState>,
    {
        let _mutation_lock = self
            .root_storage
            .try_clone()
            .and_then(|storage| storage.lock_system_mutations())
            .map_err(FileTypedTransactionError::MutationLock)?;
        crate::engine::transaction::apply_one(backend, &mut self.journal, operation)
            .map_err(FileTypedTransactionError::Operation)
    }

    /// Evaluate the guard inside the mutation barrier against the queried snapshot.
    /// If accepted, persist that same snapshot before applying the operation.
    pub fn apply_if<B, F>(
        &mut self,
        backend: &mut B,
        operation: &Operation<TypedTarget, TypedState>,
        guard: F,
    ) -> Result<
        crate::engine::transaction::ApplyOutcome,
        FileTypedTransactionError<B::Error, JournalError<io::Error>>,
    >
    where
        B: Backend<Target = TypedTarget, Change = TypedState, Snapshot = TypedState>,
        F: FnOnce(&TypedState) -> bool,
    {
        let _mutation_lock = self
            .root_storage
            .try_clone()
            .and_then(|storage| storage.lock_system_mutations())
            .map_err(FileTypedTransactionError::MutationLock)?;
        crate::engine::transaction::apply_one_if(backend, &mut self.journal, operation, guard)
            .map_err(FileTypedTransactionError::Operation)
    }

    /// Apply through the Win32 registry/SCM backend under this session's
    /// durable typed journal and the shared root mutation barrier.
    #[cfg(windows)]
    pub fn apply_windows(
        &mut self,
        operation: &Operation<TypedTarget, TypedState>,
    ) -> Result<
        (),
        FileTypedTransactionError<
            crate::engine::services::TypedBackendError,
            JournalError<io::Error>,
        >,
    > {
        let mutation_lock = self
            .root_storage
            .try_clone()
            .and_then(|storage| storage.lock_system_mutations())
            .map_err(FileTypedTransactionError::MutationLock)?;
        let mut backend = crate::engine::services::WindowsTypedBackend::new(&mutation_lock);
        crate::engine::transaction::apply_one(&mut backend, &mut self.journal, operation)
            .map_err(FileTypedTransactionError::Operation)
    }

    /// Apply a condition under the same barrier as capture, journal and write.
    /// This is an engine primitive, not authorization to expose system actions.
    #[cfg(windows)]
    pub(crate) fn apply_windows_if<F>(
        &mut self,
        operation: &Operation<TypedTarget, TypedState>,
        guard: F,
    ) -> Result<
        crate::engine::transaction::ApplyOutcome,
        FileTypedTransactionError<
            crate::engine::tweak_plan::QueryError<crate::engine::services::TypedBackendError>,
            JournalError<io::Error>,
        >,
    >
    where
        F: FnOnce(&TypedState) -> bool,
    {
        self.with_mutation_lock(|lock, journal| {
            let mut backend = crate::engine::services::WindowsTypedBackend::new(lock);
            Self::apply_guarded_under_lock(&mut backend, journal, operation, guard)
        })
    }

    #[cfg(windows)]
    fn apply_guarded_under_lock<B, F>(
        backend: &mut B,
        journal: &mut TypedJournal<LockedJournalStorage>,
        operation: &Operation<TypedTarget, TypedState>,
        guard: F,
    ) -> Result<
        crate::engine::transaction::ApplyOutcome,
        FileTypedTransactionError<
            crate::engine::tweak_plan::QueryError<B::Error>,
            JournalError<io::Error>,
        >,
    >
    where
        B: Backend<Target = TypedTarget, Change = TypedState, Snapshot = TypedState>,
        F: FnOnce(&TypedState) -> bool,
    {
        let mut backend = crate::engine::tweak_plan::ExistingKeyBackend(backend);
        crate::engine::transaction::apply_one_if(&mut backend, journal, operation, guard)
            .map_err(FileTypedTransactionError::Operation)
    }

    #[cfg(windows)]
    fn with_mutation_lock<R, E>(
        &mut self,
        run: impl FnOnce(
            &RegistryMutationLock,
            &mut TypedJournal<LockedJournalStorage>,
        ) -> Result<R, FileTypedTransactionError<E, JournalError<io::Error>>>,
    ) -> Result<R, FileTypedTransactionError<E, JournalError<io::Error>>> {
        let lock = self
            .root_storage
            .try_clone()
            .and_then(|storage| storage.lock_system_mutations())
            .map_err(FileTypedTransactionError::MutationLock)?;
        run(&lock, &mut self.journal)
    }

    #[cfg(all(test, windows))]
    pub(crate) fn apply_guarded_mock_if<B, F>(
        &mut self,
        backend: &mut B,
        operation: &Operation<TypedTarget, TypedState>,
        guard: F,
    ) -> Result<
        crate::engine::transaction::ApplyOutcome,
        FileTypedTransactionError<
            crate::engine::tweak_plan::QueryError<B::Error>,
            JournalError<io::Error>,
        >,
    >
    where
        B: Backend<Target = TypedTarget, Change = TypedState, Snapshot = TypedState>,
        F: FnOnce(&TypedState) -> bool,
    {
        self.with_mutation_lock(|_lock, journal| {
            Self::apply_guarded_under_lock(backend, journal, operation, guard)
        })
    }
}

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

fn enumerate_directory(directory: &File) -> io::Result<Vec<(OsString, u32)>> {
    // NT directory queries write variable-sized records containing UTF-16
    // names. A 2 MiB aligned buffer bounds each native call; repeated calls
    // continue from the handle's enumeration cursor.
    const BUFFER_BYTES: usize = 2 * 1024 * 1024;
    let mut result = Vec::new();
    let mut restart = 1u8;
    loop {
        let mut buffer = vec![0u64; BUFFER_BYTES / std::mem::size_of::<u64>()];
        let mut iosb: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        let status = unsafe {
            NtQueryDirectoryFile(
                directory.as_raw_handle() as HANDLE,
                std::ptr::null_mut(),
                None,
                std::ptr::null(),
                &mut iosb,
                buffer.as_mut_ptr().cast(),
                BUFFER_BYTES as u32,
                FileIdBothDirectoryInformation,
                0,
                std::ptr::null(),
                restart,
            )
        };
        restart = 0;
        if status == STATUS_NO_MORE_FILES {
            break;
        }
        if status < 0 {
            let code = unsafe { RtlNtStatusToDosError(status) };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let used = iosb.Information;
        if used == 0 || used > BUFFER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid directory query byte count",
            ));
        }
        let base = buffer.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        loop {
            let header_size = std::mem::size_of::<FILE_ID_BOTH_DIR_INFORMATION>();
            if offset.checked_add(header_size).is_none_or(|end| end > used) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "truncated directory query entry",
                ));
            }
            let entry = unsafe {
                base.add(offset)
                    .cast::<FILE_ID_BOTH_DIR_INFORMATION>()
                    .read_unaligned()
            };
            let name_bytes = entry.FileNameLength as usize;
            let name_offset = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFORMATION, FileName);
            let name_start = offset.checked_add(name_offset).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "directory name offset overflow")
            })?;
            if !name_bytes.is_multiple_of(2)
                || name_start
                    .checked_add(name_bytes)
                    .is_none_or(|end| end > used)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid directory entry name",
                ));
            }
            let name_raw = unsafe { std::slice::from_raw_parts(base.add(name_start), name_bytes) };
            let name_wide: Vec<u16> = name_raw
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            let name = OsString::from_wide(&name_wide);
            if name != "." && name != ".." {
                result.push((name, entry.FileAttributes));
            }
            let next = entry.NextEntryOffset as usize;
            if next == 0 {
                break;
            }
            if next < name_offset + name_bytes
                || !next.is_multiple_of(std::mem::align_of::<FILE_ID_BOTH_DIR_INFORMATION>())
                || offset.checked_add(next).is_none_or(|end| end > used)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid directory entry offset",
                ));
            }
            offset += next;
        }
    }
    Ok(result)
}

fn parse_transaction_filename(name: &str) -> io::Result<String> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "malformed journal filename");
    let bytes = name.as_bytes();
    if bytes.len() < 5 || bytes[3] != b'-' || !bytes[..3].iter().all(u8::is_ascii_digit) {
        return Err(invalid());
    }
    let id_length = std::str::from_utf8(&bytes[..3])
        .map_err(|_| invalid())?
        .parse::<usize>()
        .map_err(|_| invalid())?;
    let id_start = 4usize;
    let separator = id_start.checked_add(id_length).ok_or_else(invalid)?;
    if id_length == 0 || separator >= bytes.len() {
        return Err(invalid());
    }
    let id = std::str::from_utf8(&bytes[id_start..separator]).map_err(|_| invalid())?;
    if !valid_transaction_id(id) {
        return Err(invalid());
    }
    let suffix = &bytes[separator..];
    if suffix == b".lock" {
        return Ok(id.to_owned());
    }
    if suffix.first() != Some(&b'-') {
        return Err(invalid());
    }
    let mut file_suffix = &suffix[1..];
    if file_suffix.starts_with(b"recovery-") {
        file_suffix = &file_suffix[b"recovery-".len()..];
    }
    let extension = if file_suffix.ends_with(b".commit.pending") {
        b".commit.pending".as_slice()
    } else if file_suffix.ends_with(b".json") {
        b".json".as_slice()
    } else if file_suffix.ends_with(b".commit") {
        b".commit".as_slice()
    } else {
        return Err(invalid());
    };
    if file_suffix.len() != 20 + extension.len() {
        return Err(invalid());
    }
    let sequence = &file_suffix[..20];
    if !sequence.iter().all(u8::is_ascii_digit)
        || std::str::from_utf8(sequence)
            .map_err(|_| invalid())?
            .parse::<u64>()
            .is_err()
    {
        return Err(invalid());
    }
    Ok(id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::journal::{
        decode, encode_transaction_record, RegistryRecord, ServiceRecord, ServiceTarget,
        TransactionRecord, SERVICE_FORMAT_VERSION,
    };
    use crate::data::store::{
        JournalError, RegistryJournal, ServiceJournal, TypedState, TypedTarget,
    };
    use crate::engine::recovery::{RecoveryBackend, RecoveryCheckpoint, TypedRecoveryBackend};
    use crate::engine::registry::{Hive, RegistryValue, RegistryView, ValueState, ValueTarget};
    use crate::engine::transaction::SnapshotStore;
    use std::collections::BTreeMap;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn file_backed_service_journal_persists_and_resumes_typed_records() {
        let directory = TestDirectory::new();
        let transaction = "tx_service_roundtrip".to_owned();
        let target = ServiceTarget {
            name: "ExampleService".into(),
        };
        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .create_new_transaction(transaction.clone())
            .unwrap();
        let mut journal = ServiceJournal::new(storage, transaction.clone());
        SnapshotStore::persist(&mut journal, &target, &3, &4).unwrap();

        let storage = journal.into_storage();
        drop(storage);

        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(&transaction)
            .unwrap();
        let mut journal = ServiceJournal::resume(storage, transaction.clone()).unwrap();
        SnapshotStore::persist(&mut journal, &target, &4, &2).unwrap();
        let mut storage = journal.into_storage();
        let scan = storage.scan(&transaction).unwrap();

        assert!(scan.committed.is_empty());
        assert_eq!(scan.service_committed.len(), 2);
        assert_eq!(scan.typed_committed.len(), 2);
        assert_eq!(scan.typed_committed[0].sequence(), 0);
        assert_eq!(scan.typed_committed[1].sequence(), 1);
        assert!(scan.typed_committed.iter().all(|record| matches!(
            record,
            TransactionRecord::Service(service)
                if service.target == target && service.transaction_id == transaction
        )));
    }

    #[test]
    fn typed_service_recovery_checkpoint_survives_reopen() {
        let directory = TestDirectory::new();
        let transaction = "tx_service_checkpoint".to_owned();
        let target = ServiceTarget {
            name: "ExampleService".into(),
        };
        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .create_new_transaction(transaction.clone())
            .unwrap();
        let mut journal = ServiceJournal::new(storage, transaction.clone());
        SnapshotStore::persist(&mut journal, &target, &3, &4).unwrap();
        let mut storage = journal.into_storage();
        let records = storage.scan(&transaction).unwrap().typed_committed;
        drop(storage);

        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(&transaction)
            .unwrap();
        let mut checkpoint = storage
            .open_typed_recovery_checkpoint(records.clone(), Some("undo_service_checkpoint".into()))
            .unwrap();
        assert_eq!(checkpoint.next_exclusive(), 1);
        checkpoint.persist_next(0).unwrap();
        drop(checkpoint);

        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(&transaction)
            .unwrap();
        let checkpoint = storage
            .open_typed_recovery_checkpoint(records, None)
            .unwrap();
        assert_eq!(checkpoint.next_exclusive(), 0);
        assert_eq!(
            checkpoint.recovery_transaction_id(),
            "undo_service_checkpoint"
        );
    }

    #[derive(Default)]
    struct TypedServiceRecoveryMock {
        start_type: u32,
        writes: usize,
        root: PathBuf,
        recovery_id: String,
        backup_marker_visible_before_restore: bool,
    }

    #[test]
    fn typed_capacity_rotation_requires_explicit_retry_and_survives_reopen() {
        let directory = TestDirectory::new();
        let transaction = "typed_capacity_source";
        let target = ServiceTarget {
            name: "ExampleService".into(),
        };
        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .create_new_transaction(transaction)
            .unwrap();
        let mut source = ServiceJournal::new(storage, transaction.into());
        SnapshotStore::persist(&mut source, &target, &3, &4).unwrap();
        drop(source.into_storage());
        let mut session = FileTypedRecoverySession::start_with_id(
            &directory.0,
            transaction,
            "typed_capacity_old",
        )
        .unwrap();
        for _ in 0..10 {
            SnapshotStore::persist(
                &mut session.snapshots,
                &TypedTarget::Service(target.clone()),
                &TypedState::Service(4),
                &TypedState::Service(3),
            )
            .unwrap();
        }
        session
            .snapshots
            .test_storage_mut()
            .storage
            .preflight_limits = Some((1024, MAX_CHECKPOINT_BYTES));
        let mut backend = TypedServiceRecoveryMock {
            start_type: 4,
            root: directory.0.clone(),
            recovery_id: "typed_capacity_old".into(),
            ..Default::default()
        };
        assert!(matches!(
            session.run(&mut backend),
            Err(JournalRecoveryError::SnapshotRotatedRetryRequired)
        ));
        assert_eq!(backend.writes, 0);
        let new_id = session.recovery_transaction_id().to_owned();
        assert_ne!(new_id, "typed_capacity_old");
        assert_eq!(session.next_exclusive(), 1);
        drop(session);
        let mut session = FileTypedRecoverySession::open(&directory.0, transaction, None).unwrap();
        assert_eq!(session.recovery_transaction_id(), new_id);
        let mut backend = TypedServiceRecoveryMock {
            start_type: 4,
            root: directory.0.clone(),
            recovery_id: new_id,
            ..Default::default()
        };
        session.run(&mut backend).unwrap();
        assert_eq!(backend.writes, 1);
        assert!(backend.backup_marker_visible_before_restore);
        assert_eq!(session.next_exclusive(), 0);
    }

    #[test]
    fn failed_rotation_poisoned_until_reopen_even_after_marker_publish() {
        let directory = TestDirectory::new();
        let transaction = "typed_rotation_fault";
        let target = ServiceTarget {
            name: "ExampleService".into(),
        };
        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .create_new_transaction(transaction)
            .unwrap();
        let mut source = ServiceJournal::new(storage, transaction.into());
        SnapshotStore::persist(&mut source, &target, &3, &4).unwrap();
        drop(source.into_storage());
        let mut session = FileTypedRecoverySession::start_with_id(
            &directory.0,
            transaction,
            "typed_rotation_fault_old",
        )
        .unwrap();
        session.checkpoint.storage.storage.fault_injector =
            Some(fault_on_nth(IoBoundary::CheckpointMarkerFinalReadback, 1));
        let capacity: JournalRecoveryResult<&'static str, JournalError<io::Error>, io::Error> = Err(
            JournalRecoveryError::Recovery(BatchRecoveryError::Operation {
                index: 0,
                error: RecoveryError::Persist(JournalError::Preflight(capacity_error())),
            }),
        );
        assert!(matches!(
            session.finish_run(capacity),
            Err(JournalRecoveryError::SnapshotRotation(_))
        ));
        let mut backend = TypedServiceRecoveryMock {
            start_type: 4,
            ..Default::default()
        };
        assert!(matches!(
            session.run(&mut backend),
            Err(JournalRecoveryError::SnapshotRotation(_))
        ));
        assert_eq!(backend.writes, 0);
        drop(session);
        let reopened = FileTypedRecoverySession::open(&directory.0, transaction, None).unwrap();
        assert_ne!(
            reopened.recovery_transaction_id(),
            "typed_rotation_fault_old"
        );
        assert_eq!(reopened.next_exclusive(), 1);
    }

    #[test]
    fn recovery_does_not_rotate_for_other_failures() {
        for failure in ["io", "individual", "corruption", "restore"] {
            let directory = TestDirectory::new();
            let transaction = "rotation_non_capacity";
            let target = ServiceTarget {
                name: "ExampleService".into(),
            };
            let storage = FileJournalStorage::open(&directory.0)
                .unwrap()
                .create_new_transaction(transaction)
                .unwrap();
            let mut source = ServiceJournal::new(storage, transaction.into());
            SnapshotStore::persist(&mut source, &target, &3, &4).unwrap();
            drop(source.into_storage());
            let mut session = FileTypedRecoverySession::start_with_id(
                &directory.0,
                transaction,
                "rotation_non_capacity_backup",
            )
            .unwrap();
            match failure {
                "io" => {
                    session.snapshots.test_storage_mut().storage.fault_injector =
                        Some(fault_on_nth(IoBoundary::Preflight, 1))
                }
                "individual" => {
                    session
                        .snapshots
                        .test_storage_mut()
                        .storage
                        .preflight_limits = Some((1, MAX_CHECKPOINT_BYTES))
                }
                "corruption" => {
                    SnapshotStore::persist(
                        &mut session.snapshots,
                        &TypedTarget::Service(target.clone()),
                        &TypedState::Service(4),
                        &TypedState::Service(3),
                    )
                    .unwrap();
                    let path = session
                        .snapshots
                        .test_storage_mut()
                        .storage
                        .record_path("rotation_non_capacity_backup", 0)
                        .unwrap();
                    fs::write(path, b"corrupt").unwrap();
                }
                _ => {}
            }
            let mut backend = TypedServiceRecoveryMock {
                start_type: 4,
                ..Default::default()
            };
            if failure == "restore" {
                // Check classification of an uncertain restore outcome directly.
                let result: JournalRecoveryResult<
                    &'static str,
                    JournalError<io::Error>,
                    io::Error,
                > = Err(JournalRecoveryError::Recovery(
                    BatchRecoveryError::Operation {
                        index: 0,
                        error: RecoveryError::Restore("uncertain"),
                    },
                ));
                assert!(matches!(
                    session.finish_run(result),
                    Err(JournalRecoveryError::Recovery(_))
                ));
            } else {
                assert!(matches!(
                    session.run(&mut backend),
                    Err(JournalRecoveryError::Recovery(_))
                ));
                assert_eq!(backend.writes, 0);
            }
            assert_eq!(
                session.recovery_transaction_id(),
                "rotation_non_capacity_backup"
            );
            assert_eq!(session.next_exclusive(), 1);
        }
    }

    impl TypedRecoveryBackend for TypedServiceRecoveryMock {
        type Error = &'static str;

        fn query_typed(
            &mut self,
            target: &crate::data::store::TypedTarget,
        ) -> Result<crate::data::store::TypedState, Self::Error> {
            match target {
                crate::data::store::TypedTarget::Service(_) => {
                    Ok(crate::data::store::TypedState::Service(self.start_type))
                }
                crate::data::store::TypedTarget::Registry(_) => Err("unsupported resource"),
            }
        }

        fn restore_typed(
            &mut self,
            target: &crate::data::store::TypedTarget,
            state: &crate::data::store::TypedState,
        ) -> Result<(), Self::Error> {
            match (target, state) {
                (
                    crate::data::store::TypedTarget::Service(_),
                    crate::data::store::TypedState::Service(start_type),
                ) => {
                    let marker = self.root.join(format!(
                        "{:03}-{}-{:020}.commit",
                        self.recovery_id.len(),
                        self.recovery_id,
                        self.writes
                    ));
                    self.backup_marker_visible_before_restore = marker.is_file();
                    self.writes += 1;
                    self.start_type = *start_type;
                    Ok(())
                }
                _ => Err("unsupported resource"),
            }
        }
    }

    #[test]
    fn file_backed_typed_recovery_commits_service_snapshot_before_restore_and_progress() {
        let directory = TestDirectory::new();
        let transaction = "tx_typed_recovery_source".to_owned();
        let recovery_id = "tx_typed_recovery_backup".to_owned();
        let target = ServiceTarget {
            name: "ExampleService".into(),
        };

        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .create_new_transaction(transaction.clone())
            .unwrap();
        let mut source = ServiceJournal::new(storage, transaction.clone());
        SnapshotStore::persist(&mut source, &target, &3, &4).unwrap();
        drop(source.into_storage());

        let session = FileTypedRecoverySession::start_with_id(
            &directory.0,
            transaction.clone(),
            recovery_id.clone(),
        )
        .unwrap();
        assert_eq!(session.next_exclusive(), 1);
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_system_mutations()
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(session);

        let mut interrupted_snapshot = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(&recovery_id)
            .unwrap();
        let incomplete = TransactionRecord::Service(ServiceRecord {
            version: SERVICE_FORMAT_VERSION,
            transaction_id: recovery_id.clone(),
            sequence: 0,
            target: target.clone(),
            before_start_type: 4,
            intended_start_type: 3,
        });
        interrupted_snapshot
            .create_new(
                &recovery_id,
                0,
                &encode_transaction_record(&incomplete).unwrap(),
            )
            .unwrap();
        drop(interrupted_snapshot);

        let mut session =
            FileTypedRecoverySession::resume(&directory.0, transaction.clone()).unwrap();
        let active_recovery_id = session.recovery_transaction_id().to_owned();
        assert_ne!(active_recovery_id, recovery_id);
        assert_eq!(session.next_exclusive(), 1);
        let mut backend = TypedServiceRecoveryMock {
            start_type: 4,
            root: directory.0.clone(),
            recovery_id: active_recovery_id.clone(),
            ..Default::default()
        };

        session.run(&mut backend).unwrap();

        assert_eq!(backend.start_type, 3);
        assert_eq!(backend.writes, 1);
        assert!(backend.backup_marker_visible_before_restore);
        assert_eq!(session.next_exclusive(), 0);
        drop(session);

        let session = FileTypedRecoverySession::resume(&directory.0, transaction).unwrap();
        assert_eq!(session.recovery_transaction_id(), active_recovery_id);
        assert_eq!(session.next_exclusive(), 0);
        drop(session);

        let mut interrupted_snapshot = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(&recovery_id)
            .unwrap();
        assert_eq!(
            interrupted_snapshot
                .scan(&recovery_id)
                .unwrap()
                .incomplete_tail,
            Some(0)
        );
        drop(interrupted_snapshot);
        let mut snapshot_storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(&active_recovery_id)
            .unwrap();
        let snapshot_scan = snapshot_storage.scan(&active_recovery_id).unwrap();
        assert!(matches!(
            &snapshot_scan.typed_committed[..],
            [TransactionRecord::Service(record)]
                if record.before_start_type == 4 && record.intended_start_type == 3
        ));
    }

    #[derive(Default)]
    struct FaultMatrixState {
        state: u32,
        changes: usize,
        backup_marker_seen: bool,
    }

    struct FaultMatrixScm {
        shared: Arc<Mutex<FaultMatrixState>>,
        root: PathBuf,
        recovery_id: Arc<Mutex<String>>,
    }

    impl crate::engine::services::ScmApi for FaultMatrixScm {
        fn query_start_type(
            &mut self,
            _: &str,
        ) -> Result<u32, crate::engine::services::MutationError> {
            Ok(self.shared.lock().unwrap().state)
        }

        fn change_start_type(
            &mut self,
            _: &str,
            start_type: u32,
        ) -> Result<(), crate::engine::services::MutationError> {
            let recovery_id = self.recovery_id.lock().unwrap().clone();
            let backup_marker_seen = (0..MAX_TRANSACTION_RECORDS as u64).any(|sequence| {
                self.root
                    .join(format!(
                        "{:03}-{recovery_id}-{sequence:020}.commit",
                        recovery_id.len()
                    ))
                    .is_file()
            });
            let mut shared = self.shared.lock().unwrap();
            shared.backup_marker_seen = backup_marker_seen;
            shared.changes += 1;
            shared.state = start_type;
            Ok(())
        }
    }

    fn typed_recovery_session_with_fault(
        root: &Path,
        transaction: &str,
        recovery_id: &str,
        injector: Arc<dyn IoFaultInjector>,
    ) -> FileTypedRecoverySession {
        let root_storage = FileJournalStorage::open_with_fault_injector(root, injector).unwrap();
        let mut original = root_storage
            .try_clone()
            .unwrap()
            .lock_transaction(transaction)
            .unwrap();
        let records = original.scan(transaction).unwrap().typed_committed;
        let mutation_lock = root_storage
            .try_clone()
            .unwrap()
            .lock_system_mutations()
            .unwrap();
        let snapshot_storage = root_storage
            .try_clone()
            .unwrap()
            .create_new_transaction(recovery_id.to_owned())
            .unwrap();
        let checkpoint = original
            .open_typed_recovery_checkpoint(records.clone(), Some(recovery_id.to_owned()))
            .unwrap();

        FileTypedRecoverySession {
            records,
            checkpoint,
            snapshots: TypedJournal::new(snapshot_storage, recovery_id.to_owned()),
            _root_storage: root_storage,
            _mutation_lock: mutation_lock,
        }
    }

    #[test]
    fn typed_recovery_fault_matrix_with_mock_scm_reopens_without_unbacked_restore() {
        let boundaries = [
            IoBoundary::RecordCreate,
            IoBoundary::RecordWrite,
            IoBoundary::RecordFlush,
            IoBoundary::RecordSyncFlush,
            IoBoundary::RecordReadback,
            IoBoundary::CommitMarkerCreate,
            IoBoundary::CommitMarkerWrite,
            IoBoundary::CommitMarkerFlush,
            IoBoundary::CommitMarkerReadback,
            IoBoundary::CommitMarkerPublish,
            IoBoundary::CommitMarkerFinalReadback,
            IoBoundary::CheckpointCreate,
            IoBoundary::CheckpointWrite,
            IoBoundary::CheckpointFlush,
            IoBoundary::CheckpointReadback,
            IoBoundary::CheckpointMarkerCreate,
            IoBoundary::CheckpointMarkerWrite,
            IoBoundary::CheckpointMarkerFlush,
            IoBoundary::CheckpointMarkerReadback,
            IoBoundary::CheckpointMarkerPublish,
            IoBoundary::CheckpointMarkerFinalReadback,
        ];

        for (index, boundary) in boundaries.into_iter().enumerate() {
            let directory = TestDirectory::new();
            let transaction = format!("tx_scm_fault_{index}");
            let recovery_id = format!("tx_scm_undo_{index}");
            let target = ServiceTarget {
                name: "ExampleService".into(),
            };
            let source_storage = FileJournalStorage::open(&directory.0)
                .unwrap()
                .create_new_transaction(transaction.clone())
                .unwrap();
            let mut source = ServiceJournal::new(source_storage, transaction.clone());
            SnapshotStore::persist(&mut source, &target, &3, &4).unwrap();
            drop(source.into_storage());

            let checkpoint_boundary = matches!(
                boundary,
                IoBoundary::CheckpointCreate
                    | IoBoundary::CheckpointWrite
                    | IoBoundary::CheckpointFlush
                    | IoBoundary::CheckpointReadback
                    | IoBoundary::CheckpointMarkerCreate
                    | IoBoundary::CheckpointMarkerWrite
                    | IoBoundary::CheckpointMarkerFlush
                    | IoBoundary::CheckpointMarkerReadback
                    | IoBoundary::CheckpointMarkerPublish
                    | IoBoundary::CheckpointMarkerFinalReadback
            );
            let injector = fault_on_nth(boundary, if checkpoint_boundary { 2 } else { 1 });
            let mut recovery = typed_recovery_session_with_fault(
                &directory.0,
                &transaction,
                &recovery_id,
                injector,
            );
            let current_recovery_id = Arc::new(Mutex::new(recovery_id.clone()));
            let shared = Arc::new(Mutex::new(FaultMatrixState {
                state: 4,
                ..Default::default()
            }));
            let mut backend = crate::engine::services::TypedScmBackend::new(FaultMatrixScm {
                shared: Arc::clone(&shared),
                root: directory.0.clone(),
                recovery_id: Arc::clone(&current_recovery_id),
            });

            assert!(
                recovery.run(&mut backend).is_err(),
                "fault {boundary:?} was not reached"
            );
            if checkpoint_boundary {
                assert_eq!(
                    shared.lock().unwrap().state,
                    3,
                    "restore should precede {boundary:?}"
                );
                assert_eq!(shared.lock().unwrap().changes, 1);
            } else {
                assert_eq!(
                    shared.lock().unwrap().state,
                    4,
                    "restore must not precede {boundary:?}"
                );
                assert_eq!(shared.lock().unwrap().changes, 0);
            }
            drop(recovery);

            let mut recovery =
                FileTypedRecoverySession::resume(&directory.0, &transaction).unwrap();
            *current_recovery_id.lock().unwrap() = recovery.recovery_transaction_id().to_owned();
            recovery.run(&mut backend).unwrap();
            assert_eq!(
                shared.lock().unwrap().state,
                3,
                "recovery did not finish after {boundary:?}"
            );
            assert_eq!(
                shared.lock().unwrap().changes,
                1,
                "restore repeated after {boundary:?}"
            );
            assert!(shared.lock().unwrap().backup_marker_seen);
            assert_eq!(recovery.next_exclusive(), 0);
            let active_recovery_id = recovery.recovery_transaction_id().to_owned();
            drop(recovery);

            let reopened = FileTypedRecoverySession::resume(&directory.0, &transaction)
                .unwrap_or_else(|error| panic!("reopen after {boundary:?} failed: {error:?}"));
            assert_eq!(
                reopened.next_exclusive(),
                0,
                "checkpoint reopen failed after {boundary:?}"
            );
            assert_eq!(reopened.recovery_transaction_id(), active_recovery_id);
        }
    }

    struct TypedApplyMock {
        root: PathBuf,
        transaction: String,
        current: TypedState,
        apply_saw_committed_backup: bool,
        fail_apply: bool,
    }

    impl Backend for TypedApplyMock {
        type Target = TypedTarget;
        type Change = TypedState;
        type Snapshot = TypedState;
        type Error = &'static str;

        fn query(&mut self, _: &TypedTarget) -> Result<TypedState, Self::Error> {
            Ok(self.current.clone())
        }

        fn apply(&mut self, _: &TypedTarget, change: &TypedState) -> Result<(), Self::Error> {
            let marker = self.root.join(format!(
                "{:03}-{}-{:020}.commit",
                self.transaction.len(),
                self.transaction,
                0
            ));
            self.apply_saw_committed_backup = marker.is_file();
            self.current = change.clone();
            if self.fail_apply {
                Err("partial apply")
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn typed_apply_session_commits_backup_before_apply_and_retains_partial_failure() {
        let directory = TestDirectory::new();
        let transaction = "tx_typed_apply".to_owned();
        let mut session =
            FileJournalStorage::start_typed_transaction(&directory.0, transaction.clone()).unwrap();
        let mut backend = TypedApplyMock {
            root: directory.0.clone(),
            transaction: transaction.clone(),
            current: TypedState::Service(3),
            apply_saw_committed_backup: false,
            fail_apply: true,
        };
        let operation = Operation {
            target: TypedTarget::Service(ServiceTarget {
                name: "ExampleService".into(),
            }),
            change: TypedState::Service(4),
        };

        assert!(matches!(
            session.apply(&mut backend, &operation),
            Err(FileTypedTransactionError::Operation(
                TransactionError::Apply("partial apply")
            ))
        ));
        assert!(backend.apply_saw_committed_backup);
        assert_eq!(backend.current, TypedState::Service(4));
        drop(session);

        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(&transaction)
            .unwrap();
        let scan = storage.scan(&transaction).unwrap();
        assert_eq!(scan.typed_committed.len(), 1);
        assert!(matches!(
            &scan.typed_committed[0],
            TransactionRecord::Service(record)
                if record.before_start_type == 3 && record.intended_start_type == 4
        ));
    }

    #[test]
    fn file_scan_keeps_mixed_tagged_records_in_order_and_registry_writer_rejects_services() {
        let directory = TestDirectory::new();
        let transaction = "tx_service_record";
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(transaction)
            .unwrap();
        let registry_record = RegistryRecord {
            version: crate::data::journal::FORMAT_VERSION,
            transaction_id: transaction.into(),
            sequence: 0,
            target: ValueTarget {
                hive: crate::engine::registry::Hive::LocalMachine,
                view: crate::engine::registry::RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Setting".into(),
            },
            before: ValueState::ValueMissing,
            intended: ValueState::Present(RegistryValue {
                native_type: 4,
                bytes: vec![4],
            }),
        };
        let service_record = ServiceRecord {
            version: SERVICE_FORMAT_VERSION,
            transaction_id: transaction.into(),
            sequence: 1,
            target: ServiceTarget {
                name: "ExampleService".into(),
            },
            before_start_type: 3,
            intended_start_type: 4,
        };
        storage
            .create_new(
                transaction,
                0,
                &encode_transaction_record(&TransactionRecord::Registry(registry_record.clone()))
                    .unwrap(),
            )
            .unwrap();
        storage.sync(transaction, 0).unwrap();
        storage
            .create_new(
                transaction,
                1,
                &encode_transaction_record(&TransactionRecord::Service(service_record.clone()))
                    .unwrap(),
            )
            .unwrap();
        storage.sync(transaction, 1).unwrap();
        let scan = storage.scan(transaction).unwrap();
        assert_eq!(scan.committed, vec![registry_record]);
        assert_eq!(
            scan.typed_committed,
            vec![
                TransactionRecord::Registry(scan.committed[0].clone()),
                TransactionRecord::Service(service_record),
            ]
        );
        assert!(matches!(
            RegistryJournal::resume(storage, transaction.into()),
            Err(JournalError::UnsupportedRecordKind)
        ));

        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(transaction)
            .unwrap();
        let error = storage
            .open_recovery_checkpoint(scan.committed, None)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[derive(Debug)]
    struct FailOnce(Mutex<Option<IoBoundary>>);

    impl IoFaultInjector for FailOnce {
        fn fail_at(&self, boundary: IoBoundary) -> bool {
            let mut selected = self.0.lock().unwrap();
            if *selected == Some(boundary) {
                *selected = None;
                true
            } else {
                false
            }
        }
    }

    fn fault(boundary: IoBoundary) -> Arc<dyn IoFaultInjector> {
        Arc::new(FailOnce(Mutex::new(Some(boundary))))
    }

    #[derive(Debug)]
    struct FailOnNth {
        boundary: IoBoundary,
        remaining: Mutex<usize>,
    }

    impl IoFaultInjector for FailOnNth {
        fn fail_at(&self, boundary: IoBoundary) -> bool {
            if boundary != self.boundary {
                return false;
            }
            let mut remaining = self.remaining.lock().unwrap();
            if *remaining > 1 {
                *remaining -= 1;
                false
            } else {
                *remaining = 0;
                true
            }
        }
    }

    fn fault_on_nth(boundary: IoBoundary, occurrence: usize) -> Arc<dyn IoFaultInjector> {
        assert!(occurrence > 0);
        Arc::new(FailOnNth {
            boundary,
            remaining: Mutex::new(occurrence),
        })
    }

    #[derive(Debug)]
    struct SignalAndPauseAtBoundary {
        boundary: IoBoundary,
        occurrence: usize,
        seen: Mutex<usize>,
        endpoint: std::net::SocketAddr,
    }

    impl IoFaultInjector for SignalAndPauseAtBoundary {
        fn fail_at(&self, boundary: IoBoundary) -> bool {
            if boundary != self.boundary {
                return false;
            }
            let mut seen = self.seen.lock().unwrap();
            *seen += 1;
            if *seen != self.occurrence {
                return false;
            }
            drop(seen);

            use std::io::Write as _;
            let mut signal = std::net::TcpStream::connect(self.endpoint)
                .expect("connect to the external journal crash controller");
            writeln!(signal, "{boundary:?}").expect("signal the external journal crash controller");
            drop(signal);
            loop {
                std::thread::park_timeout(std::time::Duration::from_secs(60));
            }
        }
    }

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn unique_path() -> PathBuf {
            let unique = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before unix epoch")
                .as_nanos();
            let profile =
                std::env::var_os("USERPROFILE").expect("USERPROFILE is required for journal tests");
            PathBuf::from(profile).join(format!(
                "rustweaker-journal-test-{}-{stamp}-{unique}",
                std::process::id()
            ))
        }

        fn new() -> Self {
            let path = Self::unique_path();
            FileJournalStorage::create_private(&path)
                .expect("create isolated protected journal test directory");
            Self(path)
        }

        fn unprotected() -> Self {
            let path = Self::unique_path();
            fs::create_dir(&path).expect("create isolated unprotected test directory");
            Self(path)
        }

        fn with_sddl(sddl: &str) -> Self {
            let path = Self::unique_path();
            super::super::windows_security::create_test_directory_with_sddl(&path, sddl)
                .expect("create isolated ACL test directory");
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exclusive_create_syncs_and_reads_exact_bytes() {
        let directory = TestDirectory::new();
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_a0")
            .unwrap();
        let record = RegistryRecord {
            version: super::super::journal::FORMAT_VERSION,
            transaction_id: "tx_a0".into(),
            sequence: 0,
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Test".into(),
            },
            before: ValueState::ValueMissing,
            intended: ValueState::ValueMissing,
        };
        let expected = super::super::journal::encode(&record).unwrap();
        storage.create_new("tx_a0", 0, &expected).unwrap();
        storage.sync("tx_a0", 0).unwrap();
        assert_eq!(storage.read("tx_a0", 0).unwrap(), expected);
        assert_eq!(
            storage
                .create_new("tx_a0", 0, b"replacement")
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(storage.read("tx_a0", 0).unwrap(), expected);
    }

    #[test]
    fn rejects_traversal_ids_and_non_directories() {
        let directory = TestDirectory::new();
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("../escape")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let missing = TestDirectory::unique_path()
            .join("missing-parent")
            .join("missing");
        assert_eq!(
            FileJournalStorage::open(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            FileJournalStorage::create_private(&missing)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(!missing.exists());
    }

    #[test]
    fn private_root_creation_refuses_existing_directories_without_repairing_them() {
        let directory = TestDirectory::new();
        let error = FileJournalStorage::create_private(&directory.0).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(directory.0.is_dir());
    }

    #[test]
    fn open_rejects_a_root_without_the_private_dacl() {
        let directory = TestDirectory::unprotected();

        let error = FileJournalStorage::open(&directory.0).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(directory.0.is_dir());
    }

    #[test]
    fn open_rejects_an_existing_reparse_point_in_the_path() {
        let profile =
            std::env::var_os("USERPROFILE").expect("USERPROFILE is required for journal tests");
        let junction = PathBuf::from(profile).join("Application Data");
        let metadata = match fs::symlink_metadata(&junction) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                eprintln!("skipped reparse ACL test: no known profile junction exists");
                return;
            }
            Err(error) => panic!("cannot inspect profile junction: {error}"),
        };
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            eprintln!("skipped reparse ACL test: profile alias is not a reparse directory");
            return;
        }

        let error = FileJournalStorage::open(&junction).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::InvalidInput | io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn private_root_creation_rejects_parent_delete_child_grants_to_users() {
        let parent = TestDirectory::with_sddl("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BU)");
        let root = parent.0.join("journal");

        let error = FileJournalStorage::create_private(&root).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!root.exists());
    }

    #[test]
    fn private_root_creation_removes_a_leaf_that_fails_post_create_validation() {
        let root = TestDirectory::unique_path();

        let error =
            super::super::windows_security::create_test_root_with_invalid_dacl(&root).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!root.exists());
    }

    #[test]
    fn trusted_profile_owner_can_replace_the_root_even_while_storage_is_open() {
        struct RenameBack {
            moved: PathBuf,
            original: PathBuf,
        }
        impl Drop for RenameBack {
            fn drop(&mut self) {
                if self.moved.exists() && !self.original.exists() {
                    let _ = fs::rename(&self.moved, &self.original);
                }
            }
        }

        let directory = TestDirectory::new();
        let storage = FileJournalStorage::open(&directory.0).unwrap();
        let moved = TestDirectory::unique_path();
        let restore = RenameBack {
            moved: moved.clone(),
            original: directory.0.clone(),
        };

        fs::rename(&directory.0, &moved).unwrap();
        assert!(moved.is_dir());
        assert!(!directory.0.exists());

        drop(storage);
        drop(restore);
        assert!(directory.0.is_dir());
    }

    #[test]
    fn journal_io_stays_on_retained_root_handle_after_path_replacement() {
        let root = TestDirectory::unique_path();
        let moved = TestDirectory::unique_path();
        let storage = FileJournalStorage::create_private(&root).unwrap();
        fs::rename(&root, &moved).unwrap();
        let replacement = FileJournalStorage::create_private(&root).unwrap();
        drop(replacement);

        let transaction = "handle_anchor";
        let mut journal = RegistryJournal::new(
            storage.create_new_transaction(transaction).unwrap(),
            transaction.to_owned(),
        );
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        drop(journal.into_storage());

        let names: Vec<_> = fs::read_dir(&moved)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|name| name == "013-handle_anchor.lock"));
        assert!(names
            .iter()
            .any(|name| name == "013-handle_anchor-00000000000000000000.json"));
        assert!(names
            .iter()
            .any(|name| name == "013-handle_anchor-00000000000000000000.commit"));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(&moved).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn journal_io_stays_on_retained_root_handle_after_ancestor_replacement() {
        let parent = TestDirectory::unique_path();
        let moved_parent = TestDirectory::unique_path();
        let root = parent.join("journal");
        let moved_root = moved_parent.join("journal");
        fs::create_dir(&parent).unwrap();
        let storage = FileJournalStorage::create_private(&root).unwrap();

        let ancestor_rename_succeeded = fs::rename(&parent, &moved_parent).is_ok();
        let original_root = if ancestor_rename_succeeded {
            &moved_root
        } else {
            &root
        };
        if ancestor_rename_succeeded {
            fs::create_dir(&parent).unwrap();
            drop(FileJournalStorage::create_private(parent.join("journal")).unwrap());
        }

        let transaction = "ancestor_anchor";
        let mut journal = RegistryJournal::new(
            storage.create_new_transaction(transaction).unwrap(),
            transaction.to_owned(),
        );
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        drop(journal.into_storage());

        let names: Vec<_> = fs::read_dir(original_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let prefix = format!("{:03}-{transaction}", transaction.len());
        assert!(names.iter().any(|name| name == &format!("{prefix}.lock")));
        assert!(names
            .iter()
            .any(|name| name == &format!("{prefix}-00000000000000000000.json")));
        assert!(names
            .iter()
            .any(|name| name == &format!("{prefix}-00000000000000000000.commit")));
        if ancestor_rename_succeeded {
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
            fs::remove_dir_all(&moved_parent).unwrap();
            fs::remove_dir_all(&parent).unwrap();
        } else {
            fs::remove_dir_all(&parent).unwrap();
        }
    }

    #[test]
    fn transaction_session_keeps_root_locked_and_persists_its_backup() {
        struct BackendMock(ValueState);
        impl Backend for BackendMock {
            type Target = ValueTarget;
            type Change = ValueState;
            type Snapshot = ValueState;
            type Error = &'static str;

            fn query(&mut self, _: &ValueTarget) -> Result<ValueState, Self::Error> {
                Ok(self.0.clone())
            }

            fn apply(&mut self, _: &ValueTarget, change: &ValueState) -> Result<(), Self::Error> {
                self.0 = change.clone();
                Ok(())
            }
        }

        let root = TestDirectory::unique_path();
        let moved = TestDirectory::unique_path();
        let transaction = "session_handle_anchor";
        drop(FileJournalStorage::create_private(&root).unwrap());
        let mut session = FileJournalStorage::start_transaction(&root, transaction).unwrap();
        assert!(fs::rename(&root, &moved).is_err());

        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let intended = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1, 0, 0, 0],
        });
        let mut backend = BackendMock(ValueState::ValueMissing);
        session
            .apply(
                &mut backend,
                &Operation {
                    target,
                    change: intended.clone(),
                },
            )
            .unwrap();
        assert_eq!(backend.0, intended);
        drop(session);

        let names: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let prefix = format!("{:03}-{transaction}", transaction.len());
        assert!(names.iter().any(|name| name == &format!("{prefix}.lock")));
        assert!(names
            .iter()
            .any(|name| name == &format!("{prefix}-00000000000000000000.json")));
        assert!(names
            .iter()
            .any(|name| name == &format!("{prefix}-00000000000000000000.commit")));
        assert!(!moved.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn anonymous_token_cannot_replace_the_root_while_storage_is_open() {
        use windows_sys::Win32::Security::{ImpersonateAnonymousToken, RevertToSelf};
        use windows_sys::Win32::System::Threading::GetCurrentThread;

        struct RevertImpersonation;
        impl Drop for RevertImpersonation {
            fn drop(&mut self) {
                unsafe { RevertToSelf() };
            }
        }

        let directory = TestDirectory::new();
        let storage = FileJournalStorage::open(&directory.0).unwrap();
        let moved = TestDirectory::unique_path();

        if unsafe { ImpersonateAnonymousToken(GetCurrentThread()) } == 0 {
            panic!(
                "cannot run anonymous path-replacement check; impersonation is required: {}",
                io::Error::last_os_error()
            );
        }
        let revert = RevertImpersonation;
        let error = fs::rename(&directory.0, &moved).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        let error = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.0.join("anonymous-entry"))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        drop(revert);
        assert!(directory.0.is_dir());
        assert!(!moved.exists());
        drop(storage);
    }

    #[test]
    fn restricted_user_token_cannot_replace_the_root_while_storage_is_open() {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Security::{
            CreateRestrictedToken, CreateWellKnownSid, ImpersonateLoggedOnUser, RevertToSelf,
            WinBuiltinGuestsSid, SID_AND_ATTRIBUTES, TOKEN_DUPLICATE, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        struct OwnedToken(HANDLE);
        impl Drop for OwnedToken {
            fn drop(&mut self) {
                if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                    unsafe { CloseHandle(self.0) };
                }
            }
        }
        struct RevertImpersonation;
        impl Drop for RevertImpersonation {
            fn drop(&mut self) {
                unsafe { RevertToSelf() };
            }
        }

        let directory = TestDirectory::new();
        let storage = FileJournalStorage::open(&directory.0).unwrap();
        let moved = TestDirectory::unique_path();

        let mut primary = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                OpenProcessToken(
                    GetCurrentProcess(),
                    TOKEN_DUPLICATE | TOKEN_QUERY,
                    &mut primary,
                )
            },
            0,
            "open process token: {}",
            io::Error::last_os_error()
        );
        let primary = OwnedToken(primary);

        let mut guest_sid = [0usize; 16];
        let mut sid_bytes = std::mem::size_of_val(&guest_sid) as u32;
        assert_ne!(
            unsafe {
                CreateWellKnownSid(
                    WinBuiltinGuestsSid,
                    std::ptr::null_mut(),
                    guest_sid.as_mut_ptr().cast(),
                    &mut sid_bytes,
                )
            },
            0,
            "create Guests SID: {}",
            io::Error::last_os_error()
        );
        let restricted_sid = SID_AND_ATTRIBUTES {
            Sid: guest_sid.as_mut_ptr().cast(),
            Attributes: 0,
        };
        let mut restricted = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                CreateRestrictedToken(
                    primary.0,
                    0,
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    1,
                    &restricted_sid,
                    &mut restricted,
                )
            },
            0,
            "create restricted token: {}",
            io::Error::last_os_error()
        );
        let restricted = OwnedToken(restricted);
        assert_ne!(
            unsafe { ImpersonateLoggedOnUser(restricted.0) },
            0,
            "impersonate restricted token: {}",
            io::Error::last_os_error()
        );
        let revert = RevertImpersonation;

        let error = fs::rename(&directory.0, &moved).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        let error = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.0.join("restricted-entry"))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        drop(revert);
        assert!(directory.0.is_dir());
        assert!(!moved.exists());
        drop(storage);
    }

    #[test]
    fn restricted_users_can_create_parent_file_but_not_journal_file() {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Security::{
            CreateRestrictedToken, CreateWellKnownSid, ImpersonateLoggedOnUser, RevertToSelf,
            WinBuiltinUsersSid, SID_AND_ATTRIBUTES, TOKEN_DUPLICATE, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        struct OwnedToken(HANDLE);
        impl Drop for OwnedToken {
            fn drop(&mut self) {
                if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                    unsafe { CloseHandle(self.0) };
                }
            }
        }
        struct RevertImpersonation;
        impl Drop for RevertImpersonation {
            fn drop(&mut self) {
                unsafe { RevertToSelf() };
            }
        }

        let parent = TestDirectory::with_sddl(
            "D:P(A;;FA;;;SY)(A;;0x00120022;;;BU)(A;OIIO;0x00120116;;;BU)(A;;FA;;;OW)",
        );
        let root = parent.0.join("journal");
        let storage = FileJournalStorage::create_private(&root)
            .expect("controlled parent should pass journal-root policy");

        let mut primary = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                OpenProcessToken(
                    GetCurrentProcess(),
                    TOKEN_DUPLICATE | TOKEN_QUERY,
                    &mut primary,
                )
            },
            0,
            "open process token: {}",
            io::Error::last_os_error()
        );
        let primary = OwnedToken(primary);

        let mut users_sid = [0usize; 16];
        let mut sid_bytes = std::mem::size_of_val(&users_sid) as u32;
        assert_ne!(
            unsafe {
                CreateWellKnownSid(
                    WinBuiltinUsersSid,
                    std::ptr::null_mut(),
                    users_sid.as_mut_ptr().cast(),
                    &mut sid_bytes,
                )
            },
            0,
            "create Users SID: {}",
            io::Error::last_os_error()
        );
        let restricted_sid = SID_AND_ATTRIBUTES {
            Sid: users_sid.as_mut_ptr().cast(),
            Attributes: 0,
        };
        let mut restricted = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                CreateRestrictedToken(
                    primary.0,
                    0,
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    1,
                    &restricted_sid,
                    &mut restricted,
                )
            },
            0,
            "create restricted token: {}",
            io::Error::last_os_error()
        );
        let restricted = OwnedToken(restricted);
        assert_ne!(
            unsafe { ImpersonateLoggedOnUser(restricted.0) },
            0,
            "impersonate restricted token: {}",
            io::Error::last_os_error()
        );
        let revert = RevertImpersonation;

        let parent_file = parent.0.join("restricted-control");
        let mut control = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&parent_file)
            .expect("restricted Users token should create a file in parent");
        control.write_all(b"test").unwrap();
        drop(control);

        let root_entry = root.join("restricted-entry");
        let error = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&root_entry)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

        drop(revert);
        let root_entry_was_created = root_entry.exists();
        if root_entry_was_created {
            fs::remove_file(&root_entry).expect("clean up unexpected journal child file");
        }
        fs::remove_file(&parent_file).expect("remove restricted-token control file");
        assert!(
            !root_entry_was_created,
            "denied journal file creation must not leave a file behind"
        );
        assert!(root.is_dir());
        drop(storage);
    }

    #[test]
    fn new_transaction_id_cannot_claim_existing_log_data_without_a_lock_file() {
        let directory = TestDirectory::new();
        let storage = FileJournalStorage::open(&directory.0).unwrap();
        let record_path = storage.record_path("tx_claimed", 0).unwrap();
        fs::write(record_path, b"uncommitted prior data").unwrap();
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .create_new_transaction("tx_claimed")
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn refuses_oversized_records_before_creating_a_file() {
        let directory = TestDirectory::new();
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx")
            .unwrap();
        let bytes = vec![0; MAX_RECORD_BYTES + 1];
        assert_eq!(
            storage.create_new("tx", 0, &bytes).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(fs::read_dir(&directory.0).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".json")));
    }

    #[test]
    fn committed_legacy_service_record_scans_and_opens_checkpoint() {
        let directory = TestDirectory::new();
        let transaction = "tx_legacy";
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(transaction)
            .unwrap();
        let bytes = br#"{"record_type":"service","payload":{"version":1,"transaction_id":"tx_legacy","sequence":0,"target":{"name":"old service"},"before_start_type":5,"intended_start_type":3}}"#;
        // Model bytes and marker produced by the old writer without using today's sync.
        storage.create_new(transaction, 0, bytes).unwrap();
        let marker = CommitMarker {
            version: COMMIT_MARKER_VERSION,
            transaction_id: transaction.into(),
            sequence: 0,
            record_crc32: crc32(bytes),
        };
        fs::write(
            storage.storage.commit_path(transaction, 0).unwrap(),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        let records = storage.scan(transaction).unwrap().typed_committed;
        assert_eq!(records.len(), 1);
        let checkpoint = storage
            .open_typed_recovery_checkpoint(records, Some("legacy_recovery".into()))
            .unwrap();
        assert_eq!(checkpoint.next_exclusive(), 1);
    }

    #[test]
    fn direct_sync_does_not_publish_unsupported_new_service_record() {
        let directory = TestDirectory::new();
        let transaction = "tx_legacy";
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(transaction)
            .unwrap();
        let bytes = br#"{"record_type":"service","payload":{"version":1,"transaction_id":"tx_legacy","sequence":0,"target":{"name":"old service"},"before_start_type":5,"intended_start_type":3}}"#;
        storage.create_new(transaction, 0, bytes).unwrap();
        assert_eq!(
            storage.sync(transaction, 0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(!storage
            .storage
            .commit_path(transaction, 0)
            .unwrap()
            .exists());
        let scan = storage.scan(transaction).unwrap();
        assert!(scan.typed_committed.is_empty());
        assert_eq!(scan.incomplete_tail, Some(0));
    }

    #[test]
    fn direct_sync_checks_limits_before_publishing() {
        for limits in [(1, MAX_CHECKPOINT_BYTES), (MAX_TRANSACTION_BYTES, 1)] {
            let directory = TestDirectory::new();
            let transaction = "direct_limit";
            let mut storage = FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction(transaction)
                .unwrap();
            let record = TransactionRecord::Service(ServiceRecord {
                version: super::super::journal::SERVICE_FORMAT_VERSION,
                transaction_id: transaction.into(),
                sequence: 0,
                target: ServiceTarget {
                    name: "ExampleService".into(),
                },
                before_start_type: 3,
                intended_start_type: 4,
            });
            let bytes = super::super::journal::encode_transaction_record(&record).unwrap();
            storage.create_new(transaction, 0, &bytes).unwrap();
            storage.storage.preflight_limits = Some(limits);
            assert!(storage.sync(transaction, 0).is_err());
            assert!(!storage
                .storage
                .commit_path(transaction, 0)
                .unwrap()
                .exists());
            let scan = storage.scan(transaction).unwrap();
            assert!(scan.typed_committed.is_empty());
            assert_eq!(scan.incomplete_tail, Some(0));
        }
    }

    #[test]
    fn direct_sync_revalidates_staged_bytes_and_accepts_valid_record() {
        let directory = TestDirectory::new();
        let transaction = "direct_staged";
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(transaction)
            .unwrap();
        let record = TransactionRecord::Service(ServiceRecord {
            version: super::super::journal::SERVICE_FORMAT_VERSION,
            transaction_id: transaction.into(),
            sequence: 0,
            target: ServiceTarget {
                name: "ExampleService".into(),
            },
            before_start_type: 3,
            intended_start_type: 4,
        });
        let bytes = super::super::journal::encode_transaction_record(&record).unwrap();
        storage.storage.preflight_limits = Some((bytes.len() + 128, MAX_CHECKPOINT_BYTES));
        storage.preflight(transaction, 0, &bytes).unwrap();
        storage.create_new(transaction, 0, &bytes).unwrap();
        let path = storage.storage.record_path(transaction, 0).unwrap();
        let mut replacement = record.clone();
        if let TransactionRecord::Service(service) = &mut replacement {
            service.transaction_id = "different".into();
        }
        fs::write(
            &path,
            super::super::journal::encode_transaction_record(&replacement).unwrap(),
        )
        .unwrap();
        assert!(storage.sync(transaction, 0).is_err());
        assert!(!storage
            .storage
            .commit_path(transaction, 0)
            .unwrap()
            .exists());
        if let TransactionRecord::Service(service) = &mut replacement {
            service.transaction_id = transaction.into();
            service.target.name = "s".repeat(200);
        }
        fs::write(
            &path,
            super::super::journal::encode_transaction_record(&replacement).unwrap(),
        )
        .unwrap();
        assert!(storage.sync(transaction, 0).is_err());
        assert!(!storage
            .storage
            .commit_path(transaction, 0)
            .unwrap()
            .exists());
        fs::write(&path, &bytes).unwrap();
        storage.sync(transaction, 0).unwrap();
        assert_eq!(
            storage.scan(transaction).unwrap().typed_committed,
            vec![record]
        );
    }

    #[test]
    fn append_preflight_rejects_unrecoverable_prefix_before_creating_record() {
        let directory = TestDirectory::new();
        let transaction = "tx_preflight";
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(transaction)
            .unwrap();
        let record = RegistryRecord {
            version: super::super::journal::FORMAT_VERSION,
            transaction_id: transaction.into(),
            sequence: 0,
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Setting".into(),
            },
            before: ValueState::ValueMissing,
            intended: ValueState::ValueMissing,
        };
        let bytes = super::super::journal::encode(&record).unwrap();
        assert!(storage
            .preflight_with_limits(
                transaction,
                0,
                &bytes,
                bytes.len() - 1,
                MAX_CHECKPOINT_BYTES
            )
            .is_err());
        assert!(storage
            .preflight_with_limits(transaction, 0, &bytes, MAX_TRANSACTION_BYTES, 1)
            .is_err());
        assert!(!storage
            .storage
            .record_path(transaction, 0)
            .unwrap()
            .exists());
        storage.preflight(transaction, 0, &bytes).unwrap();
        storage.create_new(transaction, 0, &bytes).unwrap();
        storage.sync(transaction, 0).unwrap();
        let mut next = record.clone();
        next.sequence = 1;
        let next_bytes = super::super::journal::encode(&next).unwrap();
        let error = storage
            .preflight_with_limits(
                transaction,
                1,
                &next_bytes,
                bytes.len() + next_bytes.len() - 1,
                MAX_CHECKPOINT_BYTES,
            )
            .unwrap_err();
        assert!(error
            .get_ref()
            .is_some_and(|source| source.is::<SnapshotCapacity>()));
        assert!(!storage
            .storage
            .record_path(transaction, 1)
            .unwrap()
            .exists());
        assert_eq!(storage.scan(transaction).unwrap().committed, vec![record]);
    }

    #[test]
    fn recovery_record_preflight_reserves_long_identity_and_sequence() {
        let record = TransactionRecord::Registry(RegistryRecord {
            version: super::super::journal::FORMAT_VERSION,
            transaction_id: "x".into(),
            sequence: 0,
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Setting".into(),
            },
            before: ValueState::ValueMissing,
            intended: ValueState::ValueMissing,
        });
        let original_len = super::super::journal::encode_transaction_record(&record)
            .unwrap()
            .len();
        assert!(ensure_recovery_record_fits(&record, original_len).is_err());
        assert!(ensure_recovery_record_fits(&record, original_len + 130).is_ok());
    }

    #[test]
    fn registry_journal_persists_and_verifies_before_returning() {
        let directory = TestDirectory::new();
        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_contract")
            .unwrap();
        let mut journal = RegistryJournal::new(storage, "tx_contract".into());
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let before = ValueState::ValueMissing;
        let intended = ValueState::ValueMissing;

        journal.persist(&target, &before, &intended).unwrap();

        let mut storage = journal.into_storage();
        let record: RegistryRecord = decode(&storage.read("tx_contract", 0).unwrap()).unwrap();
        assert_eq!(record.sequence, 0);
        assert_eq!(record.target, target);
        assert_eq!(record.before, before);
        assert_eq!(record.intended, intended);
    }

    #[test]
    fn committed_records_scan_and_resume_after_reopening_storage() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_resume")
                .unwrap(),
            "tx_resume".into(),
        );
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        drop(journal.into_storage());

        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_resume")
            .unwrap();
        let mut resumed = RegistryJournal::resume(storage, "tx_resume".into()).unwrap();
        resumed
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut storage = resumed.into_storage();
        let scan = storage.scan("tx_resume").unwrap();
        assert_eq!(scan.incomplete_tail, None);
        assert_eq!(scan.committed.len(), 2);
        assert_eq!(scan.committed[0].sequence, 0);
        assert_eq!(scan.committed[1].sequence, 1);
    }

    #[test]
    fn uncommitted_record_is_reported_and_cannot_be_resumed() {
        let directory = TestDirectory::new();
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_orphan")
            .unwrap();
        let record = RegistryRecord {
            version: super::super::journal::FORMAT_VERSION,
            transaction_id: "tx_orphan".into(),
            sequence: 0,
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Setting".into(),
            },
            before: ValueState::ValueMissing,
            intended: ValueState::ValueMissing,
        };
        storage
            .create_new(
                "tx_orphan",
                0,
                &super::super::journal::encode(&record).unwrap(),
            )
            .unwrap();
        let scan = storage.scan("tx_orphan").unwrap();
        assert!(scan.committed.is_empty());
        assert_eq!(scan.incomplete_tail, Some(0));
        assert!(matches!(
            RegistryJournal::resume(storage, "tx_orphan".into()),
            Err(crate::data::store::JournalError::IncompleteTail(0))
        ));
    }

    #[test]
    fn scan_detects_record_corruption_using_its_commit_marker() {
        let directory = TestDirectory::new();
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_corrupt")
                .unwrap(),
            "tx_corrupt".into(),
        );
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut storage = journal.into_storage();
        let record_path = storage.storage.record_path("tx_corrupt", 0).unwrap();
        fs::write(record_path, b"{}").unwrap();
        assert_eq!(
            storage.scan("tx_corrupt").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn partial_commit_marker_fails_closed_after_record_write() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_partial_marker")
                .unwrap(),
            "tx_partial_marker".into(),
        );
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut storage = journal.into_storage();
        let marker_path = storage.storage.commit_path("tx_partial_marker", 0).unwrap();
        fs::write(&marker_path, b"{\"version\":").unwrap();
        assert_eq!(
            storage.scan("tx_partial_marker").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read(marker_path).unwrap(), b"{\"version\":");
    }

    #[test]
    fn commit_marker_flush_failure_stays_uncommitted_across_reopen() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let storage = FileJournalStorage::open_with_fault_injector(
            &directory.0,
            fault(IoBoundary::CommitMarkerFlush),
        )
        .unwrap();
        let mut journal = RegistryJournal::new(
            storage.lock_transaction("tx_marker_flush_failure").unwrap(),
            "tx_marker_flush_failure".into(),
        );
        assert!(matches!(
            journal.persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            ),
            Err(JournalError::Sync(_))
        ));
        assert!(matches!(
            journal.persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            ),
            Err(JournalError::Poisoned)
        ));
        drop(journal);

        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_marker_flush_failure")
            .unwrap();
        let scan = storage.scan("tx_marker_flush_failure").unwrap();
        assert!(scan.committed.is_empty());
        assert_eq!(scan.incomplete_tail, Some(0));
        let pending = storage
            .storage
            .commit_pending_path("tx_marker_flush_failure", 0)
            .unwrap();
        assert!(pending.exists());
        assert!(!storage
            .storage
            .commit_path("tx_marker_flush_failure", 0)
            .unwrap()
            .exists());
        assert!(matches!(
            RegistryJournal::resume(storage, "tx_marker_flush_failure".into()),
            Err(JournalError::IncompleteTail(0))
        ));
    }

    #[test]
    fn injected_journal_write_boundaries_poison_and_scan_fail_closed() {
        let boundaries = [
            IoBoundary::RecordCreate,
            IoBoundary::RecordWrite,
            IoBoundary::RecordFlush,
            IoBoundary::RecordSyncFlush,
            IoBoundary::RecordReadback,
            IoBoundary::CommitMarkerCreate,
            IoBoundary::CommitMarkerWrite,
            IoBoundary::CommitMarkerFlush,
            IoBoundary::CommitMarkerReadback,
            IoBoundary::CommitMarkerPublish,
            IoBoundary::CommitMarkerFinalReadback,
        ];
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        for (index, boundary) in boundaries.into_iter().enumerate() {
            let directory = TestDirectory::new();
            let transaction = format!("tx_journal_fault_{index}");
            let storage =
                FileJournalStorage::open_with_fault_injector(&directory.0, fault(boundary))
                    .unwrap();
            let mut journal = RegistryJournal::new(
                storage.lock_transaction(transaction.clone()).unwrap(),
                transaction.clone(),
            );
            assert!(
                journal
                    .persist(
                        &target,
                        &ValueState::ValueMissing,
                        &ValueState::ValueMissing,
                    )
                    .is_err(),
                "fault boundary {boundary:?} did not fail"
            );
            assert!(matches!(
                journal.persist(
                    &target,
                    &ValueState::ValueMissing,
                    &ValueState::ValueMissing,
                ),
                Err(JournalError::Poisoned)
            ));
            drop(journal);

            let mut storage = FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction(transaction.clone())
                .unwrap();
            let scan = storage.scan(&transaction).unwrap();
            assert_eq!(
                storage.storage.discover_transactions().unwrap(),
                vec![transaction.clone()]
            );
            if boundary == IoBoundary::RecordCreate {
                assert!(scan.committed.is_empty());
                assert_eq!(scan.incomplete_tail, None);
            } else if boundary == IoBoundary::CommitMarkerFinalReadback {
                assert_eq!(scan.committed.len(), 1);
                assert_eq!(scan.incomplete_tail, None);
            } else {
                assert!(scan.committed.is_empty(), "{boundary:?} committed a record");
                assert_eq!(scan.incomplete_tail, Some(0), "boundary {boundary:?}");
                assert!(matches!(
                    RegistryJournal::resume(storage, transaction),
                    Err(JournalError::IncompleteTail(0))
                ));
                continue;
            }
        }
    }

    #[test]
    fn journal_child_process_crashes_reopen_to_only_published_commits() {
        let record_boundaries = [
            IoBoundary::RecordCreate,
            IoBoundary::RecordWrite,
            IoBoundary::RecordFlush,
            IoBoundary::RecordSyncFlush,
            IoBoundary::RecordReadback,
            IoBoundary::CommitMarkerCreate,
            IoBoundary::CommitMarkerWrite,
            IoBoundary::CommitMarkerFlush,
            IoBoundary::CommitMarkerReadback,
            IoBoundary::CommitMarkerPublish,
            IoBoundary::CommitMarkerFinalReadback,
        ];
        let checkpoint_boundaries = [
            IoBoundary::CheckpointCreate,
            IoBoundary::CheckpointWrite,
            IoBoundary::CheckpointFlush,
            IoBoundary::CheckpointReadback,
            IoBoundary::CheckpointMarkerCreate,
            IoBoundary::CheckpointMarkerWrite,
            IoBoundary::CheckpointMarkerFlush,
            IoBoundary::CheckpointMarkerReadback,
            IoBoundary::CheckpointMarkerPublish,
            IoBoundary::CheckpointMarkerFinalReadback,
        ];
        let rotation_boundaries = checkpoint_boundaries;
        let crash_cases = record_boundaries
            .into_iter()
            .map(|boundary| ("record", boundary))
            .chain(
                checkpoint_boundaries
                    .into_iter()
                    .map(|boundary| ("checkpoint", boundary)),
            )
            .chain(
                rotation_boundaries
                    .into_iter()
                    .map(|boundary| ("rotation", boundary)),
            );
        for (index, (kind, boundary)) in crash_cases.enumerate() {
            let directory = TestDirectory::new();
            let transaction = format!("tx_process_crash_{index}");
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "data::file_store::tests::journal_crash_child_process_probe",
                    "--nocapture",
                ])
                .env("RUSTWEAKER_CRASH_PROBE_ROOT", &directory.0)
                .env("RUSTWEAKER_CRASH_PROBE_TRANSACTION", &transaction)
                .env("RUSTWEAKER_CRASH_PROBE_KIND", kind)
                .env("RUSTWEAKER_CRASH_PROBE_BOUNDARY", format!("{boundary:?}"))
                .env(
                    "RUSTWEAKER_CRASH_PROBE_SIGNAL",
                    listener.local_addr().unwrap().to_string(),
                )
                .env("RUSTWEAKER_CRASH_PROBE_MODE", "arm")
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let signal = loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        use std::io::Read as _;
                        stream.set_nonblocking(false).unwrap();
                        let mut message = String::new();
                        stream.read_to_string(&mut message).unwrap();
                        break Some(message.trim().to_owned());
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            break None;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("failed to accept crash probe signal: {error}"),
                }
            };
            if signal.is_none() {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child did not reach crash boundary {boundary:?}");
            }
            let expected_signal = format!("{boundary:?}");
            assert_eq!(signal.as_deref(), Some(expected_signal.as_str()));
            child.kill().unwrap();
            assert!(!child.wait().unwrap().success());

            let verify = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "data::file_store::tests::journal_crash_child_process_probe",
                    "--nocapture",
                ])
                .env("RUSTWEAKER_CRASH_PROBE_ROOT", &directory.0)
                .env("RUSTWEAKER_CRASH_PROBE_TRANSACTION", &transaction)
                .env("RUSTWEAKER_CRASH_PROBE_KIND", kind)
                .env("RUSTWEAKER_CRASH_PROBE_BOUNDARY", format!("{boundary:?}"))
                .env("RUSTWEAKER_CRASH_PROBE_MODE", "verify")
                .status()
                .unwrap();
            assert!(verify.success(), "verify mode failed for {boundary:?}");
        }
    }

    #[test]
    fn journal_vm_crash_probe_prepare_mode_creates_a_private_root() {
        let path = TestDirectory::unique_path();
        let prepared = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "data::file_store::tests::journal_crash_child_process_probe",
                "--nocapture",
            ])
            .env("RUSTWEAKER_CRASH_PROBE_ROOT", &path)
            .env("RUSTWEAKER_CRASH_PROBE_TRANSACTION", "unused_prepare_id")
            .env("RUSTWEAKER_CRASH_PROBE_MODE", "prepare")
            .status()
            .unwrap();
        assert!(prepared.success());
        let directory = TestDirectory(path);
        assert!(FileJournalStorage::open(&directory.0).is_ok());
    }

    #[test]
    fn journal_crash_child_process_probe() {
        let (Ok(mode), Ok(root), Ok(transaction)) = (
            std::env::var("RUSTWEAKER_CRASH_PROBE_MODE"),
            std::env::var("RUSTWEAKER_CRASH_PROBE_ROOT"),
            std::env::var("RUSTWEAKER_CRASH_PROBE_TRANSACTION"),
        ) else {
            return;
        };
        match mode.as_str() {
            "prepare" => {
                FileJournalStorage::create_private(root)
                    .expect("create a disposable NTFS journal probe root");
            }
            "arm" => {
                let kind = std::env::var("RUSTWEAKER_CRASH_PROBE_KIND")
                    .expect("arm mode requires a probe kind");
                let boundary = probe_boundary(
                    &std::env::var("RUSTWEAKER_CRASH_PROBE_BOUNDARY")
                        .expect("arm mode requires a boundary name"),
                );
                let endpoint = std::env::var("RUSTWEAKER_CRASH_PROBE_SIGNAL")
                    .expect("arm mode requires an external controller endpoint")
                    .parse()
                    .expect("controller endpoint must be an IP socket address");
                let occurrence = if kind == "rotation" { 2 } else { 1 };
                let storage = FileJournalStorage::open_with_fault_injector(
                    root,
                    Arc::new(SignalAndPauseAtBoundary {
                        boundary,
                        occurrence,
                        seen: Mutex::new(0),
                        endpoint,
                    }),
                )
                .unwrap();
                let storage = storage.create_new_transaction(transaction.clone()).unwrap();
                let mut journal = RegistryJournal::new(storage, transaction.clone());
                let target = ValueTarget {
                    hive: Hive::LocalMachine,
                    view: RegistryView::Registry64,
                    key_path: "Software\\CrashProbe".into(),
                    value_name: "Probe".into(),
                };
                let _ = journal.persist(
                    &target,
                    &ValueState::ValueMissing,
                    &ValueState::ValueMissing,
                );
                if kind == "checkpoint" {
                    let mut storage = journal.into_storage();
                    let records = storage.scan(&transaction).unwrap().committed;
                    let recovery_id = format!("{transaction}_undo");
                    let _checkpoint = storage
                        .open_recovery_checkpoint(records, Some(recovery_id))
                        .expect("open recovery checkpoint probe");
                    panic!("checkpoint crash probe returned without reaching its boundary");
                } else if kind == "rotation" {
                    let mut storage = journal.into_storage();
                    let records = storage.scan(&transaction).unwrap().committed;
                    let recovery_id = format!("{transaction}_undo");
                    let mut checkpoint = storage
                        .open_recovery_checkpoint(records, Some(recovery_id))
                        .expect("open initial recovery checkpoint");
                    checkpoint
                        .rotate_snapshot_transaction()
                        .expect("rotate recovery snapshot");
                    panic!("rotation crash probe returned without reaching its boundary");
                } else if kind != "record" {
                    panic!("unsupported crash probe kind: {kind}");
                }
                panic!("record crash probe returned without reaching its boundary");
            }
            "verify" => {
                let kind = std::env::var("RUSTWEAKER_CRASH_PROBE_KIND")
                    .expect("verify mode requires a probe kind");
                let boundary = probe_boundary(
                    &std::env::var("RUSTWEAKER_CRASH_PROBE_BOUNDARY")
                        .expect("verify mode requires a boundary name"),
                );
                let mut storage = FileJournalStorage::open(root)
                    .unwrap()
                    .lock_transaction(transaction.clone())
                    .unwrap();
                let scan = storage.scan(&transaction).unwrap();
                match kind.as_str() {
                    "record" => {
                        if boundary == IoBoundary::RecordCreate {
                            assert!(scan.committed.is_empty());
                            assert_eq!(scan.incomplete_tail, None);
                        } else if boundary == IoBoundary::CommitMarkerFinalReadback {
                            assert_eq!(scan.committed.len(), 1);
                            assert_eq!(scan.incomplete_tail, None);
                        } else {
                            assert!(scan.committed.is_empty());
                            assert_eq!(scan.incomplete_tail, Some(0));
                        }
                    }
                    "checkpoint" => {
                        assert_eq!(scan.committed.len(), 1);
                        assert_eq!(scan.incomplete_tail, None);
                        let checkpoint_files = storage
                            .storage
                            .checkpoint_sequences_for(&transaction, ".json")
                            .unwrap();
                        let checkpoint_markers = storage
                            .storage
                            .checkpoint_sequences_for(&transaction, ".commit")
                            .unwrap();
                        let pending_markers = storage
                            .storage
                            .checkpoint_sequences_for(&transaction, ".commit.pending")
                            .unwrap();
                        if boundary == IoBoundary::CheckpointCreate {
                            assert!(checkpoint_files.is_empty());
                        } else {
                            assert_eq!(checkpoint_files, BTreeSet::from([0]));
                        }
                        if boundary == IoBoundary::CheckpointMarkerFinalReadback {
                            assert_eq!(checkpoint_markers, BTreeSet::from([0]));
                            assert!(pending_markers.is_empty());
                        } else {
                            assert!(checkpoint_markers.is_empty());
                        }
                        if matches!(
                            boundary,
                            IoBoundary::CheckpointMarkerWrite
                                | IoBoundary::CheckpointMarkerFlush
                                | IoBoundary::CheckpointMarkerReadback
                                | IoBoundary::CheckpointMarkerPublish
                        ) {
                            assert_eq!(pending_markers, BTreeSet::from([0]));
                        } else {
                            assert!(pending_markers.is_empty());
                        }
                        let recovery_id = format!("{transaction}_undo");
                        let checkpoint = storage
                            .open_recovery_checkpoint(scan.committed, Some(recovery_id))
                            .unwrap();
                        assert_eq!(checkpoint.next_exclusive(), 1);
                        drop(checkpoint);
                    }
                    "rotation" => {
                        assert_eq!(scan.committed.len(), 1);
                        assert_eq!(scan.incomplete_tail, None);
                        let files = storage
                            .storage
                            .checkpoint_sequences_for(&transaction, ".json")
                            .unwrap();
                        let markers = storage
                            .storage
                            .checkpoint_sequences_for(&transaction, ".commit")
                            .unwrap();
                        let pending = storage
                            .storage
                            .checkpoint_sequences_for(&transaction, ".commit.pending")
                            .unwrap();
                        assert_eq!(
                            files,
                            if boundary == IoBoundary::CheckpointCreate {
                                BTreeSet::from([0])
                            } else {
                                BTreeSet::from([0, 1])
                            }
                        );
                        if boundary == IoBoundary::CheckpointMarkerFinalReadback {
                            assert_eq!(markers, BTreeSet::from([0, 1]));
                            assert!(pending.is_empty());
                        } else {
                            assert_eq!(markers, BTreeSet::from([0]));
                            if matches!(
                                boundary,
                                IoBoundary::CheckpointMarkerWrite
                                    | IoBoundary::CheckpointMarkerFlush
                                    | IoBoundary::CheckpointMarkerReadback
                                    | IoBoundary::CheckpointMarkerPublish
                            ) {
                                assert_eq!(pending, BTreeSet::from([1]));
                            } else {
                                assert!(pending.is_empty());
                            }
                        }
                        let checkpoint = storage
                            .open_recovery_checkpoint(scan.committed, None)
                            .unwrap();
                        assert_eq!(checkpoint.next_exclusive(), 1);
                        if boundary == IoBoundary::CheckpointMarkerFinalReadback {
                            assert_ne!(
                                checkpoint.recovery_transaction_id(),
                                format!("{transaction}_undo")
                            );
                        } else {
                            assert_eq!(
                                checkpoint.recovery_transaction_id(),
                                format!("{transaction}_undo")
                            );
                        }
                    }
                    _ => panic!("unsupported crash probe kind: {kind}"),
                }
            }
            _ => panic!("unsupported crash probe mode: {mode}"),
        }
    }

    fn probe_boundary(name: &str) -> IoBoundary {
        match name {
            "RecordCreate" => IoBoundary::RecordCreate,
            "RecordWrite" => IoBoundary::RecordWrite,
            "RecordFlush" => IoBoundary::RecordFlush,
            "RecordSyncFlush" => IoBoundary::RecordSyncFlush,
            "RecordReadback" => IoBoundary::RecordReadback,
            "CommitMarkerCreate" => IoBoundary::CommitMarkerCreate,
            "CommitMarkerWrite" => IoBoundary::CommitMarkerWrite,
            "CommitMarkerFlush" => IoBoundary::CommitMarkerFlush,
            "CommitMarkerReadback" => IoBoundary::CommitMarkerReadback,
            "CommitMarkerPublish" => IoBoundary::CommitMarkerPublish,
            "CommitMarkerFinalReadback" => IoBoundary::CommitMarkerFinalReadback,
            "CheckpointCreate" => IoBoundary::CheckpointCreate,
            "CheckpointWrite" => IoBoundary::CheckpointWrite,
            "CheckpointFlush" => IoBoundary::CheckpointFlush,
            "CheckpointReadback" => IoBoundary::CheckpointReadback,
            "CheckpointMarkerCreate" => IoBoundary::CheckpointMarkerCreate,
            "CheckpointMarkerWrite" => IoBoundary::CheckpointMarkerWrite,
            "CheckpointMarkerFlush" => IoBoundary::CheckpointMarkerFlush,
            "CheckpointMarkerReadback" => IoBoundary::CheckpointMarkerReadback,
            "CheckpointMarkerPublish" => IoBoundary::CheckpointMarkerPublish,
            "CheckpointMarkerFinalReadback" => IoBoundary::CheckpointMarkerFinalReadback,
            _ => panic!("unsupported crash probe boundary: {name}"),
        }
    }

    #[test]
    fn commit_markers_cannot_skip_a_sequence() {
        let directory = TestDirectory::new();
        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_gap")
            .unwrap();
        let record = RegistryRecord {
            version: super::super::journal::FORMAT_VERSION,
            transaction_id: "tx_gap".into(),
            sequence: 1,
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Setting".into(),
            },
            before: ValueState::ValueMissing,
            intended: ValueState::ValueMissing,
        };
        storage
            .create_new(
                "tx_gap",
                1,
                &super::super::journal::encode(&record).unwrap(),
            )
            .unwrap();
        assert_eq!(
            storage.sync("tx_gap", 1).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            storage.scan("tx_gap").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn transaction_lock_is_exclusive_and_released_on_drop() {
        let directory = TestDirectory::new();
        let lock = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_lock")
            .unwrap();
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_lock")
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "data::file_store::tests::transaction_lock_child_process_probe",
                "--nocapture",
            ])
            .env("RUSTWEAKER_LOCK_PROBE_ROOT", &directory.0)
            .status()
            .unwrap();
        assert!(child.success());
        drop(lock);
        let reacquired = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_lock")
            .unwrap();
        drop(reacquired);
    }

    #[test]
    fn transaction_lock_child_process_probe() {
        let Ok(root) = std::env::var("RUSTWEAKER_LOCK_PROBE_ROOT") else {
            return;
        };
        assert_eq!(
            FileJournalStorage::open(root)
                .unwrap()
                .lock_transaction("tx_lock")
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn transaction_discovery_lock_excludes_new_ids_from_other_processes() {
        let directory = TestDirectory::new();
        let storage = FileJournalStorage::open(&directory.0).unwrap();
        let discovery_lock = storage.lock_transaction_discovery().unwrap();
        assert_eq!(
            storage.discover_transactions().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );

        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "data::file_store::tests::transaction_discovery_lock_child_process_probe",
                "--nocapture",
            ])
            .env("RUSTWEAKER_DISCOVERY_LOCK_PROBE_ROOT", &directory.0)
            .status()
            .unwrap();
        assert!(child.success());
        drop(discovery_lock);

        assert_eq!(
            storage.discover_transactions().unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn transaction_discovery_lock_child_process_probe() {
        let Ok(root) = std::env::var("RUSTWEAKER_DISCOVERY_LOCK_PROBE_ROOT") else {
            return;
        };
        assert_eq!(
            FileJournalStorage::open(root)
                .unwrap()
                .create_new_transaction("tx_discovery_child")
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn registry_mutation_lock_is_cross_process_and_excluded_from_discovery() {
        let directory = TestDirectory::new();
        let storage = FileJournalStorage::open(&directory.0).unwrap();
        let lock = storage.lock_registry_mutations().unwrap();
        assert_eq!(
            storage.discover_transactions().unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_registry_mutations()
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "data::file_store::tests::registry_mutation_lock_child_process_probe",
                "--nocapture",
            ])
            .env("RUSTWEAKER_REGISTRY_LOCK_PROBE_ROOT", &directory.0)
            .status()
            .unwrap();
        assert!(child.success());
        drop(lock);
        drop(storage.lock_registry_mutations().unwrap());
    }

    #[test]
    fn registry_mutation_lock_child_process_probe() {
        let Ok(root) = std::env::var("RUSTWEAKER_REGISTRY_LOCK_PROBE_ROOT") else {
            return;
        };
        assert_eq!(
            FileJournalStorage::open(root)
                .unwrap()
                .lock_registry_mutations()
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn recovery_checkpoint_progress_survives_reopen_under_transaction_lock() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_checkpoint")
                .unwrap(),
            "tx_checkpoint".into(),
        );
        for value in [1, 2] {
            journal
                .persist(
                    &target,
                    &ValueState::ValueMissing,
                    &ValueState::Present(crate::engine::registry::RegistryValue {
                        native_type: 4,
                        bytes: vec![value],
                    }),
                )
                .unwrap();
        }
        let mut storage = journal.into_storage();
        let records = storage.scan("tx_checkpoint").unwrap().committed;
        let mut checkpoint = storage
            .open_recovery_checkpoint(records.clone(), Some("tx_checkpoint_undo".into()))
            .unwrap();
        assert_eq!(checkpoint.recovery_transaction_id(), "tx_checkpoint_undo");
        assert_eq!(checkpoint.next_exclusive(), 2);
        checkpoint.persist_next(1).unwrap();
        drop(checkpoint);

        let mut resumed = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_checkpoint")
            .unwrap()
            .open_recovery_checkpoint(records.clone(), None)
            .unwrap();
        assert_eq!(resumed.next_exclusive(), 1);
        resumed.persist_next(0).unwrap();
        drop(resumed);

        let completed = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_checkpoint")
            .unwrap()
            .open_recovery_checkpoint(records, None)
            .unwrap();
        assert_eq!(completed.next_exclusive(), 0);
        drop(completed);
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .discover_transactions()
                .unwrap(),
            vec!["tx_checkpoint"]
        );
    }

    #[test]
    fn incomplete_checkpoint_tail_is_not_overwritten_or_treated_as_committed() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_checkpoint_tail")
                .unwrap(),
            "tx_checkpoint_tail".into(),
        );
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut storage = journal.into_storage();
        let records = storage.scan("tx_checkpoint_tail").unwrap().committed;
        let checkpoint = storage
            .open_recovery_checkpoint(records.clone(), Some("tx_checkpoint_tail_undo".into()))
            .unwrap();
        drop(checkpoint);

        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_checkpoint_tail")
            .unwrap();
        let incomplete_path = storage
            .storage
            .checkpoint_path("tx_checkpoint_tail", 1)
            .unwrap();
        fs::write(&incomplete_path, b"partial checkpoint").unwrap();
        drop(storage);

        let mut resumed = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_checkpoint_tail")
            .unwrap()
            .open_recovery_checkpoint(records, None)
            .unwrap();
        assert_eq!(resumed.next_exclusive(), 1);
        resumed.persist_next(0).unwrap();
        assert!(incomplete_path.exists());
        assert!(resumed
            .storage
            .storage
            .checkpoint_path("tx_checkpoint_tail", 2)
            .unwrap()
            .exists());
    }

    #[test]
    fn partial_checkpoint_commit_marker_fails_closed() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_partial_checkpoint_marker")
                .unwrap(),
            "tx_partial_checkpoint_marker".into(),
        );
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut storage = journal.into_storage();
        let records = storage
            .scan("tx_partial_checkpoint_marker")
            .unwrap()
            .committed;
        drop(storage);
        drop(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_partial_checkpoint_marker")
                .unwrap()
                .open_recovery_checkpoint(
                    records.clone(),
                    Some("tx_partial_checkpoint_undo".into()),
                )
                .unwrap(),
        );
        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_partial_checkpoint_marker")
            .unwrap();
        let checkpoint = Checkpoint::new(&records, "tx_partial_checkpoint_undo".into(), 0).unwrap();
        let checkpoint_path = storage
            .storage
            .checkpoint_path("tx_partial_checkpoint_marker", 1)
            .unwrap();
        fs::write(&checkpoint_path, checkpoint::encode(&checkpoint).unwrap()).unwrap();
        let marker_path = storage
            .storage
            .checkpoint_marker_path("tx_partial_checkpoint_marker", 1)
            .unwrap();
        fs::write(&marker_path, b"{\"generation\":").unwrap();
        drop(storage);

        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_partial_checkpoint_marker")
                .unwrap()
                .open_recovery_checkpoint(records, None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read(marker_path).unwrap(), b"{\"generation\":");
    }

    #[test]
    fn checkpoint_marker_flush_failure_never_publishes_commit_marker() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_checkpoint_flush_failure")
                .unwrap(),
            "tx_checkpoint_flush_failure".into(),
        );
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut storage = journal.into_storage();
        let records = storage
            .scan("tx_checkpoint_flush_failure")
            .unwrap()
            .committed;
        drop(storage);

        let storage = FileJournalStorage::open_with_fault_injector(
            &directory.0,
            fault(IoBoundary::CheckpointMarkerFlush),
        )
        .unwrap()
        .lock_transaction("tx_checkpoint_flush_failure")
        .unwrap();
        assert_eq!(
            storage
                .open_recovery_checkpoint(records.clone(), Some("tx_checkpoint_flush_undo".into()),)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Other
        );

        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_checkpoint_flush_failure")
            .unwrap();
        assert!(!storage
            .storage
            .checkpoint_marker_path("tx_checkpoint_flush_failure", 0)
            .unwrap()
            .exists());
        assert!(storage
            .storage
            .checkpoint_marker_pending_path("tx_checkpoint_flush_failure", 0)
            .unwrap()
            .exists());
        let resumed = storage
            .open_recovery_checkpoint(records, Some("tx_checkpoint_flush_retry".into()))
            .unwrap();
        assert_eq!(
            resumed.recovery_transaction_id(),
            "tx_checkpoint_flush_retry"
        );
        assert_eq!(resumed.next_exclusive(), 1);
    }

    #[test]
    fn injected_checkpoint_boundaries_resume_without_claiming_unflushed_state() {
        let boundaries = [
            IoBoundary::CheckpointCreate,
            IoBoundary::CheckpointWrite,
            IoBoundary::CheckpointFlush,
            IoBoundary::CheckpointReadback,
            IoBoundary::CheckpointMarkerCreate,
            IoBoundary::CheckpointMarkerWrite,
            IoBoundary::CheckpointMarkerFlush,
            IoBoundary::CheckpointMarkerReadback,
            IoBoundary::CheckpointMarkerPublish,
            IoBoundary::CheckpointMarkerFinalReadback,
        ];
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        for (index, boundary) in boundaries.into_iter().enumerate() {
            let directory = TestDirectory::new();
            let transaction = format!("tx_checkpoint_fault_{index}");
            let mut journal = RegistryJournal::new(
                FileJournalStorage::open(&directory.0)
                    .unwrap()
                    .lock_transaction(transaction.clone())
                    .unwrap(),
                transaction.clone(),
            );
            journal
                .persist(
                    &target,
                    &ValueState::ValueMissing,
                    &ValueState::ValueMissing,
                )
                .unwrap();
            let mut storage = journal.into_storage();
            let records = storage.scan(&transaction).unwrap().committed;
            drop(storage);

            let storage =
                FileJournalStorage::open_with_fault_injector(&directory.0, fault(boundary))
                    .unwrap()
                    .lock_transaction(transaction.clone())
                    .unwrap();
            assert!(
                storage
                    .open_recovery_checkpoint(
                        records.clone(),
                        Some(format!("tx_checkpoint_fault_{index}_undo")),
                    )
                    .is_err(),
                "fault boundary {boundary:?} did not fail"
            );

            let resume_id = format!("tx_checkpoint_fault_{index}_retry");
            let storage = FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction(transaction.clone())
                .unwrap();
            let resumed = if boundary == IoBoundary::CheckpointMarkerFinalReadback {
                storage.open_recovery_checkpoint(records, None).unwrap()
            } else {
                storage
                    .open_recovery_checkpoint(records, Some(resume_id.clone()))
                    .unwrap()
            };
            if boundary == IoBoundary::CheckpointMarkerFinalReadback {
                assert_eq!(
                    resumed.recovery_transaction_id(),
                    format!("tx_checkpoint_fault_{index}_undo")
                );
            } else {
                assert_eq!(resumed.recovery_transaction_id(), resume_id);
            }
            assert_eq!(resumed.next_exclusive(), 1);
        }
    }

    #[test]
    fn checkpoint_rejects_a_different_journal_and_forward_progress() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_checkpoint_identity")
                .unwrap(),
            "tx_checkpoint_identity".into(),
        );
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut storage = journal.into_storage();
        let mut records = storage.scan("tx_checkpoint_identity").unwrap().committed;
        let mut wrong = records.clone();
        wrong[0].target.value_name = "Other".into();
        assert_eq!(
            storage
                .open_recovery_checkpoint(wrong, Some("tx_checkpoint_identity_undo".into()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let mut checkpoint = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_checkpoint_identity")
            .unwrap()
            .open_recovery_checkpoint(records.clone(), Some("tx_checkpoint_identity_undo".into()))
            .unwrap();
        checkpoint.persist_next(0).unwrap();
        assert_eq!(
            checkpoint.persist_next(1).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        drop(checkpoint);

        records[0].intended = ValueState::KeyMissing;
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_checkpoint_identity")
                .unwrap()
                .open_recovery_checkpoint(records, None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[derive(Debug)]
    struct MockRecoveryBackend {
        state: ValueState,
        writes: usize,
        fail_on_write: Option<usize>,
    }

    #[test]
    fn registry_capacity_rotation_requires_explicit_retry_and_survives_reopen() {
        let directory = TestDirectory::new();
        let transaction = "registry_capacity_source";
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let before = ValueState::ValueMissing;
        let intended = ValueState::Present(RegistryValue {
            native_type: 1,
            bytes: vec![1],
        });
        let storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .create_new_transaction(transaction)
            .unwrap();
        let mut source = RegistryJournal::new(storage, transaction.into());
        source.persist(&target, &before, &intended).unwrap();
        drop(source.into_storage());
        let mut session =
            FileRecoverySession::start_with_id(&directory.0, transaction, "registry_capacity_old")
                .unwrap();
        for _ in 0..10 {
            session
                .snapshots
                .persist(&target, &intended, &before)
                .unwrap();
        }
        session
            .snapshots
            .test_storage_mut()
            .storage
            .preflight_limits = Some((1024, MAX_CHECKPOINT_BYTES));
        let mut backend = MockRecoveryBackend {
            state: intended.clone(),
            writes: 0,
            fail_on_write: None,
        };
        assert!(matches!(
            session.run(&mut backend),
            Err(JournalRecoveryError::SnapshotRotatedRetryRequired)
        ));
        assert_eq!(backend.writes, 0);
        let new_id = session.recovery_transaction_id().to_owned();
        assert_ne!(new_id, "registry_capacity_old");
        assert_eq!(session.next_exclusive(), 1);
        drop(session);
        let mut session = FileRecoverySession::open(&directory.0, transaction, None).unwrap();
        assert_eq!(session.recovery_transaction_id(), new_id);
        let mut backend = MockRecoveryBackend {
            state: intended,
            writes: 0,
            fail_on_write: None,
        };
        session.run(&mut backend).unwrap();
        assert_eq!(backend.writes, 1);
        assert_eq!(backend.state, before);
        assert_eq!(session.next_exclusive(), 0);
    }

    struct MutationProbeBackend {
        root: PathBuf,
        transaction: String,
        state: ValueState,
        saw_registry_lock: bool,
        saw_committed_backup: bool,
        queries: usize,
        fail_after_write: bool,
    }

    impl Backend for MutationProbeBackend {
        type Target = ValueTarget;
        type Change = ValueState;
        type Snapshot = ValueState;
        type Error = &'static str;

        fn query(&mut self, _: &ValueTarget) -> Result<ValueState, Self::Error> {
            self.queries += 1;
            Ok(self.state.clone())
        }

        fn apply(&mut self, target: &ValueTarget, change: &ValueState) -> Result<(), Self::Error> {
            let storage =
                FileJournalStorage::open(&self.root).map_err(|_| "journal root failed")?;
            let record_path = storage
                .record_path(&self.transaction, 0)
                .map_err(|_| "record path failed")?;
            let marker_path = storage
                .commit_path(&self.transaction, 0)
                .map_err(|_| "marker path failed")?;
            let record = decode(&fs::read(record_path).map_err(|_| "backup record missing")?)
                .map_err(|_| "backup record invalid")?;
            self.saw_committed_backup =
                marker_path.exists() && record.target == *target && record.intended == *change;
            if !self.saw_committed_backup {
                return Err("committed backup was not visible before apply");
            }
            match storage.lock_registry_mutations() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    self.saw_registry_lock = true;
                }
                _ => return Err("registry mutation barrier was not held during apply"),
            }
            self.state = change.clone();
            if self.fail_after_write {
                Err("partial apply failure")
            } else {
                Ok(())
            }
        }
    }

    impl RecoveryBackend for MockRecoveryBackend {
        type Target = ValueTarget;
        type State = ValueState;
        type Error = &'static str;

        fn query(&mut self, _: &ValueTarget) -> Result<ValueState, Self::Error> {
            Ok(self.state.clone())
        }

        fn restore(&mut self, _: &ValueTarget, state: &ValueState) -> Result<(), Self::Error> {
            self.writes += 1;
            if self.fail_on_write == Some(self.writes) {
                return Err("restore failed");
            }
            self.state = state.clone();
            Ok(())
        }
    }

    #[test]
    fn disk_journal_checkpoint_and_recovery_snapshot_journal_work_together() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let missing = ValueState::ValueMissing;
        let first_state = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1],
        });
        let final_state = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![2],
        });
        let mut original_journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_original")
                .unwrap(),
            "tx_original".into(),
        );
        original_journal
            .persist(&target, &missing, &first_state)
            .unwrap();
        original_journal
            .persist(&target, &first_state, &final_state)
            .unwrap();
        drop(original_journal.into_storage());
        let mut session = FileRecoverySession::open(
            &directory.0,
            "tx_original",
            Some("tx_recovery_attempt".into()),
        )
        .unwrap();
        assert_eq!(session.recovery_transaction_id(), "tx_recovery_attempt");
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_registry_mutations()
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        let mut backend = MockRecoveryBackend {
            state: final_state.clone(),
            writes: 0,
            fail_on_write: None,
        };
        session.run(&mut backend).unwrap();

        assert_eq!(backend.state, missing);
        assert_eq!(backend.writes, 2);
        assert_eq!(session.next_exclusive(), 0);
        drop(session);
        let mut recovery_storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_recovery_attempt")
            .unwrap();
        let recovery_records = recovery_storage
            .scan("tx_recovery_attempt")
            .unwrap()
            .committed;
        assert_eq!(recovery_records.len(), 2);
        assert_eq!(recovery_records[0].before, final_state);
        assert_eq!(recovery_records[0].intended, first_state);
        assert_eq!(recovery_records[1].before, first_state);
        assert_eq!(recovery_records[1].intended, missing);
    }

    #[test]
    fn transaction_session_does_not_apply_when_preflight_fails() {
        let directory = TestDirectory::new();
        let transaction = "tx_preflight_session";
        let storage = FileJournalStorage::open_with_fault_injector(
            &directory.0,
            fault(IoBoundary::Preflight),
        )
        .unwrap();
        let root_storage = storage.try_clone().unwrap();
        let locked = storage.create_new_transaction(transaction).unwrap();
        let mut session = FileTransactionSession {
            root_storage,
            transaction: transaction.into(),
            journal: RegistryJournal::new(locked, transaction.into()),
        };
        let before = ValueState::ValueMissing;
        let mut backend = MutationProbeBackend {
            root: directory.0.clone(),
            transaction: transaction.into(),
            state: before.clone(),
            saw_registry_lock: false,
            saw_committed_backup: false,
            queries: 0,
            fail_after_write: false,
        };
        let operation = Operation {
            target: ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Setting".into(),
            },
            change: ValueState::Present(RegistryValue {
                native_type: 4,
                bytes: vec![1, 0, 0, 0],
            }),
        };
        assert!(matches!(
            session.apply(&mut backend, &operation),
            Err(FileTransactionError::Operation(TransactionError::Persist(
                _
            )))
        ));
        assert_eq!(backend.queries, 1);
        assert_eq!(backend.state, before);
        assert!(!backend.saw_committed_backup);
        assert!(!FileJournalStorage::open(&directory.0)
            .unwrap()
            .record_path(transaction, 0)
            .unwrap()
            .exists());
    }

    #[test]
    fn file_transaction_session_persists_before_apply_under_registry_barrier() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let before = ValueState::ValueMissing;
        let intended = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![9],
        });
        let mut backend = MutationProbeBackend {
            root: directory.0.clone(),
            transaction: "tx_apply_session".into(),
            state: before.clone(),
            saw_registry_lock: false,
            saw_committed_backup: false,
            queries: 0,
            fail_after_write: false,
        };
        let mut session =
            FileJournalStorage::start_transaction(&directory.0, "tx_apply_session").unwrap();
        assert_eq!(session.transaction_id(), "tx_apply_session");
        session
            .apply(
                &mut backend,
                &Operation {
                    target: target.clone(),
                    change: intended.clone(),
                },
            )
            .unwrap();
        assert!(backend.saw_registry_lock);
        assert!(backend.saw_committed_backup);
        assert_eq!(backend.queries, 1);
        assert_eq!(backend.state, intended);
        drop(session);

        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_apply_session")
            .unwrap();
        let scan = storage.scan("tx_apply_session").unwrap();
        assert_eq!(scan.committed.len(), 1);
        assert_eq!(scan.committed[0].before, before);
        assert_eq!(scan.committed[0].intended, backend.state);
    }

    #[test]
    fn file_transaction_session_keeps_durable_backup_after_partial_apply_failure() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let before = ValueState::ValueMissing;
        let intended = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![9],
        });
        let mut backend = MutationProbeBackend {
            root: directory.0.clone(),
            transaction: "tx_partial_apply".into(),
            state: before.clone(),
            saw_registry_lock: false,
            saw_committed_backup: false,
            queries: 0,
            fail_after_write: true,
        };
        let mut session =
            FileJournalStorage::start_transaction(&directory.0, "tx_partial_apply").unwrap();
        assert!(matches!(
            session.apply(
                &mut backend,
                &Operation {
                    target: target.clone(),
                    change: intended.clone(),
                },
            ),
            Err(FileTransactionError::Operation(TransactionError::Apply(
                "partial apply failure"
            )))
        ));
        assert!(backend.saw_registry_lock);
        assert!(backend.saw_committed_backup);
        assert_eq!(backend.state, intended);
        drop(session);

        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_partial_apply")
            .unwrap();
        let scan = storage.scan("tx_partial_apply").unwrap();
        assert_eq!(scan.committed.len(), 1);
        assert_eq!(scan.committed[0].target, target);
        assert_eq!(scan.committed[0].before, before);
        assert_eq!(scan.committed[0].intended, intended);
    }

    #[test]
    fn file_transaction_session_lock_failure_prevents_query_and_journal_write() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut backend = MutationProbeBackend {
            root: directory.0.clone(),
            transaction: "tx_apply_lock_failure".into(),
            state: ValueState::ValueMissing,
            saw_registry_lock: false,
            saw_committed_backup: false,
            queries: 0,
            fail_after_write: false,
        };
        let mut session =
            FileJournalStorage::start_transaction(&directory.0, "tx_apply_lock_failure").unwrap();
        let external_lock = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_registry_mutations()
            .unwrap();
        assert!(matches!(
            session.apply(
                &mut backend,
                &Operation {
                    target,
                    change: ValueState::ValueMissing,
                },
            ),
            Err(FileTransactionError::RegistryLock(error))
                if error.kind() == io::ErrorKind::WouldBlock
        ));
        assert_eq!(backend.queries, 0);
        assert!(!backend.saw_registry_lock);
        drop(external_lock);
        drop(session);

        let mut storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_apply_lock_failure")
            .unwrap();
        assert!(storage
            .scan("tx_apply_lock_failure")
            .unwrap()
            .committed
            .is_empty());
    }

    #[test]
    fn recovery_runner_reopens_both_journals_and_continues_from_checkpoint() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let missing = ValueState::ValueMissing;
        let first_state = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1],
        });
        let final_state = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![2],
        });
        let mut original_journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_restart_original")
                .unwrap(),
            "tx_restart_original".into(),
        );
        original_journal
            .persist(&target, &missing, &first_state)
            .unwrap();
        original_journal
            .persist(&target, &first_state, &final_state)
            .unwrap();
        drop(original_journal.into_storage());
        let mut session = FileRecoverySession::start_with_id(
            &directory.0,
            "tx_restart_original",
            "tx_restart_recovery",
        )
        .unwrap();
        let mut backend = MockRecoveryBackend {
            state: final_state.clone(),
            writes: 0,
            fail_on_write: Some(2),
        };
        assert!(matches!(
            session.run(&mut backend),
            Err(crate::engine::recovery::JournalRecoveryError::Recovery(
                crate::engine::recovery::BatchRecoveryError::Operation {
                    index: 0,
                    error: crate::engine::recovery::RecoveryError::Restore("restore failed")
                }
            ))
        ));
        assert_eq!(session.next_exclusive(), 1);
        assert_eq!(backend.state, first_state);
        drop(session);

        let old_tail_path = FileJournalStorage::open(&directory.0)
            .unwrap()
            .record_path("tx_restart_recovery", 2)
            .unwrap();
        fs::write(&old_tail_path, b"partial uncommitted snapshot").unwrap();

        assert!(matches!(
            FileRecoverySession::open(
                &directory.0,
                "tx_restart_original",
                Some("tx_restart_other_attempt".into())
            ),
            Err(FileRecoverySessionError::Storage(error))
                if error.kind() == io::ErrorKind::AlreadyExists
        ));

        let mut session = FileRecoverySession::resume(&directory.0, "tx_restart_original").unwrap();
        let rotated_id = session.recovery_transaction_id().to_owned();
        assert_ne!(rotated_id, "tx_restart_recovery");
        assert_eq!(session.next_exclusive(), 1);
        backend.fail_on_write = None;
        session.run(&mut backend).unwrap();

        assert_eq!(backend.state, missing);
        assert_eq!(session.next_exclusive(), 0);
        drop(session);
        let mut old_snapshot_storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction("tx_restart_recovery")
            .unwrap();
        let old_scan = old_snapshot_storage.scan("tx_restart_recovery").unwrap();
        assert_eq!(old_scan.committed.len(), 2);
        assert_eq!(old_scan.incomplete_tail, Some(2));
        assert!(old_tail_path.exists());
        drop(old_snapshot_storage);
        let mut final_snapshot_storage = FileJournalStorage::open(&directory.0)
            .unwrap()
            .lock_transaction(&rotated_id)
            .unwrap();
        let recovery_records = final_snapshot_storage.scan(&rotated_id).unwrap().committed;
        assert_eq!(recovery_records.len(), 1);
        assert_eq!(recovery_records[0].before, first_state);
        assert_eq!(recovery_records[0].intended, missing);
    }

    #[test]
    fn discovery_lists_ids_from_committed_logs_and_lock_files() {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_alpha")
                .unwrap(),
            "tx_alpha".into(),
        );
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        drop(journal.into_storage());
        drop(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("tx_beta")
                .unwrap(),
        );

        let discovered = FileJournalStorage::open(&directory.0)
            .unwrap()
            .discover_transactions()
            .unwrap();
        assert_eq!(discovered, vec!["tx_alpha", "tx_beta"]);
    }

    #[test]
    fn discovery_fails_closed_on_unrecognized_root_entries() {
        let directory = TestDirectory::new();
        fs::write(directory.0.join("unexpected.txt"), b"unrecognized").unwrap();
        assert_eq!(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .discover_transactions()
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    fn validator_fixture() -> (TestDirectory, LockedJournalStorage, Vec<TransactionRecord>) {
        let directory = TestDirectory::new();
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Setting".into(),
        };
        let mut journal = RegistryJournal::new(
            FileJournalStorage::open(&directory.0)
                .unwrap()
                .lock_transaction("validator_source")
                .unwrap(),
            "validator_source".into(),
        );
        journal
            .persist(
                &target,
                &ValueState::ValueMissing,
                &ValueState::ValueMissing,
            )
            .unwrap();
        let mut storage = journal.into_storage();
        let records = storage.scan("validator_source").unwrap().typed_committed;
        (directory, storage, records)
    }

    fn validator_snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(root)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = if name.ends_with(".lock") {
                    Vec::new()
                } else {
                    fs::read(entry.path()).unwrap()
                };
                (name, bytes)
            })
            .collect()
    }

    fn validator_write_commit(
        storage: &LockedJournalStorage,
        generation: u64,
        bytes: &[u8],
    ) -> Vec<u8> {
        let transaction = &storage.transaction;
        fs::write(
            storage
                .storage
                .checkpoint_path(transaction, generation)
                .unwrap(),
            bytes,
        )
        .unwrap();
        let marker = serde_json::to_vec(&CheckpointMarker {
            version: COMMIT_MARKER_VERSION,
            transaction_id: transaction.clone(),
            generation,
            checkpoint_crc32: crc32(bytes),
        })
        .unwrap();
        fs::write(
            storage
                .storage
                .checkpoint_marker_path(transaction, generation)
                .unwrap(),
            &marker,
        )
        .unwrap();
        marker
    }

    #[test]
    fn legacy_validator_is_read_only_without_committed_checkpoint() {
        let (directory, mut storage, records) = validator_fixture();
        fs::write(
            storage
                .storage
                .checkpoint_path("validator_source", 0)
                .unwrap(),
            b"partial",
        )
        .unwrap();
        fs::write(
            storage
                .storage
                .checkpoint_marker_pending_path("validator_source", 0)
                .unwrap(),
            b"partial",
        )
        .unwrap();
        let before = validator_snapshot(&directory.0);
        for _ in 0..2 {
            let result = storage.validate_legacy_checkpoint_prefix(&records).unwrap();
            assert!(result.committed.is_none());
            assert_eq!(result.next_generation, 1);
            assert_eq!(validator_snapshot(&directory.0), before);
        }
        let checkpoint = storage
            .open_typed_recovery_checkpoint(records, Some("validator_undo".into()))
            .unwrap();
        assert_eq!(checkpoint.next_exclusive(), 1);
        assert!(checkpoint
            .storage
            .storage
            .checkpoint_marker_path("validator_source", 1)
            .unwrap()
            .exists());
    }

    #[test]
    fn legacy_validator_preserves_exact_v3_and_v4_bytes_and_rejects_corruption() {
        #[derive(serde::Serialize)]
        struct V3<'a> {
            version: u32,
            transaction_id: &'a str,
            recovery_transaction_id: &'a str,
            next_exclusive: u64,
            records: Vec<&'a RegistryRecord>,
        }
        let (_directory, mut storage, records) = validator_fixture();
        let TransactionRecord::Registry(record) = &records[0] else {
            panic!("expected registry")
        };
        let payload = V3 {
            version: 3,
            transaction_id: "validator_source",
            recovery_transaction_id: "validator_undo",
            next_exclusive: 1,
            records: vec![record],
        };
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let v3 = format!(
            "{{\"payload\":{},\"crc32\":{}}}",
            String::from_utf8(payload_bytes.clone()).unwrap(),
            crc32(&payload_bytes)
        )
        .into_bytes();
        let marker = validator_write_commit(&storage, 0, &v3);
        let result = storage
            .validate_legacy_checkpoint_prefix(&records)
            .unwrap()
            .committed
            .unwrap();
        assert_eq!(
            (
                result.generation,
                result.next_exclusive,
                result.recovery_transaction_id.as_str()
            ),
            (0, 1, "validator_undo")
        );
        assert_eq!(result.payload_bytes, v3);
        assert_eq!(result.marker_bytes, marker);
        let v4 = checkpoint::encode(
            &Checkpoint::new_typed(&records, "validator_undo".into(), 0).unwrap(),
        )
        .unwrap();
        let marker = validator_write_commit(&storage, 1, &v4);
        let result = storage
            .validate_legacy_checkpoint_prefix(&records)
            .unwrap()
            .committed
            .unwrap();
        assert_eq!((result.generation, result.next_exclusive), (1, 0));
        assert_eq!(result.payload_bytes, v4);
        assert_eq!(result.marker_bytes, marker);
        fs::write(
            storage
                .storage
                .checkpoint_marker_path("validator_source", 1)
                .unwrap(),
            b"corrupt",
        )
        .unwrap();
        assert_eq!(
            storage
                .validate_legacy_checkpoint_prefix(&records)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            storage
                .open_typed_recovery_checkpoint(records, None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn legacy_validator_rejects_incomplete_or_mismatched_source() {
        let (directory, mut storage, records) = validator_fixture();
        assert_eq!(
            storage
                .validate_legacy_checkpoint_prefix(&[])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let mut wrong = records.clone();
        if let TransactionRecord::Registry(record) = &mut wrong[0] {
            record.intended = ValueState::Present(RegistryValue {
                native_type: 4,
                bytes: vec![1],
            });
        }
        assert_eq!(
            storage
                .validate_legacy_checkpoint_prefix(&wrong)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(
            storage.storage.record_path("validator_source", 1).unwrap(),
            b"partial",
        )
        .unwrap();
        let before = validator_snapshot(&directory.0);
        assert_eq!(
            storage
                .validate_legacy_checkpoint_prefix(&records)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(validator_snapshot(&directory.0), before);
    }

    #[test]
    fn legacy_validator_reads_last_generation_but_writer_stays_bounded() {
        let (_directory, mut storage, records) = validator_fixture();
        let bytes = checkpoint::encode(
            &Checkpoint::new_typed(&records, "validator_undo".into(), 1).unwrap(),
        )
        .unwrap();
        for generation in 0..(MAX_CHECKPOINT_GENERATIONS as u64 - 1) {
            fs::write(
                storage
                    .storage
                    .checkpoint_path("validator_source", generation)
                    .unwrap(),
                b"staging",
            )
            .unwrap();
        }
        let last = MAX_CHECKPOINT_GENERATIONS as u64 - 1;
        validator_write_commit(&storage, last, &bytes);
        let result = storage.validate_legacy_checkpoint_prefix(&records).unwrap();
        assert_eq!(result.next_generation, MAX_CHECKPOINT_GENERATIONS as u64);
        assert_eq!(result.committed.unwrap().generation, last);
        let mut writer = storage
            .open_typed_recovery_checkpoint(records, None)
            .unwrap();
        assert_eq!(
            writer.persist_next(0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
