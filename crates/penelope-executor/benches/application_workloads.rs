//! Paired complete workflows versus identical directly executed CPU work.
//! Default invocation is a bounded correctness smoke run; use the Python runner.
#![allow(clippy::arithmetic_side_effects)]
#[path = "support/application_workloads/workflow.rs"]
mod workload;
use std::{hint::black_box, time::Instant};
use workload::{Case, Error, Fixture, kernel};

#[derive(Clone, Copy)]
struct Config {
    work: u32,
    rounds: u32,
    iterations: u32,
    workers: u32,
}
fn run_worker(
    fixture: &mut Fixture,
    case: Case,
    managed: bool,
    config: Config,
    worker: usize,
) -> Result<u64, Error> {
    let mut checksum = 0_u64;
    for index in 0..config.iterations {
        let seed = u64::try_from(worker)? * 1_000_000 + u64::from(index);
        let value = if managed {
            fixture.run(case, black_box(seed), black_box(config.work))?
        } else {
            let mut value = seed;
            for _ in 0..case.calls() {
                value = kernel(black_box(value), black_box(config.work));
            }
            value
        };
        checksum = checksum.rotate_left(1) ^ black_box(value);
    }
    Ok(checksum)
}
fn measure(case: Case, managed: bool, config: Config) -> Result<(u128, u64), Error> {
    // Open/migrate databases before timing. Reopening for recovery is timed.
    // Baseline execution does not open an idle SQLite worker.
    let mut fixtures: Vec<_> = (0..config.workers)
        .map(|_| Fixture::new(if managed { case } else { Case::Linear }))
        .collect::<Result<_, _>>()?;
    let began = Instant::now();
    let checksum = if config.workers == 1 {
        run_worker(
            fixtures.first_mut().ok_or("missing fixture")?,
            case,
            managed,
            config,
            0,
        )?
    } else {
        std::thread::scope(|scope| -> Result<u64, Error> {
            let handles: Vec<_> = fixtures
                .iter_mut()
                .enumerate()
                .map(|(worker, fixture)| {
                    scope.spawn(move || run_worker(fixture, case, managed, config, worker))
                })
                .collect();
            let mut checksum = 0_u64;
            for handle in handles {
                checksum = checksum.rotate_left(1)
                    ^ handle
                        .join()
                        .map_err(|_panic| "workload thread panicked")??;
            }
            Ok(checksum)
        })?
    };
    // Fixture/database teardown runs after capturing elapsed time.
    Ok((began.elapsed().as_nanos(), checksum))
}
fn main() -> Result<(), Error> {
    let mut config = Config {
        work: 64,
        rounds: 2,
        iterations: 2,
        workers: 1,
    };
    let mut selected = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--bench" {
            continue;
        }
        let value = arguments.next().ok_or("missing option value")?;
        match argument.as_str() {
            "--work" => config.work = value.parse()?,
            "--rounds" => config.rounds = value.parse()?,
            "--iterations" => config.iterations = value.parse()?,
            "--workers" => config.workers = value.parse()?,
            "--case" => selected = Some(Case::parse(&value)?),
            _ => return Err(format!("unknown option: {argument}").into()),
        }
    }
    if !(1..=64).contains(&config.rounds)
        || !(1..=1024).contains(&config.iterations)
        || !(1..=32).contains(&config.workers)
        || config.work > 16_777_216
    {
        return Err("configuration outside workload bounds".into());
    }
    for case in Case::ALL {
        if selected.is_some_and(|selected| selected != case) {
            continue;
        }
        let warmup = Config {
            rounds: 1,
            iterations: 1,
            ..config
        };
        if measure(case, false, warmup)?.1 != measure(case, true, warmup)?.1 {
            return Err("warmup outputs differ".into());
        }
        for round in 0..config.rounds {
            let (direct, managed) = if round % 2 == 0 {
                (measure(case, false, config)?, measure(case, true, config)?)
            } else {
                let managed = measure(case, true, config)?;
                (measure(case, false, config)?, managed)
            };
            if direct.1 != managed.1 {
                return Err("workflow output checksums differ".into());
            }
            println!(
                "{}",
                serde_json::json!({
                    "schema":1,"case":case.name(),"work":config.work,"round":round,
                    "iterations_per_worker":config.iterations,"workers":config.workers,
                    "workflows":config.iterations*config.workers,"calls_per_workflow":case.calls(),
                    "direct_ns":direct.0,"penelope_ns":managed.0,"checksum":direct.1,
                    "direct_first":round%2==0,"verified":true,
                })
            );
        }
    }
    Ok(())
}
