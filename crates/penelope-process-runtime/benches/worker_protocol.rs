//! In-process protocol costs, excluding worker startup, SQLite and fencing.
#[path = "../tests/support/protocol_fixture.rs"]
mod fixture;
use penelope_core::task::runtime::TaskPortError;
use penelope_process_runtime::worker::{read_request, write_proposal};
use std::{hint::black_box, io::Cursor, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metadata = fixture::metadata();
    for checkpoint in [false, true] {
        let iterations = 100_000_u32;
        let began = Instant::now();
        for _ in 0..iterations {
            let mut frame = [0_u8; 61];
            write_proposal(
                &mut Cursor::new(&mut frame[..]),
                black_box(&metadata),
                checkpoint,
                1,
                black_box(&[0; 8]),
            )?;
            black_box(frame);
        }
        let elapsed = began.elapsed();
        let nanos = elapsed
            .as_nanos()
            .checked_div(u128::from(iterations))
            .ok_or("invalid iterations")?;
        println!(
            "worker proposal checkpoint={checkpoint}: {iterations} operations in {elapsed:?} ({nanos} ns/op)"
        );
    }
    for valid in [true, false] {
        let mut metadata = metadata.clone();
        if !valid {
            metadata.definition.budget.max_input_bytes = 3;
            metadata.scope.definition = metadata.definition.fingerprint();
        }
        let packet = fixture::packet(&metadata)?;
        let iterations = 10_000_u32;
        let began = Instant::now();
        for _ in 0..iterations {
            let result = read_request(&mut Cursor::new(black_box(&packet)), 64, 64);
            if valid {
                black_box(result?);
            } else if result.err() != Some(TaskPortError::Corrupt) {
                return Err("wrong rejection".into());
            }
        }
        let elapsed = began.elapsed();
        let nanos = elapsed
            .as_nanos()
            .checked_div(u128::from(iterations))
            .ok_or("invalid iterations")?;
        println!(
            "worker request valid={valid}: {iterations} operations in {elapsed:?} ({nanos} ns/op)"
        );
    }
    Ok(())
}
