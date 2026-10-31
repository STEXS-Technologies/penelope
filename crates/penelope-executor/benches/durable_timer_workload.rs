//! Complete synthetic durable timers; setup/open and final verification are untimed.
#![allow(clippy::arithmetic_side_effects)]
#[path = "../tests/support/timer_input_legacy.rs"]
mod legacy;
#[path = "support/timer_workload/ports.rs"]
mod ports;
#[path = "support/timer_workload/workflow.rs"]
mod workflow;
use ports::Error;
fn main() -> Result<(), Error> {
    let mut batch = 1_u16;
    let mut width = 9_usize;
    let mut work = 64_u32;
    let mut rounds = 1_u32;
    let mut reopen = false;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--bench" {
            continue;
        }
        let value = args.next().ok_or("missing option value")?;
        match argument.as_str() {
            "--batch" => batch = value.parse()?,
            "--width" => width = value.parse()?,
            "--work" => work = value.parse()?,
            "--rounds" => rounds = value.parse()?,
            "--reopen" => reopen = value.parse()?,
            _ => return Err("unknown timer workload option".into()),
        }
    }
    if !(1..=128).contains(&batch)
        || ![9, 128].contains(&width)
        || !(1..=32).contains(&rounds)
        || work > 16_777_216
    {
        return Err("timer workload configuration outside bounds".into());
    }
    for round in 0..rounds {
        let row = workflow::measure(batch, width, work, reopen, round)?;
        println!("{}", serde_json::to_string(&row)?);
    }
    Ok(())
}
