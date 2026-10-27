//! Read-only physical storage device descriptor query (not SMART health).
use serde::Serialize;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, IOCTL_STORAGE_QUERY_PROPERTY,
    STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: handle came from successful CreateFileW and is uniquely owned.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
#[derive(Debug, Serialize)]
pub struct DiskInfo {
    pub index: u8,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub bus_type: i32,
    pub removable: bool,
}
#[derive(Debug)]
pub enum DiskError {
    Open(u32),
    Query(u32),
    InvalidHandle,
    InvalidDescriptor,
}

fn descriptor_string(bytes: &[u8], offset: u32) -> Result<Option<String>, DiskError> {
    if offset == 0 {
        return Ok(None);
    }
    let tail = bytes
        .get(offset as usize..)
        .ok_or(DiskError::InvalidDescriptor)?;
    let end = tail
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(DiskError::InvalidDescriptor)?;
    let text = std::str::from_utf8(&tail[..end])
        .map_err(|_| DiskError::InvalidDescriptor)?
        .trim();
    Ok(if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    })
}
fn parse_descriptor(index: u8, bytes: &[u8]) -> Result<DiskInfo, DiskError> {
    if bytes.len() < std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() {
        return Err(DiskError::InvalidDescriptor);
    }
    // SAFETY: the input contains at least the fixed descriptor size; read_unaligned
    // avoids making assumptions about the alignment of a byte slice.
    let descriptor =
        unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<STORAGE_DEVICE_DESCRIPTOR>()) };
    let size = descriptor.Size as usize;
    if size < std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() || size > bytes.len() {
        return Err(DiskError::InvalidDescriptor);
    }
    let data = &bytes[..size];
    Ok(DiskInfo {
        index,
        vendor: descriptor_string(data, descriptor.VendorIdOffset)?,
        model: descriptor_string(data, descriptor.ProductIdOffset)?,
        bus_type: descriptor.BusType,
        removable: descriptor.RemovableMedia != 0,
    })
}
/// Physical drive number is explicit; it is not assumed to be the system drive.
pub fn query_physical_drive(index: u8) -> Result<DiskInfo, DiskError> {
    let path: Vec<u16> = format!(r"\\.\PhysicalDrive{index}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: path is terminated; zero desired access and shared read/write do not modify the device.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(DiskError::Open(unsafe { GetLastError() }));
    }
    if raw.is_null() {
        return Err(DiskError::InvalidHandle);
    }
    let handle = OwnedHandle(raw);
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    let mut buffer = vec![0u8; 64 * 1024];
    let mut returned = 0u32;
    // SAFETY: input and output buffers are valid for the specified sizes and
    // the call is synchronous; this IOCTL requests only device properties.
    if unsafe {
        DeviceIoControl(
            handle.0,
            IOCTL_STORAGE_QUERY_PROPERTY,
            (&query as *const STORAGE_PROPERTY_QUERY).cast(),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut returned,
            null_mut(),
        )
    } == 0
    {
        return Err(DiskError::Query(unsafe { GetLastError() }));
    }
    if returned as usize > buffer.len() {
        return Err(DiskError::InvalidDescriptor);
    }
    parse_descriptor(index, &buffer[..returned as usize])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn string_bounds_and_termination_are_checked() {
        assert!(matches!(
            descriptor_string(b"abc", 4),
            Err(DiskError::InvalidDescriptor)
        ));
        assert!(matches!(
            descriptor_string(b"abc", 1),
            Err(DiskError::InvalidDescriptor)
        ));
        assert_eq!(
            descriptor_string(b"x  model \0", 1).unwrap(),
            Some("model".into())
        );
        assert!(matches!(
            parse_descriptor(0, &[0; 8]),
            Err(DiskError::InvalidDescriptor)
        ));
    }
}
