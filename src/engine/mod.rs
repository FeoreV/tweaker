//! Transaction coordination for backends with independently persisted snapshots.

#[cfg(windows)]
pub mod diagnostics;
pub mod recovery;
pub mod registry;
#[cfg(windows)]
pub mod services;
#[cfg(windows)]
pub mod storage;
pub mod transaction;
