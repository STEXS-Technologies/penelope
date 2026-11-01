//! Latency measurements without allocation instrumentation.
#[path = "support/workload.rs"]
mod workload;
fn main() {
    workload::run(workload::Latency::new());
}
