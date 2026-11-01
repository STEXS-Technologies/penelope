//! Batch throughput is separate from individual timer sample percentiles.
#[path = "support/workload.rs"]
mod workload;
fn main() {
    workload::throughput();
}
