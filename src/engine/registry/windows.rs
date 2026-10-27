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
    UnsupportedKeyState,
    MissingKey,
    ValueTooLarge,
    VerificationMismatch,
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

/// A registry backend whose lifetime is tied to an exclusive registry mutation
/// barrier. Construct it only through a transaction or recovery session.
pub struct WindowsRegistryBackend<'guard> {
    _guard: &'guard RegistryMutationLock,
}

impl<'guard> WindowsRegistryBackend<'guard> {
    pub(crate) fn new(guard: &'guard RegistryMutationLock) -> Self {
        Self { _guard: guard }
    }

    fn write_state(&mut self, target: &ValueTarget, state: &ValueState) -> Result<(), WriteError> {
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
mod tests {
    use super::*;

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
