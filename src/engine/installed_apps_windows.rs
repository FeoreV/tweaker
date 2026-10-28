//! Read-only inventory of Add/Remove Programs registry registrations.
//!
//! This is not equivalent to WinGet or Chocolatey inventory and does not
//! assign package IDs. Registry registrations may be incomplete or stale.

use crate::engine::installed_apps::{
    decode_registry_string, InstalledApplication, InstalledFieldError, InventoryReport,
    InventoryScope, ScopeReport, ScopeStatus,
};
use crate::engine::registry::{self, Hive, RegistryView, ValueState, ValueTarget};
use std::collections::BTreeMap;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INVALID_HANDLE, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA,
    ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};

const UNINSTALL_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
const MAX_SUBKEYS_PER_SCOPE: usize = 4096;
const MAX_SUBKEY_NAME_UNITS: usize = 32768;
const MAX_ENUM_ATTEMPTS: usize = 8;
const MAX_INVENTORY_BYTES_PER_SCOPE: usize = 64 * 1024 * 1024;
const INVENTORY_FIELDS: [&str; 3] = ["DisplayName", "DisplayVersion", "Publisher"];

struct OwnedKey(HKEY);

impl Drop for OwnedKey {
    fn drop(&mut self) {
        // SAFETY: this guard only wraps a validated, uniquely owned key from a
        // successful RegOpenKeyExW call and closes it exactly once.
        unsafe { RegCloseKey(self.0) };
    }
}

enum EnumOutcome {
    Name(Vec<u16>),
    End,
    MoreData,
    Error(u32),
}

#[derive(Debug, Eq, PartialEq)]
enum EnumScanError {
    Win32(u32),
    NameTooLong,
    InvalidName,
    LimitReached,
}

struct EnumScan {
    names: Vec<String>,
    error: Option<EnumScanError>,
}

fn collect_subkey_names(mut read_index: impl FnMut(u32, &mut [u16]) -> EnumOutcome) -> EnumScan {
    let mut names = Vec::new();
    let mut capacity = 256usize;
    let mut retries = 0usize;
    let mut index = 0u32;

    loop {
        if names.len() == MAX_SUBKEYS_PER_SCOPE {
            return match read_name(&mut read_index, index, &mut capacity, &mut retries) {
                Ok(None) => EnumScan { names, error: None },
                Ok(Some(_)) => EnumScan {
                    names,
                    error: Some(EnumScanError::LimitReached),
                },
                Err(error) => EnumScan {
                    names,
                    error: Some(error),
                },
            };
        }

        match read_name(&mut read_index, index, &mut capacity, &mut retries) {
            Ok(None) => return EnumScan { names, error: None },
            Ok(Some(units)) => {
                let name = match String::from_utf16(&units) {
                    Ok(name) => name,
                    Err(_) => {
                        return EnumScan {
                            names,
                            error: Some(EnumScanError::InvalidName),
                        }
                    }
                };
                if name.is_empty() || name.contains('\0') {
                    return EnumScan {
                        names,
                        error: Some(EnumScanError::InvalidName),
                    };
                }
                names.push(name);
                index = match index.checked_add(1) {
                    Some(index) => index,
                    None => {
                        return EnumScan {
                            names,
                            error: Some(EnumScanError::LimitReached),
                        }
                    }
                };
                retries = 0;
            }
            Err(error) => {
                return EnumScan {
                    names,
                    error: Some(error),
                }
            }
        }
    }
}

fn read_name(
    read_index: &mut impl FnMut(u32, &mut [u16]) -> EnumOutcome,
    index: u32,
    capacity: &mut usize,
    retries: &mut usize,
) -> Result<Option<Vec<u16>>, EnumScanError> {
    loop {
        let mut buffer = vec![0u16; *capacity];
        match read_index(index, &mut buffer) {
            EnumOutcome::End => return Ok(None),
            EnumOutcome::Name(units) => return Ok(Some(units)),
            EnumOutcome::Error(code) => return Err(EnumScanError::Win32(code)),
            EnumOutcome::MoreData => {
                *retries += 1;
                if *retries >= MAX_ENUM_ATTEMPTS || *capacity >= MAX_SUBKEY_NAME_UNITS {
                    return Err(EnumScanError::NameTooLong);
                }
                *capacity = (*capacity * 2).min(MAX_SUBKEY_NAME_UNITS);
            }
        }
    }
}

/// Read uninstall-registry registrations for HKLM/HKCU in the available views.
/// The function performs no writes, process launches, package-ID inference, or
/// deduplication. Scope-level failures and partial enumeration are explicit.
pub fn inventory_uninstall_registry() -> InventoryReport {
    let views = match available_views() {
        Ok(views) => views,
        Err(code) => {
            return InventoryReport {
                scopes: [Hive::LocalMachine, Hive::CurrentUser]
                    .into_iter()
                    .map(|hive| ScopeReport {
                        scope: InventoryScope {
                            hive,
                            view: RegistryView::Native,
                        },
                        status: ScopeStatus::Failed(code),
                        applications: Vec::new(),
                    })
                    .collect(),
            }
        }
    };

    let mut scopes = Vec::with_capacity(views.len() * 2);
    for hive in [Hive::LocalMachine, Hive::CurrentUser] {
        for view in &views {
            scopes.push(inventory_scope(hive.clone(), view.clone()));
        }
    }
    InventoryReport { scopes }
}

