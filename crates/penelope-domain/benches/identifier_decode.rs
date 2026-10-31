//! Identical validated identifier JSON inputs for decoder comparisons.
use penelope_domain::ActionId;
use std::{hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let long = format!("\"act_{}\"", "x".repeat(124));
    let short = "\"act_benchmark\"";
    let escaped = "\"act_bench\\u006dark\"";
    let iterations = 100_000_u32;
    for (name, input) in [
        ("short", short),
        ("escaped", escaped),
        ("long", long.as_str()),
    ] {
        let expected: ActionId = serde_json::from_str(input)?;
        if name != "long" && expected.as_str() != "act_benchmark" {
            return Err("fixture does not decode to expected identifier".into());
        }
        let began = Instant::now();
        for _ in 0..iterations {
            black_box(serde_json::from_str::<ActionId>(black_box(input))?);
        }
        let elapsed = began.elapsed();
        let nanos = elapsed
            .as_nanos()
            .checked_div(u128::from(iterations))
            .ok_or("invalid iteration count")?;
        println!(
            "identifier JSON decode {name}: {iterations} operations in {elapsed:?} ({nanos} ns/op)"
        );
    }
    Ok(())
}
