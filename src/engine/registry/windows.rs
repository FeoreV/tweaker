//! Win32 registry access. Mutations require a journal session and registry lock.
pub use super::ValueState as ReadState;
use super::{Hive, RegistryValue, RegistryView, ValueState, ValueTarget};
use crate::data::file_store::RegistryMutationLock;
use crate::engine::recovery::RecoveryBackend;
use crate::engine::transaction::Backend;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CLASSES_ROOT, HKEY_CURRENT_CONFIG, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS,
    KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};

const MAX_VALUE_BYTES: u32 = 16 * 1024 * 1024;
const MAX_ATTEMPTS: usize = 8;

#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    InvalidName,
    RootKeyUnsupported,
    InvalidHandle,
    Win32(u32),
    ValueTooLarge,
    ChangedTooOften,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WriteError {
    InvalidName,
    RootKeyUnsupported,
    InvalidHandle,
    UnsupportedView,
    UnsupportedHive,
    UnsupportedKeyState,
    MissingKey,
    ValueTooLarge,
    VerificationMismatch,
    Conflict,
    Win32(u32),
}
struct OwnedKey(HKEY);
impl Drop for OwnedKey {
    fn drop(&mut self) {
        // SAFETY: this is a non-null handle returned by a successful open of a
        // nonempty subkey, uniquely owned by this guard and closed only here.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn wide(s: &str) -> Result<Vec<u16>, ReadError> {
    if s.contains('\0') {
        return Err(ReadError::InvalidName);
    }
    Ok(s.encode_utf16().chain(std::iter::once(0)).collect())
}

fn wide_for_write(s: &str) -> Result<Vec<u16>, WriteError> {
    if s.contains('\0') {
        return Err(WriteError::InvalidName);
    }
    Ok(s.encode_utf16().chain(std::iter::once(0)).collect())
}

fn write_target(target: &ValueTarget) -> Result<ValueTarget, WriteError> {
    // HKCR merges per-user and per-machine classes. A visible value snapshot
    // does not identify the physical layer that a subsequent write changes.
    // https://learn.microsoft.com/en-us/windows/win32/sysinfo/hkey-classes-root-key
    // HKCU and HKCC also resolve through the current user/profile context;
    // the journal does not bind either identity across later recovery.
    if matches!(
        target.hive,
        Hive::ClassesRoot | Hive::CurrentUser | Hive::CurrentConfig | Hive::Users
    ) {
        return Err(WriteError::UnsupportedHive);
    }
    #[cfg(target_pointer_width = "32")]
    {
        let mut effective = target.clone();
        match target.view {
            // A 32-bit process must explicitly bypass WoW64 redirection for
            // every write. The Native spelling therefore means the 64-bit view.
            RegistryView::Native | RegistryView::Registry64 => {
                effective.view = RegistryView::Registry64;
            }
            RegistryView::Registry32 => return Err(WriteError::UnsupportedView),
        }
        Ok(effective)
    }
    #[cfg(not(target_pointer_width = "32"))]
    {
        Ok(target.clone())
    }
}

fn root_key(hive: &Hive) -> HKEY {
    match hive {
        Hive::CurrentUser => HKEY_CURRENT_USER,
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        Hive::Users => HKEY_USERS,
        Hive::ClassesRoot => HKEY_CLASSES_ROOT,
        Hive::CurrentConfig => HKEY_CURRENT_CONFIG,
    }
}

fn view_flags(view: &RegistryView) -> u32 {
    match view {
        RegistryView::Native => 0,
        RegistryView::Registry64 => KEY_WOW64_64KEY,
        RegistryView::Registry32 => KEY_WOW64_32KEY,
    }
}

fn state_matches(expected: &ValueState, actual: &ValueState) -> bool {
    expected == actual
        || matches!(
            (expected, actual),
            (ValueState::ValueMissing, ValueState::KeyMissing)
        )
}

// The journal barrier only coordinates cooperating sessions. Re-read before a write so
// changes by other actors observed after the snapshot are not overwritten.
// This is not atomic with the subsequent Win32 write and cannot detect ABA.
fn check_observed<F>(
    observed: Option<(ValueTarget, ValueState)>,
    target: &ValueTarget,
    query: F,
) -> Result<(), WriteError>
where
    F: FnOnce(&ValueTarget) -> Result<ValueState, WriteError>,
{
    let Some((observed_target, before)) = observed else {
        return Err(WriteError::Conflict);
    };
    if observed_target != *target || query(target)? != before {
        return Err(WriteError::Conflict);
    }
    Ok(())
}

fn guarded_write<Q, W>(
    observed: Option<(ValueTarget, ValueState)>,
    target: &ValueTarget,
    state: &ValueState,
    query: Q,
    write: W,
) -> Result<(), WriteError>
where
    Q: FnOnce(&ValueTarget) -> Result<ValueState, WriteError>,
    W: FnOnce(&ValueTarget, &ValueState) -> Result<(), WriteError>,
{
    check_observed(observed, target, query)?;
    write(target, state)
}

fn read_for_write(target: &ValueTarget) -> Result<ValueState, WriteError> {
    let effective_target = write_target(target)?;
    read(&effective_target).map_err(|error| match error {
        ReadError::InvalidName => WriteError::InvalidName,
        ReadError::RootKeyUnsupported => WriteError::RootKeyUnsupported,
        ReadError::InvalidHandle => WriteError::InvalidHandle,
        ReadError::Win32(code) => WriteError::Win32(code),
        ReadError::ValueTooLarge => WriteError::ValueTooLarge,
        ReadError::ChangedTooOften => WriteError::Win32(ERROR_MORE_DATA),
    })
}

/// A registry backend whose lifetime is tied to an exclusive registry mutation
/// barrier. Construct it only through a transaction or recovery session.
pub struct WindowsRegistryBackend<'guard> {
    #[cfg(not(test))]
    _guard: &'guard RegistryMutationLock,
    #[cfg(test)]
    _guard: Option<&'guard RegistryMutationLock>,
    #[cfg(test)]
    mock: Option<std::rc::Rc<std::cell::RefCell<MockRegistry>>>,
    observed: Option<(ValueTarget, ValueState)>,
}

