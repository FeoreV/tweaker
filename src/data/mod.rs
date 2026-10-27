//! Versioned data formats and storage adapters.

pub mod catalog;
pub mod checkpoint;
#[cfg(windows)]
pub mod file_store;
pub mod journal;
pub mod store;
#[cfg(windows)]
mod windows_security;
