//! Private Windows directory creation and ACL validation for journal roots.
//!
//! Existing ACLs are only inspected. `create_private_root` creates an absent
//! leaf with a protected owner/SYSTEM DACL and rejects unsafe ancestors.

use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
    GetTokenInformation, IsValidAcl, IsWellKnownSid, LookupAccountNameW, TokenOwner,
    WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE,
    ACCESS_DENIED_ACE, ACE_HEADER, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    SECURITY_ATTRIBUTES, SE_DACL_PROTECTED, TOKEN_OWNER, TOKEN_QUERY, WELL_KNOWN_SID_TYPE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, FileAttributeTagInfo, GetDriveTypeW,
    GetFileInformationByHandleEx, GetVolumeInformationW, GetVolumePathNameW, RemoveDirectoryW,
    FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, READ_CONTROL,
};
use windows_sys::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE};

const GENERIC_DELETE_CHILD: u32 = FILE_DELETE_CHILD | 0x0001_0000;
const GENERIC_WRITE_DAC_OWNER: u32 = 0x0004_0000 | 0x0008_0000;
const DANGEROUS_PARENT_RIGHTS: u32 = GENERIC_ALL | GENERIC_DELETE_CHILD | GENERIC_WRITE_DAC_OWNER;
const ACE_ACCESS_ALLOWED: u8 = 0;
const ACE_ACCESS_DENIED: u8 = 1;
const OBJECT_INHERIT_ACE: u8 = 0x01;
const CONTAINER_INHERIT_ACE: u8 = 0x02;
const INHERIT_ONLY_ACE: u8 = 0x08;
const TRUSTED_INSTALLER_ACCOUNT: &str = "NT SERVICE\\TrustedInstaller";
const ROOT_STORAGE_ACCESS: u32 = 0x0012_01C3;
const FILE_STORAGE_ACCESS: u32 = 0x0013_019F;
const PRIVATE_ROOT_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;;0x001201C3;;;OW)(A;OIIO;0x0013019F;;;OW)";

struct OwnedSecurityDescriptor(*mut core::ffi::c_void);

impl Drop for OwnedSecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::LocalFree(self.0);
            }
        }
    }
}

struct OwnedToken(HANDLE);

