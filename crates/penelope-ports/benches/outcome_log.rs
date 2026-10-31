//! Ordinary allocator; append only is timed, with owned results verified afterward.
#[path = "support/outcome_log_fixture.rs"]
mod fixture;
use fixture::{Error, Fixture, PROFILES};
use penelope_ports::OutcomeLog;
use std::{hint::black_box, time::Instant};
fn main() -> Result<(), Error> {
    let full = match std::env::var("PENELOPE_OUTCOME_LOG_FULL") {
        Ok(value) if value == "1" => true,
        Ok(_) => return Err("PENELOPE_OUTCOME_LOG_FULL must be 1 when set".into()),
        Err(std::env::VarError::NotPresent) => false,
        Err(std::env::VarError::NotUnicode(_)) => return Err("non-Unicode benchmark option".into()),
    };
    for scope_len in [9, 128] {
        for id_len in [8, 128] {
            for (profile, count, chunk) in PROFILES {
                let fixture = Fixture::new(scope_len, id_len, count)?;
                let iterations = if full {
                    if count > 128 { 32 } else { 1000 }
                } else {
                    1
                };
                for _ in 0..if full { 8 } else { 1 } {
                    let mut log = OutcomeLog::new(fixture.scope.clone());
                    fixture.append(&mut log, chunk)?;
                    fixture.verify(&log)?;
                }
                let mut elapsed_ns = 0_u128;
                for _ in 0..iterations {
                    let mut log = OutcomeLog::new(fixture.scope.clone());
                    let start = Instant::now();
                    black_box(&fixture).append(black_box(&mut log), chunk)?;
                    elapsed_ns = elapsed_ns
                        .checked_add(start.elapsed().as_nanos())
                        .ok_or("duration overflow")?;
                    fixture.verify(black_box(&log))?;
                }
                println!(
                    "{}",
                    serde_json::json!({"scope_len":scope_len,"id_len":id_len,"profile":profile,"count":count,"chunk":chunk,"iterations":iterations,"elapsed_ns":elapsed_ns,"checksum":fixture.checksum,"verified":true})
                );
            }
        }
    }
    Ok(())
}
