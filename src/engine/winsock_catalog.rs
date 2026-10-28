//! Read-only Winsock transport catalog inspection; this is not a rollback snapshot.
//!
//! The catalog can change during collection. Provider-specific configuration and DLL
//! contents are not captured, so these results must never authorize a reset.

use serde::{Deserialize, Serialize};
use std::{mem, ptr};
#[cfg(target_pointer_width = "64")]
use windows_sys::Win32::Networking::WinSock::{WSCEnumProtocols32, WSCGetProviderPath32};
use windows_sys::{
    core::GUID,
    Win32::Networking::WinSock::{
        WSACleanup, WSAStartup, WSCEnumProtocols, WSCGetProviderPath, WSADATA, WSAEFAULT,
        WSAENOBUFS, WSAPROTOCOL_INFOW,
    },
};

const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_PATH_UNITS: usize = 32768;
const RETRIES: usize = 4;

fn protocol_buffer_len(capacity_bytes: usize) -> Result<usize, QueryError> {
    let entry_size = mem::size_of::<WSAPROTOCOL_INFOW>();
    if capacity_bytes == 0 || entry_size == 0 {
        return Err(QueryError::InvalidResponse);
    }
    // Round up so the byte capacity passed to Winsock is at least the capacity
    // requested by the retry loop. The vector and the API length must describe
    // the same allocation; rounding down made a valid required-size response
    // look non-progressing when the requested size was not entry-aligned.
    let count = capacity_bytes
        .checked_add(entry_size - 1)
        .ok_or(QueryError::InvalidResponse)?
        / entry_size;
    count
        .checked_mul(entry_size)
        .ok_or(QueryError::InvalidResponse)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogKind {
    Bit32,
    Bit64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureError {
    Api(i32),
    Limit,
    InvalidResponse,
    ChangedDuringCapture,
    Startup(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogStatus {
    Captured(Vec<ProtocolEntry>),
    Failed(CaptureError),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalogs {
    pub bit32: CatalogStatus,
    pub bit64: CatalogStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderId {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolEntry {
    pub service_flags: [u32; 4],
    pub provider_flags: u32,
    pub provider_id: ProviderId,
    pub catalog_entry_id: u32,
    pub chain_len: i32,
    pub chain_entries: [u32; 7],
    pub version: i32,
    pub address_family: i32,
    pub max_sockaddr: i32,
    pub min_sockaddr: i32,
    pub socket_type: i32,
    pub protocol: i32,
    pub protocol_max_offset: i32,
    pub network_byte_order: i32,
    pub security_scheme: i32,
    pub message_size: u32,
    pub provider_reserved: u32,
    /// All 256 UTF-16 units, including terminator and any trailing units.
    pub protocol_name: Vec<u16>,
    /// Unexpanded UTF-16 path, or the exact API/validation failure for this provider.
    pub provider_path: Result<Vec<u16>, CaptureError>,
}

/// A bounded read interface. Implementations must return an error rather than truncate.
pub trait CatalogApi {
    fn enumerate(
        &mut self,
        kind: CatalogKind,
        capacity_bytes: usize,
    ) -> Result<Vec<ProtocolEntry>, QueryError>;
    fn provider_path(
        &mut self,
        kind: CatalogKind,
        provider: ProviderId,
        capacity_units: usize,
    ) -> Result<Vec<u16>, QueryError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    BufferTooSmall(usize),
    Limit,
    Api(i32),
    InvalidResponse,
}

fn capture_one(api: &mut impl CatalogApi, kind: CatalogKind) -> CatalogStatus {
    let mut capacity = 16 * 1024;
    let mut entries = None;
    for _ in 0..RETRIES {
        match api.enumerate(kind, capacity) {
            Ok(value) => {
                entries = Some(value);
                break;
            }
            Err(QueryError::BufferTooSmall(needed))
                if needed > capacity && needed <= MAX_CATALOG_BYTES =>
            {
                capacity = needed
            }
            Err(QueryError::BufferTooSmall(needed)) if needed > MAX_CATALOG_BYTES => {
                return CatalogStatus::Failed(CaptureError::Limit)
            }
            Err(QueryError::Limit) => return CatalogStatus::Failed(CaptureError::Limit),
            Err(QueryError::BufferTooSmall(_)) => {
                return CatalogStatus::Failed(CaptureError::InvalidResponse)
            }
            Err(QueryError::Api(code)) => return CatalogStatus::Failed(CaptureError::Api(code)),
            Err(QueryError::InvalidResponse) => {
                return CatalogStatus::Failed(CaptureError::InvalidResponse)
            }
        }
    }
    let Some(mut entries) = entries else {
        return CatalogStatus::Failed(CaptureError::ChangedDuringCapture);
    };
    let mut total_path_bytes = 0usize;
    for entry in &mut entries {
        let mut capacity = 260;
        let mut result = Err(CaptureError::ChangedDuringCapture);
        for _ in 0..RETRIES {
            match api.provider_path(kind, entry.provider_id, capacity) {
                Ok(path) => {
                    let Some(path_bytes) = path.len().checked_mul(mem::size_of::<u16>()) else {
                        return CatalogStatus::Failed(CaptureError::Limit);
                    };
                    let Some(total) = total_path_bytes.checked_add(path_bytes) else {
                        return CatalogStatus::Failed(CaptureError::Limit);
                    };
                    if total > MAX_CATALOG_BYTES {
                        return CatalogStatus::Failed(CaptureError::Limit);
                    }
                    total_path_bytes = total;
                    result = Ok(path);
                    break;
                }
                Err(QueryError::BufferTooSmall(needed))
                    if needed > capacity && needed <= MAX_PATH_UNITS =>
                {
                    capacity = needed
                }
                Err(QueryError::BufferTooSmall(needed)) if needed > MAX_PATH_UNITS => {
                    result = Err(CaptureError::Limit);
                    break;
                }
                Err(QueryError::BufferTooSmall(_)) | Err(QueryError::InvalidResponse) => {
                    result = Err(CaptureError::InvalidResponse);
                    break;
                }
                Err(QueryError::Api(code)) => {
                    result = Err(CaptureError::Api(code));
                    break;
                }
                Err(QueryError::Limit) => {
                    result = Err(CaptureError::Limit);
                    break;
                }
            }
        }
        entry.provider_path = result;
    }
    CatalogStatus::Captured(entries)
}

/// Inspect both catalogs where supported by the process architecture. On a 32-bit
/// process the native 32-bit catalog is available, but the 64-bit catalog is not.
pub fn capture_with(api: &mut impl CatalogApi, can_read_64: bool) -> Catalogs {
    Catalogs {
        bit32: capture_one(api, CatalogKind::Bit32),
        bit64: if can_read_64 {
            capture_one(api, CatalogKind::Bit64)
        } else {
            CatalogStatus::Unavailable
        },
    }
}

struct WinsockGuard;
impl WinsockGuard {
    fn start() -> Result<Self, CaptureError> {
        // SAFETY: WSADATA is plain C data and points to valid writable storage.
        let mut data: WSADATA = unsafe { mem::zeroed() };
        // SAFETY: valid pointer, version 2.2; each successful startup is balanced by Drop.
        let code = unsafe { WSAStartup(0x0202, &mut data) };
        if code != 0 {
            return Err(CaptureError::Startup(code));
        }
        if data.wVersion != 0x0202 {
            unsafe {
                WSACleanup();
            }
            return Err(CaptureError::InvalidResponse);
        }
        Ok(Self)
    }
}
impl Drop for WinsockGuard {
    fn drop(&mut self) {
        // SAFETY: balances our successful WSAStartup exactly once.
        unsafe {
            WSACleanup();
        }
    }
}

struct WindowsApi;
impl CatalogApi for WindowsApi {
    fn enumerate(
        &mut self,
        kind: CatalogKind,
        capacity_bytes: usize,
    ) -> Result<Vec<ProtocolEntry>, QueryError> {
        let allocated_bytes = protocol_buffer_len(capacity_bytes)?;
        if allocated_bytes > MAX_CATALOG_BYTES {
            return Err(QueryError::Limit);
        }
        let count = allocated_bytes / mem::size_of::<WSAPROTOCOL_INFOW>();
        // SAFETY: the C structure is entirely integer fields and zero is valid.
        let mut buffer = vec![unsafe { mem::zeroed::<WSAPROTOCOL_INFOW>() }; count];
        let mut bytes = u32::try_from(allocated_bytes).map_err(|_| QueryError::InvalidResponse)?;
        let mut errno = 0;
        // SAFETY: buffer is aligned, writable for `bytes`, and remaining pointers are valid.
        let result = unsafe {
            #[cfg(target_pointer_width = "64")]
            {
                match kind {
                    CatalogKind::Bit32 => {
                        WSCEnumProtocols32(ptr::null(), buffer.as_mut_ptr(), &mut bytes, &mut errno)
                    }
                    CatalogKind::Bit64 => {
                        WSCEnumProtocols(ptr::null(), buffer.as_mut_ptr(), &mut bytes, &mut errno)
                    }
                }
            }
            #[cfg(target_pointer_width = "32")]
            {
                if kind == CatalogKind::Bit64 {
                    return Err(QueryError::InvalidResponse);
                }
                WSCEnumProtocols(ptr::null(), buffer.as_mut_ptr(), &mut bytes, &mut errno)
            }
        };
        if result < 0 {
            return if errno == WSAENOBUFS {
                Err(QueryError::BufferTooSmall(bytes as usize))
            } else {
                Err(QueryError::Api(errno))
            };
        }
        if result as usize > count {
            return Err(QueryError::InvalidResponse);
        }
        Ok(buffer[..result as usize]
            .iter()
            .map(|p| ProtocolEntry {
                service_flags: [
                    p.dwServiceFlags1,
                    p.dwServiceFlags2,
                    p.dwServiceFlags3,
                    p.dwServiceFlags4,
                ],
                provider_flags: p.dwProviderFlags,
                provider_id: ProviderId {
                    data1: p.ProviderId.data1,
                    data2: p.ProviderId.data2,
                    data3: p.ProviderId.data3,
                    data4: p.ProviderId.data4,
                },
                catalog_entry_id: p.dwCatalogEntryId,
                chain_len: p.ProtocolChain.ChainLen,
                chain_entries: p.ProtocolChain.ChainEntries,
                version: p.iVersion,
                address_family: p.iAddressFamily,
                max_sockaddr: p.iMaxSockAddr,
                min_sockaddr: p.iMinSockAddr,
                socket_type: p.iSocketType,
                protocol: p.iProtocol,
                protocol_max_offset: p.iProtocolMaxOffset,
                network_byte_order: p.iNetworkByteOrder,
                security_scheme: p.iSecurityScheme,
                message_size: p.dwMessageSize,
                provider_reserved: p.dwProviderReserved,
                protocol_name: p.szProtocol.to_vec(),
                provider_path: Err(CaptureError::InvalidResponse),
            })
            .collect())
    }
    fn provider_path(
        &mut self,
        kind: CatalogKind,
        provider: ProviderId,
        capacity_units: usize,
    ) -> Result<Vec<u16>, QueryError> {
        let guid = GUID {
            data1: provider.data1,
            data2: provider.data2,
            data3: provider.data3,
            data4: provider.data4,
        };
        let mut buffer = vec![0u16; capacity_units];
        let mut len = capacity_units as i32;
        let mut errno = 0;
        // SAFETY: GUID and buffer are valid for the call, and len is measured in UTF-16 units.
        let result = unsafe {
            #[cfg(target_pointer_width = "64")]
            {
                match kind {
                    CatalogKind::Bit32 => {
                        WSCGetProviderPath32(&guid, buffer.as_mut_ptr(), &mut len, &mut errno)
                    }
                    CatalogKind::Bit64 => {
                        WSCGetProviderPath(&guid, buffer.as_mut_ptr(), &mut len, &mut errno)
                    }
                }
            }
            #[cfg(target_pointer_width = "32")]
            {
                if kind == CatalogKind::Bit64 {
                    return Err(QueryError::InvalidResponse);
                }
                WSCGetProviderPath(&guid, buffer.as_mut_ptr(), &mut len, &mut errno)
            }
        };
        if result != 0 {
            return if errno == WSAEFAULT && len > capacity_units as i32 {
                Err(QueryError::BufferTooSmall(len as usize))
            } else {
                Err(QueryError::Api(errno))
            };
        }
        let end = buffer
            .iter()
            .position(|v| *v == 0)
            .ok_or(QueryError::InvalidResponse)?;
        buffer.truncate(end);
        Ok(buffer)
    }
}

/// Query the local process-visible transport catalogs. Does not mutate the system.
pub fn capture() -> Result<Catalogs, CaptureError> {
    let _guard = WinsockGuard::start()?;
    Ok(capture_with(
        &mut WindowsApi,
        cfg!(target_pointer_width = "64"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    fn entry(id: u32) -> ProtocolEntry {
        ProtocolEntry {
            service_flags: [1, 2, 3, 4],
            provider_flags: 1,
            provider_id: ProviderId {
                data1: id,
                data2: 0,
                data3: 0,
                data4: [0; 8],
            },
            catalog_entry_id: id,
            chain_len: 2,
            chain_entries: [9, 8, 0, 0, 0, 0, 0],
            version: 2,
            address_family: 2,
            max_sockaddr: 16,
            min_sockaddr: 16,
            socket_type: 1,
            protocol: 6,
            protocol_max_offset: 0,
            network_byte_order: 0,
            security_scheme: 0,
            message_size: 0,
            provider_reserved: 42,
            protocol_name: vec![65; 256],
            provider_path: Err(CaptureError::InvalidResponse),
        }
    }
    struct Mock {
        enumerations: VecDeque<Result<Vec<ProtocolEntry>, QueryError>>,
        paths: VecDeque<Result<Vec<u16>, QueryError>>,
        calls: Vec<(CatalogKind, usize)>,
        path_calls: Vec<usize>,
    }
    impl CatalogApi for Mock {
        fn enumerate(
            &mut self,
            kind: CatalogKind,
            size: usize,
        ) -> Result<Vec<ProtocolEntry>, QueryError> {
            self.calls.push((kind, size));
            self.enumerations.pop_front().unwrap()
        }
        fn provider_path(
            &mut self,
            _: CatalogKind,
            _: ProviderId,
            capacity: usize,
        ) -> Result<Vec<u16>, QueryError> {
            self.path_calls.push(capacity);
            self.paths.pop_front().unwrap()
        }
    }
    #[test]
    fn preserves_order_fields_path_and_architecture() {
        let mut m = Mock {
            enumerations: VecDeque::from([
                Err(QueryError::BufferTooSmall(20000)),
                Ok(vec![entry(3), entry(1)]),
                Ok(vec![entry(7)]),
            ]),
            paths: VecDeque::from([Ok(vec![37, 65]), Err(QueryError::Api(10022)), Ok(vec![66])]),
            calls: vec![],
            path_calls: vec![],
        };
        let c = capture_with(&mut m, true);
        let CatalogStatus::Captured(entries) = &c.bit32 else {
            panic!()
        };
        assert_eq!(
            entries
                .iter()
                .map(|e| e.catalog_entry_id)
                .collect::<Vec<_>>(),
            vec![3, 1]
        );
        assert_eq!(entries[0].chain_entries[..2], [9, 8]);
        assert_eq!(entries[0].protocol_name.len(), 256);
        assert_eq!(entries[0].provider_path, Ok(vec![37, 65]));
        assert_eq!(entries[1].provider_path, Err(CaptureError::Api(10022)));
        assert_eq!(
            m.calls,
            vec![
                (CatalogKind::Bit32, 16384),
                (CatalogKind::Bit32, 20000),
                (CatalogKind::Bit64, 16384)
            ]
        );
        let json = serde_json::to_vec(&c).unwrap();
        assert_eq!(serde_json::from_slice::<Catalogs>(&json).unwrap(), c);
    }
    #[test]
    fn inaccessible_64_bit_is_explicit_and_not_queried() {
        let mut m = Mock {
            enumerations: VecDeque::from([Ok(vec![])]),
            paths: VecDeque::new(),
            calls: vec![],
            path_calls: vec![],
        };
        assert_eq!(
            capture_with(&mut m, false).bit64,
            CatalogStatus::Unavailable
        );
        assert_eq!(m.calls.len(), 1);
    }
    #[test]
    fn oversized_or_nonprogressing_buffers_fail_closed() {
        for (needed, expected) in [
            (MAX_CATALOG_BYTES + 1, CaptureError::Limit),
            (1, CaptureError::InvalidResponse),
        ] {
            let mut m = Mock {
                enumerations: VecDeque::from([Err(QueryError::BufferTooSmall(needed))]),
                paths: VecDeque::new(),
                calls: vec![],
                path_calls: vec![],
            };
            assert_eq!(
                capture_with(&mut m, false).bit32,
                CatalogStatus::Failed(expected)
            );
        }
    }
    #[test]
    fn path_growth_is_bounded_and_errors_stay_per_entry() {
        let mut m = Mock {
            enumerations: VecDeque::from([Ok(vec![entry(1), entry(2)])]),
            paths: VecDeque::from([
                Err(QueryError::BufferTooSmall(400)),
                Ok(vec![65]),
                Err(QueryError::BufferTooSmall(MAX_PATH_UNITS + 1)),
            ]),
            calls: vec![],
            path_calls: vec![],
        };
        let CatalogStatus::Captured(entries) = capture_with(&mut m, false).bit32 else {
            panic!()
        };
        assert_eq!(entries[0].provider_path, Ok(vec![65]));
        assert_eq!(entries[1].provider_path, Err(CaptureError::Limit));
        assert_eq!(m.path_calls, vec![260, 400, 260]);
    }

    #[test]
    fn enumeration_grows_until_success_and_records_each_requested_capacity() {
        let mut m = Mock {
            enumerations: VecDeque::from([
                Err(QueryError::BufferTooSmall(19_001)),
                Err(QueryError::BufferTooSmall(27_003)),
                Ok(vec![]),
            ]),
            paths: VecDeque::new(),
            calls: vec![],
            path_calls: vec![],
        };
        assert_eq!(
            capture_with(&mut m, false).bit32,
            CatalogStatus::Captured(vec![])
        );
        assert_eq!(
            m.calls,
            vec![
                (CatalogKind::Bit32, 16_384),
                (CatalogKind::Bit32, 19_001),
                (CatalogKind::Bit32, 27_003),
            ]
        );
    }

    #[test]
    fn enumeration_rejects_nonprogress_and_exhausted_growth() {
        for needed in [16_384, 16_383] {
            let mut m = Mock {
                enumerations: VecDeque::from([Err(QueryError::BufferTooSmall(needed))]),
                paths: VecDeque::new(),
                calls: vec![],
                path_calls: vec![],
            };
            assert_eq!(
                capture_with(&mut m, false).bit32,
                CatalogStatus::Failed(CaptureError::InvalidResponse)
            );
        }
        let mut m = Mock {
            enumerations: VecDeque::from([
                Err(QueryError::BufferTooSmall(20_001)),
                Err(QueryError::BufferTooSmall(21_001)),
                Err(QueryError::BufferTooSmall(22_001)),
                Err(QueryError::BufferTooSmall(23_001)),
            ]),
            paths: VecDeque::new(),
            calls: vec![],
            path_calls: vec![],
        };
        assert_eq!(
            capture_with(&mut m, false).bit32,
            CatalogStatus::Failed(CaptureError::ChangedDuringCapture)
        );
        assert_eq!(m.calls.len(), RETRIES);
    }

    #[test]
    fn protocol_buffer_length_rounds_up_to_the_real_allocation() {
        let entry_size = mem::size_of::<WSAPROTOCOL_INFOW>();
        for requested in [1, entry_size, entry_size + 1, 16_384, 19_001] {
            let allocated = protocol_buffer_len(requested).unwrap();
            assert!(allocated >= requested);
            assert_eq!(allocated % entry_size, 0);
        }
        assert_eq!(protocol_buffer_len(0), Err(QueryError::InvalidResponse));
        assert_eq!(
            protocol_buffer_len(MAX_CATALOG_BYTES)
                .and_then(|allocated| (allocated > MAX_CATALOG_BYTES)
                    .then_some(QueryError::Limit)
                    .map_or(Ok(allocated), Err)),
            Err(QueryError::Limit)
        );
    }

    #[test]
    fn aggregate_provider_path_bytes_are_bounded_for_the_whole_catalog() {
        let entries = (0..65).map(entry).collect::<Vec<_>>();
        let paths = (0..65)
            .map(|_| Ok(vec![65; MAX_PATH_UNITS - 1]))
            .collect::<VecDeque<_>>();
        assert!(paths
            .iter()
            .all(|path| matches!(path, Ok(value) if value.len() < MAX_PATH_UNITS)));
        let total = paths
            .iter()
            .map(|path| path.as_ref().unwrap().len() * mem::size_of::<u16>())
            .sum::<usize>();
        assert!(total > MAX_CATALOG_BYTES);
        let mut m = Mock {
            enumerations: VecDeque::from([Ok(entries)]),
            paths,
            calls: vec![],
            path_calls: vec![],
        };
        assert_eq!(
            capture_with(&mut m, false).bit32,
            CatalogStatus::Failed(CaptureError::Limit)
        );
    }
}
