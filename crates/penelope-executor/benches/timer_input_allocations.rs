//! Timer input allocation traffic, separate from ordinary latency measurement.
#[path = "../../penelope-ports/benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "../tests/support/timer_input_fixture.rs"]
mod fixture;
#[path = "../src/timer_input.rs"]
mod implementation;
fn main() -> Result<(), fixture::Error> {
    for (profile, lease) in fixture::profiles()? {
        let expected = implementation::timer_input(&lease)?;
        let (value, allocations, reallocations, requested_bytes) =
            allocator::measure(|| implementation::timer_input(&lease));
        let value = value?;
        if value != expected {
            return Err("timer input mismatch".into());
        }
        println!(
            "{}",
            serde_json::json!({"profile":profile,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"checksum":penelope_domain::ContentDigest::sha256(&serde_json::to_vec(&value)?),"input_id":value.0.input_id,"payload_len":value.1.len()})
        );
    }
    Ok(())
}
