//! Explicit operator image evaluation, never a registered tool or model loader
//! on import. Core/boundaries are Rust; actual inference remains native sd.cpp.
pub mod artifact;
pub mod assets;
#[cfg(unix)]
pub mod coordination;
#[cfg(target_os = "macos")]
pub mod download;
#[cfg(target_os = "macos")]
pub mod job;
#[cfg(target_os = "macos")]
pub mod native;
#[cfg(target_os = "macos")]
pub mod runner;
