//! Timed checkpoints in the real protected consumer, including lost-ACK recovery.
//! This private harness reuses all admission, result and provenance checks.
#[allow(dead_code)] // The standalone consumer's main remains its normal entry point.
#[path = "durable.rs"]
mod consumer;
use std::{path::Path, process::Command, time::Instant};

const PHASES: [&str; 12] = [
    "begin",
    "bootstrap_restored",
    "initial_committed",
    "recovered",
    "native_admitted",
    "lost_ack_recovered",
    "native_setup",
    "dispatch_return",
    "durable_completed",
    "result_verified",
    "result_committed",
    "saga_admitted",
];
const SAMPLES: usize = if cfg!(debug_assertions) { 3 } else { 101 };
const WARMUP: usize = if cfg!(debug_assertions) { 1 } else { 8 };
fn worker(root: &Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("PENELOPE_PIPELINE_WORKER") {
        return Ok(path.into());
    }
    let binary = root.join("worker");
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/penelope-process-runtime/tests/fixtures/worker.c");
    let status = Command::new("cc")
        .args([
            "-nostdlib",
            "-static",
            "-fno-builtin",
            "-fno-stack-protector",
            "-fno-pie",
            "-no-pie",
            "-o",
        ])
        .arg(&binary)
        .arg(source)
        .status()?;
    if !status.success() {
        return Err("worker compilation failed".into());
    }
    Ok(binary)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let worker = worker(directory.path())?;
    let mut samples: Vec<Vec<u128>> = (0..PHASES.len())
        .map(|_| Vec::with_capacity(SAMPLES))
        .collect();
    for sample in 0..WARMUP + SAMPLES {
        let began = Instant::now();
        let mut marks = Vec::with_capacity(PHASES.len());
        consumer::run(Some(worker.clone()), |phase| {
            marks.push((phase, began.elapsed().as_nanos()))
        })?;
        let finished = began.elapsed().as_nanos();
        if marks.iter().map(|(phase, _)| *phase).ne(PHASES) {
            return Err("missing, repeated or reordered pipeline phase".into());
        }
        let mut times: Vec<_> = marks.iter().map(|(_, nanos)| *nanos).collect();
        times.push(finished);
        if times.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err("pipeline timestamps moved backwards".into());
        }
        for (index, pair) in times.windows(2).enumerate() {
            let elapsed = pair[1] - pair[0];
            println!(
                "native_pipeline_sample,{sample},{},{elapsed},{}",
                PHASES[index],
                sample < WARMUP
            );
            if sample >= WARMUP {
                samples[index].push(elapsed);
            }
        }
        println!(
            "native_pipeline_total,{sample},{},{finished},{}",
            times[0],
            sample < WARMUP
        );
    }
    for (phase, mut values) in PHASES.into_iter().zip(samples) {
        values.sort_unstable();
        let percentile = |percent: usize| values[(SAMPLES * percent).div_ceil(100) - 1];
        println!(
            "native_pipeline_summary,{phase},{SAMPLES},{},{},{}",
            percentile(50),
            percentile(95),
            percentile(99)
        );
    }
    Ok(())
}
