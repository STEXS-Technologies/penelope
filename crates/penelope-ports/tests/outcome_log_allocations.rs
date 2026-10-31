//! Invalid long scopes must be rejected without cloning their four strings.
#![allow(unsafe_code, clippy::unwrap_used, clippy::panic)]
#[path = "../benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "../benches/support/outcome_log_fixture.rs"]
mod fixture;
use penelope_ports::{OutcomeLog, OutcomeLogValidationError};
fn main() {
    for (profile, count, chunk) in fixture::PROFILES {
        let fixture = fixture::Fixture::new(128, 128, count).unwrap();
        let mut wrong = fixture.outcomes.clone();
        wrong.first_mut().unwrap().definition_digest = penelope_domain::ContentDigest([8; 32]);
        let mut log = OutcomeLog::new(fixture.scope.clone());
        let (result, allocations, _, _) = allocator::measure(|| log.append(&wrong));
        assert_eq!(result, Err(OutcomeLogValidationError::ScopeMismatch));
        assert!(
            allocations <= 1,
            "scope rejection allocated {allocations} times; at most the temporary ID set is needed"
        );
        assert!(log.is_empty());
        fixture.append(&mut log, chunk).unwrap();
        fixture.verify(&log).unwrap();
        println!(
            "profile={profile} scope_rejection_allocations={allocations} repaired=true checksum={:?}",
            fixture.checksum
        );
    }
    // Both persistent containers have spare capacity after two inline outcomes.
    // The next single append needs no temporary duplicate set or owned strings.
    let fixture = fixture::Fixture::new(9, 8, 3).unwrap();
    let mut log = OutcomeLog::new(fixture.scope.clone());
    log.append(fixture.outcomes.get(..2).unwrap()).unwrap();
    let (result, allocations, reallocations, _) =
        allocator::measure(|| log.append(fixture.outcomes.get(2..).unwrap()));
    result.unwrap();
    assert_eq!(allocations, 0, "single append allocated a temporary set");
    assert_eq!(reallocations, 0);
    fixture.verify(&log).unwrap();
    println!("single_append_allocations=0 verified=true");
}
