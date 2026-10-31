//! Warm public duplicate input admission, including normal worker dispatch.
#[path = "support/inbox_fixture.rs"]
mod fixture;
fn main() -> Result<(), fixture::Error> {
    fixture::run(|consumed, call| {
        let iterations = 5000_u32;
        let started = std::time::Instant::now();
        for _ in 0..iterations {
            call()?;
        }
        let elapsed_ns = started.elapsed().as_nanos();
        println!(
            "{}",
            serde_json::json!({"consumed":consumed,"iterations":iterations,"elapsed_ns":elapsed_ns})
        );
        Ok(())
    })
}