impl<'guard> WindowsRegistryBackend<'guard> {
    pub(crate) fn new(guard: &'guard RegistryMutationLock) -> Self {
        Self {
            #[cfg(not(test))]
            _guard: guard,
            #[cfg(test)]
            _guard: Some(guard),
            #[cfg(test)]
            mock: None,
            observed: None,
        }
    }

    fn write_state(&mut self, target: &ValueTarget, state: &ValueState) -> Result<(), WriteError> {
        let observed = self.observed.take();
        #[cfg(test)]
        if let Some(mock) = &self.mock {
            let mock = std::rc::Rc::clone(mock);
            let writer = std::rc::Rc::clone(&mock);
            return guarded_write(
                observed,
                target,
                state,
                move |_| mock.borrow_mut().query(),
                move |_, state| writer.borrow_mut().write(state),
            );
        }
        guarded_write(observed, target, state, read_for_write, |target, state| {
            self.write_state_unchecked(target, state)
        })
    }

    fn write_state_unchecked(
        &mut self,
        target: &ValueTarget,
        state: &ValueState,
    ) -> Result<(), WriteError> {
        let effective_target = write_target(target)?;
        if matches!(state, ValueState::KeyMissing) {
            return Err(WriteError::UnsupportedKeyState);
        }
        if effective_target.key_path.is_empty() {
            return Err(WriteError::RootKeyUnsupported);
        }
        let path = wide_for_write(&effective_target.key_path)?;
        let name = wide_for_write(&effective_target.value_name)?;
        let mut raw: HKEY = null_mut();
        // Open existing keys only. Creating a missing key would make exact
        // rollback require deleting a key whose other contents may change.
        let status = unsafe {
            RegOpenKeyExW(
                root_key(&effective_target.hive),
                path.as_ptr(),
                0,
                KEY_SET_VALUE | view_flags(&effective_target.view),
                &mut raw,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return match state {
                ValueState::ValueMissing => Ok(()),
                ValueState::Present(_) => Err(WriteError::MissingKey),
                ValueState::KeyMissing => unreachable!(),
            };
        }
        if status != ERROR_SUCCESS {
            return Err(WriteError::Win32(status));
        }
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(WriteError::InvalidHandle);
        }
        let key = OwnedKey(raw);
        match state {
            ValueState::KeyMissing => unreachable!(),
            ValueState::ValueMissing => {
                let status = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
                if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
                    Ok(())
                } else {
                    Err(WriteError::Win32(status))
                }
            }
            ValueState::Present(value) => {
                if value.bytes.len() > MAX_VALUE_BYTES as usize {
                    return Err(WriteError::ValueTooLarge);
                }
                let data = if value.bytes.is_empty() {
                    null()
                } else {
                    value.bytes.as_ptr()
                };
                let status = unsafe {
                    RegSetValueExW(
                        key.0,
                        name.as_ptr(),
                        0,
                        value.native_type,
                        data,
                        value.bytes.len() as u32,
                    )
                };
                if status == ERROR_SUCCESS {
                    Ok(())
                } else {
                    Err(WriteError::Win32(status))
                }
            }
        }
    }
}

