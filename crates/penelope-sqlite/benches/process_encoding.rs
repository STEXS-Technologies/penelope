//! Fresh commit latency with the ordinary system allocator.
#[path = "support/process_encoding_fixture.rs"]
mod fixture;
use penelope_ports::ProcessStore;
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    fixture::run(|store, batch| {
        let start = Instant::now();
        let receipt = fixture::block_on(ProcessStore::commit(store, batch))??;
        let elapsed_ns = start.elapsed().as_nanos();
        Ok((receipt, serde_json::json!({"elapsed_ns":elapsed_ns})))
    })
}
