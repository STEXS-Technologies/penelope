//! Public Linux fencing latency with real WAL/FULL generation authority.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "support/fence.rs"]
mod measurement;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    measurement::run()
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn main() {
    eprintln!("fence benchmark requires Linux x86_64");
}