impl Backend for WindowsRegistryBackend<'_> {
    type Target = ValueTarget;
    type Change = ValueState;
    type Snapshot = ValueState;
    type Error = WriteError;

    fn query(&mut self, target: &ValueTarget) -> Result<ValueState, Self::Error> {
        self.observed = None;
        #[cfg(test)]
        let state = if let Some(mock) = &self.mock {
            mock.borrow_mut().query()?
        } else {
            read_for_write(target)?
        };
        #[cfg(not(test))]
        let state = read_for_write(target)?;
        self.observed = Some((target.clone(), state.clone()));
        Ok(state)
    }

    fn apply(&mut self, target: &ValueTarget, change: &ValueState) -> Result<(), Self::Error> {
        self.write_state(target, change)?;
        let current = <Self as Backend>::query(self, target)?;
        if state_matches(change, &current) {
            Ok(())
        } else {
            Err(WriteError::VerificationMismatch)
        }
    }
}

impl RecoveryBackend for WindowsRegistryBackend<'_> {
    type Target = ValueTarget;
    type State = ValueState;
    type Error = WriteError;

    fn query(&mut self, target: &ValueTarget) -> Result<Self::State, Self::Error> {
        <Self as Backend>::query(self, target)
    }

    fn restore(&mut self, target: &ValueTarget, state: &Self::State) -> Result<(), Self::Error> {
        self.write_state(target, state)
    }
}

/// Reads without creating keys or modifying registry values.
/// Missing key and missing value are intentionally distinct.
pub fn read(target: &ValueTarget) -> Result<ReadState, ReadError> {
    if target.key_path.is_empty() {
        return Err(ReadError::RootKeyUnsupported);
    }
    let path = wide(&target.key_path)?;
    let name = wide(&target.value_name)?;
    let root = match target.hive {
        Hive::CurrentUser => HKEY_CURRENT_USER,
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        Hive::Users => HKEY_USERS,
        Hive::ClassesRoot => HKEY_CLASSES_ROOT,
        Hive::CurrentConfig => HKEY_CURRENT_CONFIG,
    };
    let view = match target.view {
        RegistryView::Native => 0,
        RegistryView::Registry64 => KEY_WOW64_64KEY,
        RegistryView::Registry32 => KEY_WOW64_32KEY,
    };
    let mut raw: HKEY = null_mut();
    // SAFETY: root is predefined, path is a terminated UTF-16 string, and raw
    // points to writable storage. Only query access is requested.
    let status = unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_QUERY_VALUE | view, &mut raw) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(ReadState::KeyMissing);
    }
    if status != ERROR_SUCCESS {
        return Err(ReadError::Win32(status));
    }
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(ReadError::InvalidHandle);
    }
    let key = OwnedKey(raw);
    for _ in 0..MAX_ATTEMPTS {
        let mut size = 0u32;
        let mut ty = 0u32;
        // SAFETY: key is valid for the guard lifetime, name is terminated,
        // and output pointers refer to writable local variables.
        let status = unsafe {
            RegQueryValueExW(key.0, name.as_ptr(), null(), &mut ty, null_mut(), &mut size)
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(ReadState::ValueMissing);
        }
        if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
            return Err(ReadError::Win32(status));
        }
        if size > MAX_VALUE_BYTES {
            return Err(ReadError::ValueTooLarge);
        }
        // Allocate at least one byte so the data query is not interpreted as a size-only query.
        let mut bytes = vec![0u8; (size as usize).max(1)];
        let mut capacity = bytes.len() as u32;
        // SAFETY: buffer has capacity bytes available and the remaining pointers
        // are valid. The returned count is checked before truncating.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                null(),
                &mut ty,
                bytes.as_mut_ptr(),
                &mut capacity,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(ReadState::ValueMissing);
        }
        if status == ERROR_MORE_DATA {
            continue;
        }
        if status != ERROR_SUCCESS {
            return Err(ReadError::Win32(status));
        }
        if capacity as usize > bytes.len() {
            return Err(ReadError::ValueTooLarge);
        }
        bytes.truncate(capacity as usize);
        return Ok(ReadState::Present(RegistryValue {
            native_type: ty,
            bytes,
        }));
    }
    Err(ReadError::ChangedTooOften)
}