impl Drop for OwnedToken {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

struct ProcessOwner {
    storage: Vec<usize>,
    sid: *mut core::ffi::c_void,
}

impl ProcessOwner {
    fn current() -> io::Result<Self> {
        let mut token = std::ptr::null_mut();
        let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
        if opened == 0 || token.is_null() || token == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedToken(token);

        let mut required = 0u32;
        unsafe {
            GetTokenInformation(token.0, TokenOwner, std::ptr::null_mut(), 0, &mut required);
        }
        if required < std::mem::size_of::<TOKEN_OWNER>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let words = required.div_ceil(std::mem::size_of::<usize>() as u32) as usize;
        let mut storage = vec![0usize; words];
        let result = unsafe {
            GetTokenInformation(
                token.0,
                TokenOwner,
                storage.as_mut_ptr().cast(),
                (storage.len() * std::mem::size_of::<usize>()) as u32,
                &mut required,
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        let sid = unsafe { (*(storage.as_ptr().cast::<TOKEN_OWNER>())).Owner };
        if sid.is_null() {
            return Err(invalid_security("process token has no owner SID"));
        }
        Ok(Self { storage, sid })
    }

    fn sid(&self) -> *mut core::ffi::c_void {
        let _keep_storage_alive = &self.storage;
        self.sid
    }
}

struct TrustedInstallerSid([usize; 16]);

impl TrustedInstallerSid {
    fn resolve() -> io::Result<Self> {
        let account: Vec<u16> = TRUSTED_INSTALLER_ACCOUNT
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut sid = [0usize; 16];
        let mut sid_bytes = std::mem::size_of_val(&sid) as u32;
        let mut domain = [0u16; 256];
        let mut domain_chars = domain.len() as u32;
        let mut sid_type: WELL_KNOWN_SID_TYPE = 0;
        let result = unsafe {
            LookupAccountNameW(
                std::ptr::null(),
                account.as_ptr(),
                sid.as_mut_ptr().cast(),
                &mut sid_bytes,
                domain.as_mut_ptr(),
                &mut domain_chars,
                &mut sid_type,
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(sid))
    }

    fn sid(&self) -> *mut core::ffi::c_void {
        self.0.as_ptr() as *mut core::ffi::c_void
    }
}

struct SecurityContext {
    owner: ProcessOwner,
    trusted_installer: TrustedInstallerSid,
}

impl SecurityContext {
    fn current() -> io::Result<Self> {
        Ok(Self {
            owner: ProcessOwner::current()?,
            trusted_installer: TrustedInstallerSid::resolve()?,
        })
    }

    fn is_sid(&self, left: *mut core::ffi::c_void, right: *mut core::ffi::c_void) -> bool {
        !left.is_null()
            && !right.is_null()
            && unsafe { windows_sys::Win32::Security::EqualSid(left, right) != 0 }
    }

    fn is_well_known(&self, sid: *mut core::ffi::c_void, kind: WELL_KNOWN_SID_TYPE) -> bool {
        !sid.is_null() && unsafe { IsWellKnownSid(sid, kind) != 0 }
    }

    fn is_trusted_principal(&self, sid: *mut core::ffi::c_void) -> bool {
        self.is_sid(sid, self.owner.sid())
            || self.is_sid(sid, self.trusted_installer.sid())
            || self.is_well_known(sid, WinLocalSystemSid)
            || self.is_well_known(sid, WinBuiltinAdministratorsSid)
    }

    fn is_trusted_owner(&self, sid: *mut core::ffi::c_void) -> bool {
        self.is_trusted_principal(sid)
    }
}

pub(super) fn create_private_root(root: &Path) -> io::Result<(PathBuf, Vec<File>)> {
    create_private_root_with_sddl(root, PRIVATE_ROOT_SDDL)
}

fn create_private_root_with_sddl(root: &Path, sddl: &str) -> io::Result<(PathBuf, Vec<File>)> {
    let paths = directory_chain(root)?;
    if paths.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "journal root must be below an existing parent directory",
        ));
    }
    validate_storage_volume(&paths[paths.len() - 2])?;
    let context = SecurityContext::current()?;
    let parent_guards = open_and_validate_chain(&paths[..paths.len() - 1], &context, false)?;

    match std::fs::symlink_metadata(paths.last().expect("non-empty directory chain")) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "private journal root must not already exist",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let descriptor = security_descriptor_from_sddl(sddl)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let path = wide_path(paths.last().expect("non-empty directory chain"));
    let created = unsafe { CreateDirectoryW(path.as_ptr(), &attributes) };
    drop(parent_guards);
    if created == 0 {
        return Err(io::Error::last_os_error());
    }

    match open_private_root(root) {
        Ok(opened) => Ok(opened),
        Err(validation_error) => {
            let path = wide_path(paths.last().expect("non-empty directory chain"));
            let removed = unsafe { RemoveDirectoryW(path.as_ptr()) };
            if removed == 0 {
                let cleanup_error = io::Error::last_os_error();
                return Err(io::Error::new(
                    validation_error.kind(),
                    format!(
                        "new journal root failed ACL validation ({validation_error}); empty-root cleanup also failed ({cleanup_error})"
                    ),
                ));
            }
            Err(validation_error)
        }
    }
}

#[cfg(test)]
pub(super) fn create_test_root_with_invalid_dacl(root: &Path) -> io::Result<()> {
    create_private_root_with_sddl(root, "D:P(A;;FA;;;OW)").map(|_| ())
}

pub(super) fn open_private_root(root: &Path) -> io::Result<(PathBuf, Vec<File>)> {
    let paths = directory_chain(root)?;
    if paths.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "journal root must be below an existing parent directory",
        ));
    }
    validate_storage_volume(&paths[paths.len() - 2])?;
    let context = SecurityContext::current()?;
    let guards = open_and_validate_chain(&paths, &context, true)?;
    Ok((
        paths.last().expect("non-empty directory chain").clone(),
        guards,
    ))
}

