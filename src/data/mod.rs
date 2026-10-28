//! Versioned data formats and storage adapters.

pub mod applications;
pub mod catalog;
pub mod checkpoint;
#[cfg(windows)]
pub mod file_store;
pub mod journal;
pub mod selection_catalog;
pub mod selection_export;
pub mod selection_import;
pub mod selection_state;
pub mod store;
#[cfg(windows)]
mod windows_security;
