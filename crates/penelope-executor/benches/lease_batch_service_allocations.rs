//! Allocation traffic through the same immediate public service fixtures.
#[path = "../../penelope-ports/benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "../tests/support/lease_batch_fixture.rs"]
mod fixture;
fn main() -> Result<(), fixture::Error> {
    let mut path = None;
    for argument in std::env::args().skip(1) {
        if argument == "--bench" {
            continue;
        }
        if path.is_some() || (argument != "--timers-only" && argument != "--outbox-only") {
            return Err("unknown or repeated service path".into());
        }
        path = Some(argument);
    }
    for batch in [1, 8, 16, 32, 64, 128] {
        for width in [9, 128] {
            for order in [0, 1, 2] {
                for timers in [false, true] {
                    if (timers && path.as_deref() == Some("--outbox-only"))
                        || (!timers && path.as_deref() == Some("--timers-only"))
                    {
                        continue;
                    }
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
