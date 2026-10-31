//! Allocation traffic is measured separately from ordinary append latency.
#![allow(unsafe_code)]
#[path = "support/outcome_log_allocator.rs"]
mod allocator;
#[path = "support/outcome_log_fixture.rs"]
mod fixture;
use fixture::{Error, Fixture, PROFILES};
use penelope_ports::OutcomeLog;
fn main() -> Result<(), Error> {
    for scope_len in [9, 128] {
        for id_len in [8, 128] {
            for (profile, count, chunk) in PROFILES {
                let fixture = Fixture::new(scope_len, id_len, count)?;
                for sample in 0..8 {
                    let mut log = OutcomeLog::new(fixture.scope.clone());
                    let (result, allocations, reallocations, requested_bytes) =
                        allocator::measure(|| fixture.append(&mut log, chunk));
                    result?;
                    fixture.verify(&log)?;
                    println!(
                        "{}",
                        serde_json::json!({"scope_len":scope_len,"id_len":id_len,"profile":profile,"count":count,"chunk":chunk,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"checksum":fixture.checksum,"verified":true})
                    );
                }
            }
        }
    }
    Ok(())
}
