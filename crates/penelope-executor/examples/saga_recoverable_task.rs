//! SQLite-backed composition with a simulated in-process runtime.
//! For protected execution use penelope-process-runtime's example instead.
#[path = "support/saga.rs"]
mod implementation;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    implementation::run()
}