#[cfg(test)]
pub(super) fn create_test_directory_with_sddl(root: &Path, sddl: &str) -> io::Result<()> {
    let descriptor = security_descriptor_from_sddl(sddl)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let path = wide_path(root);
    if unsafe { CreateDirectoryW(path.as_ptr(), &attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn directory_chain(root: &Path) -> io::Result<Vec<PathBuf>> {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };
    let mut components = absolute.components();
    let prefix = match components.next() {
        Some(Component::Prefix(prefix)) => prefix,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal root must be an absolute local-drive path",
            ));
        }
    };
    if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "journal root must be on a local drive",
        ));
    }
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "journal root must be an absolute local-drive path",
        ));
    }

    let mut current = PathBuf::from(prefix.as_os_str());
    current.push("\\");
    let mut paths = vec![current.clone()];
    for component in components {
        match component {
            Component::Normal(name) => {
                current.push(name);
                paths.push(current.clone());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "journal root contains a non-normal path component",
                ));
            }
        }
    }
    Ok(paths)
}

fn validate_storage_volume(anchor: &Path) -> io::Result<()> {
    const VOLUME_PATH_CAPACITY: usize = 32_768;
    const FILESYSTEM_NAME_CAPACITY: usize = 64;

    let anchor = wide_path(anchor);
    let mut volume_path = vec![0u16; VOLUME_PATH_CAPACITY];
    let volume_path_ok = unsafe {
        GetVolumePathNameW(
            anchor.as_ptr(),
            volume_path.as_mut_ptr(),
            volume_path.len() as u32,
        )
    };
    if volume_path_ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let drive_type = unsafe { GetDriveTypeW(volume_path.as_ptr()) };

    let mut filesystem_name = [0u16; FILESYSTEM_NAME_CAPACITY];
    let mut filesystem_flags = 0u32;
    let volume_info_ok = unsafe {
        GetVolumeInformationW(
            volume_path.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut filesystem_flags,
            filesystem_name.as_mut_ptr(),
            filesystem_name.len() as u32,
        )
    };
    if volume_info_ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let name_len = filesystem_name
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(filesystem_name.len());
    let filesystem_name = String::from_utf16_lossy(&filesystem_name[..name_len]);
    validate_volume_properties(drive_type, &filesystem_name, filesystem_flags)
}

fn validate_volume_properties(
    drive_type: u32,
    filesystem_name: &str,
    filesystem_flags: u32,
) -> io::Result<()> {
    if drive_type != DRIVE_FIXED {
        let drive_description = match drive_type {
            DRIVE_REMOVABLE => "removable",
            DRIVE_REMOTE => "remote",
            _ => "non-fixed",
        };
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("journal storage requires a fixed local volume; found {drive_description}"),
        ));
    }
    if !filesystem_name.eq_ignore_ascii_case("NTFS") || filesystem_flags & FILE_PERSISTENT_ACLS == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "journal storage requires NTFS with persistent ACLs; found {filesystem_name} (flags 0x{filesystem_flags:08x})"
            ),
        ));
    }
    Ok(())
}

fn open_and_validate_chain(
    paths: &[PathBuf],
    context: &SecurityContext,
    validate_private_leaf: bool,
) -> io::Result<Vec<File>> {
    let mut guards = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let handle = open_directory(path)?;
        let mut tag = FILE_ATTRIBUTE_TAG_INFO {
            FileAttributes: 0,
            ReparseTag: 0,
        };
        let info_ok = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        };
        if info_ok == 0 {
            unsafe { CloseHandle(handle) };
            return Err(io::Error::last_os_error());
        }
        if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            unsafe { CloseHandle(handle) };
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal path contains a non-directory or reparse point",
            ));
        }

        let file = unsafe { File::from_raw_handle(handle as RawHandle) };
        let descriptor = security_descriptor_for_handle(handle)?;
        let leaf = validate_private_leaf && index + 1 == paths.len();
        if leaf {
            validate_private_dacl(&descriptor, context)?;
        } else {
            validate_parent_dacl(&descriptor, context)?;
        }
        guards.push(file);
    }
    Ok(guards)
}