#[cfg(test)]
struct MockRegistry {
    state: ValueState,
    reads: usize,
    writes: usize,
    change_on_second_read: Option<ValueState>,
    marker: Option<std::path::PathBuf>,
    marker_seen: bool,
}

#[cfg(test)]
impl MockRegistry {
    fn query(&mut self) -> Result<ValueState, WriteError> {
        self.reads += 1;
        if self.reads == 2 {
            self.marker_seen = self.marker.as_ref().is_some_and(|path| path.is_file());
            if let Some(state) = self.change_on_second_read.take() {
                self.state = state;
            }
        }
        Ok(self.state.clone())
    }

    fn write(&mut self, state: &ValueState) -> Result<(), WriteError> {
        self.writes += 1;
        self.state = state.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // This adapter uses the same registry backend methods as WindowsTypedBackend,
    // while the session owns the real journal and mutation barrier.
    struct TypedTestBackend(WindowsRegistryBackend<'static>);

    impl Backend for TypedTestBackend {
        type Target = crate::data::store::TypedTarget;
        type Change = crate::data::store::TypedState;
        type Snapshot = crate::data::store::TypedState;
        type Error = WriteError;

        fn query(&mut self, target: &Self::Target) -> Result<Self::Snapshot, Self::Error> {
            let crate::data::store::TypedTarget::Registry(target) = target else {
                panic!("registry only")
            };
            Backend::query(&mut self.0, target).map(crate::data::store::TypedState::Registry)
        }

        fn apply(
            &mut self,
            target: &Self::Target,
            state: &Self::Change,
        ) -> Result<(), Self::Error> {
            let (
                crate::data::store::TypedTarget::Registry(target),
                crate::data::store::TypedState::Registry(state),
            ) = (target, state)
            else {
                panic!("registry only")
            };
            RecoveryBackend::restore(&mut self.0, target, state)?;
            if Backend::query(&mut self.0, target)? == *state {
                Ok(())
            } else {
                Err(WriteError::VerificationMismatch)
            }
        }
    }

    impl crate::engine::recovery::TypedRecoveryBackend for TypedTestBackend {
        type Error = WriteError;
        fn query_typed(
            &mut self,
            target: &crate::data::store::TypedTarget,
        ) -> Result<crate::data::store::TypedState, Self::Error> {
            Backend::query(self, target)
        }
        fn restore_typed(
            &mut self,
            target: &crate::data::store::TypedTarget,
            state: &crate::data::store::TypedState,
        ) -> Result<(), Self::Error> {
            let (
                crate::data::store::TypedTarget::Registry(target),
                crate::data::store::TypedState::Registry(state),
            ) = (target, state)
            else {
                panic!("registry only")
            };
            RecoveryBackend::restore(&mut self.0, target, state)
        }
    }

    struct TestRoot(std::path::PathBuf);
    impl TestRoot {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static ID: AtomicU64 = AtomicU64::new(0);
            let path =
                std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(format!(
                    "rustweaker-registry-guard-{}-{}",
                    std::process::id(),
                    ID.fetch_add(1, Ordering::Relaxed)
                ));
            crate::data::file_store::FileJournalStorage::create_private(&path).unwrap();
            Self(path)
        }
        fn marker(&self, id: &str) -> std::path::PathBuf {
            self.0
                .join(format!("{:03}-{id}-{:020}.commit", id.len(), 0))
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn mock_typed(
        state: ValueState,
        changed: Option<ValueState>,
        marker: std::path::PathBuf,
    ) -> (
        TypedTestBackend,
        std::rc::Rc<std::cell::RefCell<MockRegistry>>,
    ) {
        let mock = std::rc::Rc::new(std::cell::RefCell::new(MockRegistry {
            state,
            reads: 0,
            writes: 0,
            change_on_second_read: changed,
            marker: Some(marker),
            marker_seen: false,
        }));
        (
            TypedTestBackend(WindowsRegistryBackend {
                _guard: None,
                mock: Some(mock.clone()),
                observed: None,
            }),
            mock,
        )
    }
    fn typed_operation() -> crate::engine::transaction::Operation<
        crate::data::store::TypedTarget,
        crate::data::store::TypedState,
    > {
        use crate::data::store::{TypedState, TypedTarget};
        crate::engine::transaction::Operation {
            target: TypedTarget::Registry(ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Value".into(),
            }),
            change: TypedState::Registry(ValueState::Present(RegistryValue {
                native_type: 4,
                bytes: vec![1, 0, 0, 0],
            })),
        }
    }
    fn external_value() -> ValueState {
        ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![2, 0, 0, 0],
        })
    }