fn available_views() -> Result<Vec<RegistryView>, u32> {
    #[cfg(target_pointer_width = "64")]
    {
        Ok(vec![RegistryView::Registry64, RegistryView::Registry32])
    }
    #[cfg(target_pointer_width = "32")]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process};
        let mut is_wow64 = 0;
        // SAFETY: GetCurrentProcess returns a pseudo handle valid for this
        // process; IsWow64Process writes one BOOL to a valid local pointer.
        let succeeded = unsafe { IsWow64Process(GetCurrentProcess(), &mut is_wow64) };
        if succeeded == 0 {
            // SAFETY: GetLastError has no pointer arguments and is valid here.
            return Err(unsafe { windows_sys::Win32::Foundation::GetLastError() });
        }
        if is_wow64 != 0 {
            Ok(vec![RegistryView::Registry64, RegistryView::Registry32])
        } else {
            Ok(vec![RegistryView::Native])
        }
    }
}

fn inventory_scope(hive: Hive, view: RegistryView) -> ScopeReport {
    let scope = InventoryScope { hive, view };
    let root = match &scope.hive {
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        Hive::CurrentUser => HKEY_CURRENT_USER,
        _ => unreachable!("inventory only requests HKLM/HKCU"),
    };
    let path: Vec<u16> = UNINSTALL_KEY.encode_utf16().chain(Some(0)).collect();
    let mut raw: HKEY = null_mut();
    // SAFETY: root is a predefined hive handle, `path` is NUL-terminated, and
    // `raw` is writable. Only subkey-enumeration/query access is requested.
    let status = unsafe {
        RegOpenKeyExW(
            root,
            path.as_ptr(),
            0,
            KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE | view_flag(&scope.view),
            &mut raw,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return ScopeReport {
            scope,
            status: ScopeStatus::MissingKey,
            applications: Vec::new(),
        };
    }
    if status != ERROR_SUCCESS {
        return ScopeReport {
            scope,
            status: ScopeStatus::Failed(status),
            applications: Vec::new(),
        };
    }
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return ScopeReport {
            scope,
            status: ScopeStatus::Failed(ERROR_INVALID_HANDLE),
            applications: Vec::new(),
        };
    }
    let key = OwnedKey(raw);
    let scan = collect_subkey_names(|index, buffer| {
        let mut length = buffer.len() as u32;
        // SAFETY: key is guarded; buffer is writable for `length` UTF-16
        // units. Optional class/time outputs are unused and passed as null.
        let code = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                buffer.as_mut_ptr(),
                &mut length,
                null(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if code == ERROR_NO_MORE_ITEMS {
            EnumOutcome::End
        } else if code == ERROR_MORE_DATA {
            EnumOutcome::MoreData
        } else if code != ERROR_SUCCESS {
            EnumOutcome::Error(code)
        } else if length as usize > buffer.len() {
            EnumOutcome::Error(ERROR_MORE_DATA)
        } else {
            EnumOutcome::Name(buffer[..length as usize].to_vec())
        }
    });
    let names = scan.names;
    let enum_error = scan.error.map(|error| match error {
        EnumScanError::LimitReached => (true, ERROR_MORE_DATA),
        EnumScanError::NameTooLong => (false, ERROR_MORE_DATA),
        EnumScanError::InvalidName => (false, ERROR_INVALID_PARAMETER),
        EnumScanError::Win32(code) => (false, code),
    });

    let mut applications = Vec::with_capacity(names.len());
    let mut total_bytes = 0usize;
    let mut field_error = None;
    let mut budget_limited = false;
    'applications: for subkey in names {
        total_bytes = total_bytes.saturating_add(subkey.len());
        if total_bytes > MAX_INVENTORY_BYTES_PER_SCOPE {
            budget_limited = true;
            break;
        }
        let child_path: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
        let mut child_raw: HKEY = null_mut();
        // SAFETY: `key` owns the parent handle, the subkey is NUL-terminated,
        // and only query access is requested with the same explicit view.
        let child_status = unsafe {
            RegOpenKeyExW(
                key.0,
                child_path.as_ptr(),
                0,
                KEY_QUERY_VALUE | view_flag(&scope.view),
                &mut child_raw,
            )
        };
        let mut application = InstalledApplication {
            subkey,
            subkey_error: None,
            display_name: None,
            version: None,
            publisher: None,
            field_errors: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        if child_status != ERROR_SUCCESS || child_raw.is_null() || child_raw == INVALID_HANDLE_VALUE
        {
            let code = if child_status == ERROR_SUCCESS {
                ERROR_INVALID_HANDLE
            } else {
                child_status
            };
            field_error.get_or_insert(code);
            application.subkey_error = Some(code);
            applications.push(application);
            continue;
        }
        let child = OwnedKey(child_raw);
        let full_path = format!("{UNINSTALL_KEY}\\{}", application.subkey);
        for name in INVENTORY_FIELDS {
            let target = ValueTarget {
                hive: scope.hive.clone(),
                view: scope.view.clone(),
                key_path: full_path.clone(),
                value_name: name.to_owned(),
            };
            match registry::windows::read(&target) {
                Ok(ValueState::Present(value)) => {
                    total_bytes = total_bytes.saturating_add(value.bytes.len());
                    if total_bytes > MAX_INVENTORY_BYTES_PER_SCOPE {
                        budget_limited = true;
                        break;
                    }
                    match decode_registry_string(&value) {
                        Ok(value) => match name {
                            "DisplayName" => application.display_name = Some(value),
                            "DisplayVersion" => application.version = Some(value),
                            "Publisher" => application.publisher = Some(value),
                            _ => unreachable!(),
                        },
                        Err(error) => {
                            application
                                .field_errors
                                .insert(name.to_owned(), InstalledFieldError::InvalidString(error));
                        }
                    }
                }
                Ok(ValueState::ValueMissing) => {}
                Ok(ValueState::KeyMissing) => {
                    field_error.get_or_insert(ERROR_FILE_NOT_FOUND);
                    application.field_errors.insert(
                        name.to_owned(),
                        InstalledFieldError::RegistryReadFailed(ERROR_FILE_NOT_FOUND),
                    );
                }
                Err(error) => {
                    let code = match error {
                        registry::windows::ReadError::Win32(code) => code,
                        registry::windows::ReadError::InvalidHandle => ERROR_INVALID_HANDLE,
                        registry::windows::ReadError::ValueTooLarge
                        | registry::windows::ReadError::ChangedTooOften => ERROR_MORE_DATA,
                        registry::windows::ReadError::InvalidName
                        | registry::windows::ReadError::RootKeyUnsupported => {
                            ERROR_INVALID_PARAMETER
                        }
                    };
                    field_error.get_or_insert(code);
                    application.field_errors.insert(
                        name.to_owned(),
                        InstalledFieldError::RegistryReadFailed(code),
                    );
                }
            }
        }
        if budget_limited {
            break 'applications;
        }
        drop(child);
        applications.push(application);
    }
    drop(key);

    let status = if budget_limited {
        ScopeStatus::LimitReached
    } else {
        match enum_error {
            Some((true, _)) => ScopeStatus::LimitReached,
            Some((false, code)) => {
                if applications.is_empty() {
                    ScopeStatus::Failed(code)
                } else {
                    ScopeStatus::Partial(code)
                }
            }
            None if field_error.is_some() => ScopeStatus::Partial(field_error.unwrap()),
            None => ScopeStatus::Complete,
        }
    };
    ScopeReport {
        scope,
        status,
        applications,
    }
}

fn view_flag(view: &RegistryView) -> u32 {
    match view {
        RegistryView::Native => 0,
        RegistryView::Registry64 => KEY_WOW64_64KEY,
        RegistryView::Registry32 => KEY_WOW64_32KEY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn enumeration_preserves_names_and_ends_on_no_more_items() {
        let calls = Cell::new(0u32);
        let names = collect_subkey_names(|index, _| {
            calls.set(calls.get() + 1);
            match index {
                0 => EnumOutcome::Name("First".encode_utf16().collect()),
                1 => EnumOutcome::Name("Second".encode_utf16().collect()),
                _ => EnumOutcome::End,
            }
        });
        assert_eq!(names.names, ["First", "Second"]);
        assert_eq!(names.error, None);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn enumeration_retries_a_growing_key_name_and_is_bounded() {
        let calls = Cell::new(0usize);
        let expected: Vec<u16> = "A long key name".encode_utf16().collect();
        let names = collect_subkey_names(|index, buffer| {
            calls.set(calls.get() + 1);
            if index > 0 {
                EnumOutcome::End
            } else if buffer.len() < expected.len() {
                EnumOutcome::MoreData
            } else {
                EnumOutcome::Name(expected.clone())
            }
        });
        assert_eq!(names.names, ["A long key name"]);
        assert_eq!(names.error, None);
        assert_eq!(calls.get(), 2);

        let too_long = collect_subkey_names(|_, _| EnumOutcome::MoreData);
        assert!(too_long.names.is_empty());
        assert_eq!(too_long.error, Some(EnumScanError::NameTooLong));
    }

    #[test]
    fn enumeration_reports_malformed_names_and_api_errors() {
        let invalid = collect_subkey_names(|_, _| EnumOutcome::Name(vec![0xd800]));
        assert!(invalid.names.is_empty());
        assert_eq!(invalid.error, Some(EnumScanError::InvalidName));
        let failed = collect_subkey_names(|_, _| EnumOutcome::Error(5));
        assert!(failed.names.is_empty());
        assert_eq!(failed.error, Some(EnumScanError::Win32(5)));
    }
}
