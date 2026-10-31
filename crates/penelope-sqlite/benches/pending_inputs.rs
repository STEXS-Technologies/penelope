//! Warm public pending-input discovery, including normal worker dispatch.
#[path = "support/pending_fixture.rs"]
mod fixture;
fn main() -> Result<(), fixture::Error> {
    fixture::run(|pending, call| {
        let iterations = 5000_u32;
        let started = std::time::Instant::now();
        for _ in 0..iterations {
            call()?;
        }
        let elapsed_ns = started.elapsed().as_nanos();
        println!(
            "{}",
            serde_json::json!({"pending":pending,"iterations":iterations,"elapsed_ns":elapsed_ns})
        );
        Ok(())
    })
}
