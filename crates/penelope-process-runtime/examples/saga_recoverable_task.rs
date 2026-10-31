//! Linux reference composition: a one-step saga, separate SQLite stores, real
//! protected worker, verified result and durable event replay. Run as one local
//! authenticated OS user; this demonstration policy is not a network service.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "support/saga.rs"]
mod implementation;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        implementation::run()
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        Err(std::io::Error::other("this example requires Linux x86-64").into())
    }
}
