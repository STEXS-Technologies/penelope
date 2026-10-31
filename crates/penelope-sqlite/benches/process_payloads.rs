//! Public payload latency using the ordinary allocator.
#[path = "support/payload_fixture.rs"]
mod fixture;
use fixture::{Error, run};
use std::time::Instant;
fn main() -> Result<(), Error> {
    run(|fixture, case| {
        let (warmup, iterations) = if fixture.size() >= 1048576 {
            (32, 128_u32)
        } else {
            (100, 1000_u32)
        };
        for _ in 0..warmup {
            fixture.validate(case, fixture.call(case)?)?;
        }
        let mut elapsed_ns = 0_u128;
        for _ in 0..iterations {
            let started = Instant::now();
            let result = fixture.call(case)?;
            elapsed_ns = elapsed_ns
                .checked_add(started.elapsed().as_nanos())
                .ok_or("duration overflow")?;
            fixture.validate(case, result)?;
        }
        println!(
            "{}",
            serde_json::json!({"size":fixture.size(),"expected_removed_bytes":fixture.removed_bytes(case),"case":case.name(),"warmup":warmup,"iterations":iterations,"elapsed_ns":elapsed_ns})
        );
        Ok(())
    })
}
