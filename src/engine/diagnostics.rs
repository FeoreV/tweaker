//! Read-only OS and memory diagnostics using Win32, without WMI.
use serde::Serialize;
use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::SystemInformation::{
    GetPhysicallyInstalledSystemMemory, GetWindowsDirectoryW, GlobalMemoryStatusEx, MEMORYSTATUSEX,
    OSVERSIONINFOW,
};

#[derive(Debug, Serialize)]
pub struct Diagnostics {
    pub os_major: u32,
    pub os_minor: u32,
    pub os_build: u32,
    pub cpu_model: Option<String>,
    pub physical_memory_total_bytes: u64,
    pub physical_memory_available_bytes: u64,
    pub physically_installed_memory_bytes: Option<u64>,
    pub physically_installed_memory_error: Option<String>,
    pub system_volume: Option<VolumeSpace>,
    pub system_volume_error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct VolumeSpace {
    /// Total capacity available to this user, which may be limited by quota.
    pub total_bytes_for_caller: u64,
    pub available_bytes_for_caller: u64,
    pub total_free_bytes: u64,
}
#[derive(Debug)]
pub enum DiagnosticsError {
    Version(i32),
    Memory(std::io::Error),
}

pub fn collect() -> Result<Diagnostics, DiagnosticsError> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        dwMajorVersion: 0,
        dwMinorVersion: 0,
        dwBuildNumber: 0,
        dwPlatformId: 0,
        szCSDVersion: [0; 128],
    };
    // SAFETY: version has the required initialized size and points to writable storage.
    let status = unsafe { RtlGetVersion(&mut version) };
    if status < 0 {
        return Err(DiagnosticsError::Version(status));
    }
    let mut memory = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        dwMemoryLoad: 0,
        ullTotalPhys: 0,
        ullAvailPhys: 0,
        ullTotalPageFile: 0,
        ullAvailPageFile: 0,
        ullTotalVirtual: 0,
        ullAvailVirtual: 0,
        ullAvailExtendedVirtual: 0,
    };
    // SAFETY: memory has the required initialized size and writable fields.
    if unsafe { GlobalMemoryStatusEx(&mut memory) } == 0 {
        return Err(DiagnosticsError::Memory(std::io::Error::last_os_error()));
    }
    let (system_volume, system_volume_error) = match system_volume_space() {
        Ok(space) => (Some(space), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let (physically_installed_memory_bytes, physically_installed_memory_error) =
        installed_memory_result(|| {
            let mut kilobytes = 0u64;
            // SAFETY: `kilobytes` is a valid writable output pointer.
            if unsafe { GetPhysicallyInstalledSystemMemory(&mut kilobytes) } == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(kilobytes)
            }
        });
    Ok(Diagnostics {
        os_major: version.dwMajorVersion,
        os_minor: version.dwMinorVersion,
        os_build: version.dwBuildNumber,
        cpu_model: cpu_model(),
        physical_memory_total_bytes: memory.ullTotalPhys,
        physical_memory_available_bytes: memory.ullAvailPhys,
        physically_installed_memory_bytes,
        physically_installed_memory_error,
        system_volume,
        system_volume_error,
    })
}

fn installed_memory_result(
    query: impl FnOnce() -> Result<u64, std::io::Error>,
) -> (Option<u64>, Option<String>) {
    match query().and_then(|kilobytes| {
        if kilobytes == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "installed physical memory count is zero",
            ));
        }
        kilobytes.checked_mul(1024).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "installed physical memory byte count overflow",
            )
        })
    }) {
        Ok(bytes) => (Some(bytes), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

fn system_volume_space() -> Result<VolumeSpace, std::io::Error> {
    // The Windows path limit is 32,767 UTF-16 units including the terminator.
    let mut directory = vec![0u16; 32_768];
    // SAFETY: buffer is writable for the specified number of UTF-16 units.
    let length = unsafe { GetWindowsDirectoryW(directory.as_mut_ptr(), directory.len() as u32) };
    if length == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if length as usize >= directory.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows directory path too long",
        ));
    }
    let mut available = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    // SAFETY: the successful Windows-directory call supplies a terminated path,
    // and all output pointers refer to writable u64 values.
    if unsafe { GetDiskFreeSpaceExW(directory.as_ptr(), &mut available, &mut total, &mut free) }
        == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(VolumeSpace {
        total_bytes_for_caller: total,
        available_bytes_for_caller: available,
        total_free_bytes: free,
    })
}

fn parse_cpu_brand(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let value = std::str::from_utf8(&bytes[..end]).ok()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn cpu_model() -> Option<String> {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::__cpuid;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::__cpuid;
    // Check the extended leaf range before requesting the three brand leaves.
    if __cpuid(0x8000_0000).eax < 0x8000_0004 {
        return None;
    }
    let mut bytes = Vec::with_capacity(48);
    for leaf in 0x8000_0002..=0x8000_0004 {
        let result = __cpuid(leaf);
        for value in [result.eax, result.ebx, result.ecx, result.edx] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    parse_cpu_brand(&bytes)
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn cpu_model() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn cpu_brand_rejects_empty_and_invalid_text() {
        assert_eq!(
            super::parse_cpu_brand(b"  CPU model  \0junk"),
            Some("CPU model".into())
        );
        assert_eq!(super::parse_cpu_brand(b"   \0"), None);
        assert_eq!(super::parse_cpu_brand(&[0xff]), None);
    }
    #[test]
    fn reports_consistent_live_memory() {
        let data = super::collect().expect("read diagnostics");
        assert!(data.os_major > 0);
        assert!(data.physical_memory_total_bytes > 0);
        assert!(data.physical_memory_available_bytes <= data.physical_memory_total_bytes);
        if let Some(space) = data.system_volume {
            assert!(space.available_bytes_for_caller <= space.total_bytes_for_caller);
            assert!(data.system_volume_error.is_none());
        } else {
            assert!(data.system_volume_error.is_some());
        }
        assert_eq!(
            data.physically_installed_memory_bytes.is_some(),
            data.physically_installed_memory_error.is_none()
        );
        if let Some(installed) = data.physically_installed_memory_bytes {
            assert!(installed > 0);
        }
    }

    #[test]
    fn installed_memory_result_converts_kilobytes_to_bytes() {
        assert_eq!(
            super::installed_memory_result(|| Ok(16 * 1024 * 1024)),
            (Some(16 * 1024 * 1024 * 1024), None)
        );
    }

    #[test]
    fn installed_memory_result_keeps_api_errors_local() {
        let (bytes, error) =
            super::installed_memory_result(|| Err(std::io::Error::from_raw_os_error(13)));
        assert_eq!(bytes, None);
        assert!(error.is_some());
    }

    #[test]
    fn installed_memory_result_reports_overflow() {
        let (bytes, error) = super::installed_memory_result(|| Ok(u64::MAX));
        assert_eq!(bytes, None);
        assert!(error.unwrap().contains("overflow"));
    }

    #[test]
    fn installed_memory_result_rejects_zero() {
        let (bytes, error) = super::installed_memory_result(|| Ok(0));
        assert_eq!(bytes, None);
        assert!(error.unwrap().contains("zero"));
    }
}
