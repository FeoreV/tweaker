//! Core building blocks. Windows registry access is read-only.

pub mod data;
pub mod engine;

#[cfg(all(windows, feature = "experimental-ui"))]
pub mod ui;