    #[test]
    fn managed_typed_registry_apply_conflict_follows_persist_and_never_writes() {
        use crate::data::file_store::{FileJournalStorage, FileTypedTransactionError};
        use crate::data::store::JournalStorage;
        use crate::engine::transaction::TransactionError;
        let root = TestRoot::new();
        let id = "tx_registry_conflict";
        let (mut backend, mock) = mock_typed(
            ValueState::ValueMissing,
            Some(external_value()),
            root.marker(id),
        );
        let mut session = FileJournalStorage::start_typed_transaction(&root.0, id).unwrap();
        assert!(matches!(
            session.apply(&mut backend, &typed_operation()),
            Err(FileTypedTransactionError::Operation(
                TransactionError::Apply(WriteError::Conflict)
            ))
        ));
        assert!(mock.borrow().marker_seen);
        assert_eq!(mock.borrow().writes, 0);
        assert_eq!(mock.borrow().state, external_value());
        drop(session);
        let mut storage = FileJournalStorage::open(&root.0)
            .unwrap()
            .lock_transaction(id)
            .unwrap();
        assert_eq!(storage.scan(id).unwrap().typed_committed.len(), 1);
    }

    #[test]
    fn managed_typed_registry_recovery_conflict_follows_persist_and_never_writes() {
        use crate::data::file_store::{FileJournalStorage, FileTypedRecoverySession};
        use crate::engine::recovery::{BatchRecoveryError, JournalRecoveryError, RecoveryError};
        let root = TestRoot::new();
        let source = "tx_registry_source";
        let recovery = "tx_registry_restore";
        let (mut initial, _) = mock_typed(ValueState::ValueMissing, None, root.marker(source));
        let operation = typed_operation();
        let mut session = FileJournalStorage::start_typed_transaction(&root.0, source).unwrap();
        session.apply(&mut initial, &operation).unwrap();
        drop(session);
        let crate::data::store::TypedState::Registry(intended) = operation.change else {
            unreachable!()
        };
        let (mut backend, mock) =
            mock_typed(intended, Some(external_value()), root.marker(recovery));
        let mut session =
            FileTypedRecoverySession::start_with_id(&root.0, source, recovery).unwrap();
        assert!(matches!(
            session.run(&mut backend),
            Err(JournalRecoveryError::Recovery(
                BatchRecoveryError::Operation {
                    error: RecoveryError::Restore(WriteError::Conflict),
                    ..
                }
            ))
        ));
        assert!(mock.borrow().marker_seen);
        assert_eq!(mock.borrow().writes, 0);
        assert_eq!(mock.borrow().state, external_value());
        assert_eq!(session.next_exclusive(), 1);
    }

