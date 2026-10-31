//! Bounded duplicate indexing must not allocate.
#[path = "../../penelope-ports/benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "../src/lease_batch_index.rs"]
mod implementation;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut profiles = 0_u16;
    for count in [0, 1, 8, 16, 17, 32, 64, 128, 129] {
        for width in [9_usize, 128] {
            let mut leases = Vec::new();
            for index in 0..count {
                leases.push((
                    format!(
                        "act_{}{:04}",
                        "x".repeat(width.checked_sub(8).ok_or("short ID")?),
                        index
                    ),
                    u64::try_from(index)?,
                ));
            }
            let (duplicate, allocations, reallocations, requested_bytes) =
                allocator::measure(|| {
                    implementation::validate_batch(
                        &leases,
                        |a| (a.0.as_str(), a.1),
                        |_lease| Ok(()),
                    )
                });
            if duplicate.is_err() || (allocations, reallocations, requested_bytes) != (0, 0, 0) {
                return Err("duplicate index allocated or found a false conflict".into());
            }
            profiles = profiles.checked_add(1).ok_or("profile overflow")?;
        }
    }
    println!(
        "lease_batch_allocation_profiles={profiles} allocations=0 reallocations=0 requested_bytes=0"
    );
    Ok(())
}
