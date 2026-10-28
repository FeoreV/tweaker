//! Transaction coordination for backends with independently persisted snapshots.

pub mod application_filter;
#[cfg(windows)]
pub mod diagnostics;
pub mod installed_apps;
#[cfg(windows)]
pub mod installed_apps_windows;
pub mod location_status;
pub mod numlock_status;
pub mod package_import;
pub mod package_log_summary;
pub mod package_plan;
pub mod package_state;
pub mod recovery;
pub mod registry;
#[cfg(windows)]
pub mod services;
#[cfg(windows)]
pub mod storage;
pub mod toggle_status;
pub mod transaction;
pub mod tweak_plan;
pub mod tweak_state;
pub mod utc_status;
// Source operation is intentionally not wired to an executor until HKCU identity
// binding and journal-root path-identity security are supported.
#[expect(
    dead_code,
    reason = "Window Snapping is not authorized for live execution yet"
)]
pub(crate) mod window_snapping_plan;
pub mod window_snapping_status;
#[cfg(windows)]
pub mod winsock_catalog;