fn open_directory(path: &Path) -> io::Result<HANDLE> {
    let path = wide_path(path);
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}

fn security_descriptor_from_sddl(sddl: &str) -> io::Result<OwnedSecurityDescriptor> {
    let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    if descriptor.is_null() {
        return Err(invalid_security(
            "SDDL conversion returned a null descriptor",
        ));
    }
    Ok(OwnedSecurityDescriptor(descriptor))
}

fn security_descriptor_for_handle(handle: HANDLE) -> io::Result<OwnedSecurityDescriptor> {
    let mut descriptor = std::ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    if descriptor.is_null() {
        return Err(invalid_security(
            "security query returned a null descriptor",
        ));
    }
    Ok(OwnedSecurityDescriptor(descriptor))
}

fn security_parts(
    descriptor: &OwnedSecurityDescriptor,
) -> io::Result<(
    *mut core::ffi::c_void,
    *mut windows_sys::Win32::Security::ACL,
)> {
    let mut owner = std::ptr::null_mut();
    let mut owner_defaulted = 0;
    let owner_ok =
        unsafe { GetSecurityDescriptorOwner(descriptor.0, &mut owner, &mut owner_defaulted) };
    if owner_ok == 0 || owner.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut dacl_present = 0;
    let mut dacl = std::ptr::null_mut();
    let mut dacl_defaulted = 0;
    let dacl_ok = unsafe {
        GetSecurityDescriptorDacl(
            descriptor.0,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    };
    if dacl_ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if dacl_present == 0 || dacl.is_null() {
        return Err(invalid_security("journal directory has no DACL"));
    }
    if unsafe { IsValidAcl(dacl) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "journal directory contains an invalid DACL",
        ));
    }
    Ok((owner, dacl))
}

fn validate_private_dacl(
    descriptor: &OwnedSecurityDescriptor,
    context: &SecurityContext,
) -> io::Result<()> {
    let (owner, dacl) = security_parts(descriptor)?;
    if !context.is_sid(owner, context.owner.sid()) {
        return Err(invalid_security(
            "journal root owner does not match the current token owner",
        ));
    }
    let mut control = 0u16;
    let mut revision = 0u32;
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if control & SE_DACL_PROTECTED == 0 {
        return Err(invalid_security("journal root DACL is not protected"));
    }
    let ace_count = unsafe { (*dacl).AceCount };
    if ace_count != 3 {
        return Err(invalid_security(
            "journal root DACL must contain only SYSTEM and the two required OWNER RIGHTS entries",
        ));
    }
    let mut saw_system = false;
    let mut saw_owner_directory = false;
    let mut saw_owner_files = false;
    for index in 0..u32::from(ace_count) {
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
            return Err(io::Error::last_os_error());
        }
        let header = unsafe { &*(ace.cast::<ACE_HEADER>()) };
        if header.AceType != ACE_ACCESS_ALLOWED {
            return Err(invalid_security(
                "journal root DACL contains an unexpected ACE type or flags",
            ));
        }
        let allowed = unsafe { &*(ace.cast::<ACCESS_ALLOWED_ACE>()) };
        let sid = std::ptr::addr_of!(allowed.SidStart) as *mut core::ffi::c_void;
        if context.is_well_known(sid, WinLocalSystemSid) {
            if saw_system
                || header.AceFlags != OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
                || allowed.Mask != FILE_ALL_ACCESS
            {
                return Err(invalid_security("duplicate SYSTEM ACE on journal root"));
            }
            saw_system = true;
        } else if context.is_well_known(sid, WinCreatorOwnerRightsSid) {
            if header.AceFlags == 0 && allowed.Mask == ROOT_STORAGE_ACCESS && !saw_owner_directory {
                saw_owner_directory = true;
            } else if header.AceFlags == OBJECT_INHERIT_ACE | INHERIT_ONLY_ACE
                && allowed.Mask == FILE_STORAGE_ACCESS
                && !saw_owner_files
            {
                saw_owner_files = true;
            } else {
                return Err(invalid_security(
                    "unexpected OWNER RIGHTS ACE on journal root",
                ));
            }
        } else {
            return Err(invalid_security(
                "journal root DACL grants access to an unexpected principal",
            ));
        }
    }
    if !saw_system || !saw_owner_directory || !saw_owner_files {
        return Err(invalid_security(
            "journal root DACL is missing a required SYSTEM or OWNER RIGHTS entry",
        ));
    }
    Ok(())
}

