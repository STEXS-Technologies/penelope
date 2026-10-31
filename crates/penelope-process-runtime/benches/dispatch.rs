//! Linux worker latency, with real WAL/FULL task persistence and host ACK writes.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "support/dispatch.rs"]
mod measurement;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    measurement::run()
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn main() {
    eprintln!("dispatch benchmark requires Linux x86_64");
}