    #[test]
    fn pre_write_check_rejects_changed_or_unobserved_registry_state() {
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Value".into(),
        };
        let before = ValueState::ValueMissing;
        let changed = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1],
        });
        assert_eq!(
            check_observed(Some((target.clone(), before.clone())), &target, |_| Ok(
                changed
            )),
            Err(WriteError::Conflict)
        );
        assert_eq!(
            check_observed(None, &target, |_| panic!("must not query")),
            Err(WriteError::Conflict)
        );
        assert_eq!(
            check_observed(Some((target.clone(), before.clone())), &target, |_| Err(
                WriteError::Win32(5)
            )),
            Err(WriteError::Win32(5))
        );
        assert_eq!(
            check_observed(Some((target.clone(), before.clone())), &target, |_| Ok(
                before
            )),
            Ok(())
        );
    }

    #[test]
    fn guarded_write_never_calls_mutation_on_conflict_or_query_failure() {
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Registry64,
            key_path: "Software\\Example".into(),
            value_name: "Value".into(),
        };
        let before = ValueState::ValueMissing;
        let desired = ValueState::Present(RegistryValue {
            native_type: 4,
            bytes: vec![1],
        });
        let mut writes = 0;
        for result in [Ok(desired.clone()), Err(WriteError::Win32(5))] {
            assert!(guarded_write(
                Some((target.clone(), before.clone())),
                &target,
                &desired,
                |_| result,
                |_, _| {
                    writes += 1;
                    Ok(())
                }
            )
            .is_err());
        }
        assert_eq!(writes, 0);
        guarded_write(
            Some((target.clone(), before.clone())),
            &target,
            &desired,
            |_| Ok(before),
            |_, _| {
                writes += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(writes, 1);
    }

    #[test]
    fn context_dependent_hives_are_rejected_for_mutation_before_read_or_write() {
        for hive in [
            Hive::ClassesRoot,
            Hive::CurrentUser,
            Hive::CurrentConfig,
            Hive::Users,
        ] {
            let target = ValueTarget {
                hive,
                view: RegistryView::Registry64,
                key_path: "Software\\Example".into(),
                value_name: "Value".into(),
            };
            assert_eq!(write_target(&target), Err(WriteError::UnsupportedHive));
            assert_eq!(read_for_write(&target), Err(WriteError::UnsupportedHive));
            let mut backend = WindowsRegistryBackend {
                _guard: None,
                mock: None,
                observed: None,
            };
            assert_eq!(
                Backend::query(&mut backend, &target),
                Err(WriteError::UnsupportedHive)
            );
            assert_eq!(
                RecoveryBackend::query(&mut backend, &target),
                Err(WriteError::UnsupportedHive)
            );
            // Read-only access has its own path and remains available; embedded
            // NUL shows it reaches name validation instead of the mutation gate.
            let mut invalid = target;
            invalid.key_path.push('\0');
            assert_eq!(read(&invalid), Err(ReadError::InvalidName));
        }
    }

    #[test]
    fn users_hive_backend_queries_reject_before_reaching_mock_or_win32() {
        let target = ValueTarget {
            hive: Hive::Users,
            view: RegistryView::Registry64,
            key_path: "S-1-5-21-1\\Software\\Example".into(),
            value_name: "Value".into(),
        };
        let mut backend = WindowsRegistryBackend {
            _guard: None,
            mock: None,
            observed: None,
        };
        assert_eq!(
            Backend::query(&mut backend, &target),
            Err(WriteError::UnsupportedHive)
        );
        assert_eq!(
            RecoveryBackend::query(&mut backend, &target),
            Err(WriteError::UnsupportedHive)
        );
        // Read/status uses the independent read path; it proceeds to Win32
        // lookup and returns an ordinary missing/query result.
        assert!(matches!(
            read(&target),
            Ok(ReadState::KeyMissing) | Err(ReadError::Win32(_))
        ));
    }

    #[test]
    fn write_view_never_silently_uses_wow64_native_redirection() {
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Native,
            key_path: "Software\\Example".into(),
            value_name: "Value".into(),
        };
        #[cfg(target_pointer_width = "32")]
        {
            assert_eq!(
                write_target(&target).unwrap().view,
                RegistryView::Registry64
            );
            let mut registry32 = target.clone();
            registry32.view = RegistryView::Registry32;
            assert_eq!(write_target(&registry32), Err(WriteError::UnsupportedView));
        }
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(write_target(&target).unwrap().view, RegistryView::Native);
            let mut registry32 = target;
            registry32.view = RegistryView::Registry32;
            assert_eq!(
                write_target(&registry32).unwrap().view,
                RegistryView::Registry32
            );
        }
    }

    #[test]
    fn missing_key_satisfies_missing_value_but_not_present_state() {
        assert!(state_matches(
            &ValueState::ValueMissing,
            &ValueState::KeyMissing
        ));
        assert!(!state_matches(
            &ValueState::Present(RegistryValue {
                native_type: 4,
                bytes: vec![],
            }),
            &ValueState::KeyMissing
        ));
    }

    #[test]
    fn invalid_names_are_rejected_without_api_calls() {
        let mut target = ValueTarget {
            hive: Hive::CurrentUser,
            view: RegistryView::Native,
            key_path: "bad\0key".into(),
            value_name: String::new(),
        };
        assert_eq!(read(&target), Err(ReadError::InvalidName));
        target.key_path.clear();
        assert_eq!(read(&target), Err(ReadError::RootKeyUnsupported));
    }
    #[test]
    fn reads_existing_windows_version_key_without_writing() {
        let target = ValueTarget {
            hive: Hive::LocalMachine,
            view: RegistryView::Native,
            key_path: "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion".into(),
            value_name: "SystemRoot".into(),
        };
        let state = read(&target).expect("read SystemRoot");
        assert!(matches!(
            state,
            ReadState::Present(RegistryValue { native_type: 1, .. })
        ));
    }
}