fn validate_parent_dacl(
    descriptor: &OwnedSecurityDescriptor,
    context: &SecurityContext,
) -> io::Result<()> {
    let (owner, dacl) = security_parts(descriptor)?;
    if !context.is_trusted_owner(owner) {
        return Err(invalid_security(
            "journal ancestor is owned by an untrusted principal",
        ));
    }
    let ace_count = unsafe { (*dacl).AceCount };
    for index in 0..u32::from(ace_count) {
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
            return Err(io::Error::last_os_error());
        }
        let header = unsafe { &*(ace.cast::<ACE_HEADER>()) };
        if header.AceFlags & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        match header.AceType {
            ACE_ACCESS_DENIED => {
                let denied = unsafe { &*(ace.cast::<ACCESS_DENIED_ACE>()) };
                let sid = std::ptr::addr_of!(denied.SidStart) as *mut core::ffi::c_void;
                if unsafe { windows_sys::Win32::Security::IsValidSid(sid) } == 0 {
                    return Err(invalid_security("ancestor DACL contains an invalid SID"));
                }
            }
            ACE_ACCESS_ALLOWED => {
                let allowed = unsafe { &*(ace.cast::<ACCESS_ALLOWED_ACE>()) };
                let sid = std::ptr::addr_of!(allowed.SidStart) as *mut core::ffi::c_void;
                if unsafe { windows_sys::Win32::Security::IsValidSid(sid) } == 0 {
                    return Err(invalid_security("ancestor DACL contains an invalid SID"));
                }
                if allowed.Mask & DANGEROUS_PARENT_RIGHTS != 0 {
                    let trustee_is_trusted = context.is_trusted_principal(sid)
                        || (context.is_well_known(sid, WinCreatorOwnerRightsSid)
                            && context.is_trusted_owner(owner));
                    if !trustee_is_trusted {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            format!(
                                "journal ancestor grants delete/ACL-owner rights to an untrusted principal (mask 0x{:08x}, ACE flags 0x{:02x})",
                                allowed.Mask, header.AceFlags
                            ),
                        ));
                    }
                }
            }
            _ => {
                return Err(invalid_security(
                    "journal ancestor DACL contains an unsupported ACE type",
                ));
            }
        }
    }
    Ok(())
}

fn invalid_security(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod volume_policy_tests {
    use super::*;

    #[test]
    fn journal_volume_policy_requires_fixed_ntfs_with_persistent_acls() {
        assert!(validate_volume_properties(DRIVE_FIXED, "NTFS", FILE_PERSISTENT_ACLS).is_ok());
        assert_eq!(
            validate_volume_properties(DRIVE_REMOVABLE, "NTFS", FILE_PERSISTENT_ACLS)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            validate_volume_properties(DRIVE_REMOTE, "NTFS", FILE_PERSISTENT_ACLS)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            validate_volume_properties(DRIVE_FIXED, "exFAT", FILE_PERSISTENT_ACLS)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            validate_volume_properties(DRIVE_FIXED, "NTFS", 0)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
    }
}
