//! Allocation traffic through the same immediate public service fixtures.
#[path = "../../penelope-ports/benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "../tests/support/lease_batch_fixture.rs"]
mod fixture;
fn main() -> Result<(), fixture::Error> {
    for batch in [1, 8, 16, 32, 64, 128] {
        for width in [9, 128] {
            for order in [0, 1, 2] {
                for timers in [false, true] {
                    let fixture = fixture::Fixture::new(width, batch, order)?;
                    let ports = fixture.prepare(timers);
                    let (output, allocations, reallocations, requested_bytes) =
                        allocator::measure(|| fixture.invoke(&ports, timers));
                    let checksum = fixture.verify(&ports, &output?)?;
                    println!(
                        "{}",
                        serde_json::json!({"batch":batch,"width":width,"order":order,"timers":timers,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"checksum":checksum})
                    );
                }
            }
        }
    }
    Ok(())
}
