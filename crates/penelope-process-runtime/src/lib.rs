//! Optional protected Linux x86-64 execution for static single-process workers.
//! No default facade dependency or application scheduler is installed.
#![deny(unsafe_code)]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod image;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod sandbox;
pub mod worker;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod registry;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod runtime;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use runtime::{LinuxProcessRuntime, ProcessRuntimeConfig};
