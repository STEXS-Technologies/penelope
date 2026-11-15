//! Differential admission rules preserved from the pre-extraction engines.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use penelope_core::__private::admit_intent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    Capacity,
    Duplicate,
}
const ERRORS: (Refusal, Refusal) = (Refusal::Capacity, Refusal::Duplicate);

type Outcome = Result<(usize, u64), Refusal>;

// Original timeline push preflight, without payload mutation or domain mapping.
#[allow(clippy::manual_contains)] // Preserve the original admission algorithm.
fn legacy_timeline<const C: usize>(
    ids: &[u8],
    bytes: u64,
    limits: (usize, u64),
    id: u8,
    charge: u64,
) -> Outcome {
    if ids.len() >= limits.0 || ids.len() >= C {
        return Err(Refusal::Capacity);
    }
    if ids.iter().any(|old| *old == id) {
        return Err(Refusal::Duplicate);
    }
    let next_bytes = bytes.checked_add(charge).ok_or(Refusal::Capacity)?;
    if next_bytes > limits.1 {
        return Err(Refusal::Capacity);
    }
    let next_len = ids.len().checked_add(1).ok_or(Refusal::Capacity)?;
    Ok((next_len, next_bytes))
}

// Original machine staging keeps retained and tentative usage separately.
#[allow(clippy::manual_contains)] // Preserve the original admission algorithm.
fn legacy_machine<const C: usize>(
    old: (&[u8], u64),
    new: (&[u8], u64),
    limits: (usize, u64),
    id: u8,
    charge: u64,
) -> Outcome {
    let count = old
        .0
        .len()
        .checked_add(new.0.len())
        .ok_or(Refusal::Capacity)?;
    if count >= C || count >= limits.0 {
        return Err(Refusal::Capacity);
    }
    if old.0.iter().any(|old_id| *old_id == id) || new.0.iter().any(|new_id| *new_id == id) {
        return Err(Refusal::Duplicate);
    }
    let new_bytes = new.1.checked_add(charge).ok_or(Refusal::Capacity)?;
    let total = old.1.checked_add(new_bytes).ok_or(Refusal::Capacity)?;
    if total > limits.1 {
        return Err(Refusal::Capacity);
    }
    let len = new.0.len().checked_add(1).ok_or(Refusal::Capacity)?;
    Ok((old.0.len() + len, total))
}

fn compare<const C: usize>() {
    let identities = [1, 2, 3, 4];
    for len in 0..=C {
        let ids = identities.get(..len).unwrap();
        for split in 0..=len {
            for bytes in [0, 1, u64::MAX - 1, u64::MAX] {
                for old_bytes in [0, bytes / 2, bytes] {
                    for charge in [0, 1, 2, u64::MAX] {
                        for max_count in [0, 1, C, usize::MAX] {
                            for max_bytes in [0, 1, u64::MAX - 1, u64::MAX] {
                                for id in 0..=4 {
                                    let limits = (max_count, max_bytes);
                                    let actual = admit_intent::<C, _, _>(
                                        len,
                                        bytes,
                                        limits,
                                        ids.iter().copied(),
                                        id,
                                        charge,
                                        ERRORS,
                                    );
                                    assert_eq!(
                                        actual,
                                        legacy_timeline::<C>(ids, bytes, limits, id, charge)
                                    );
                                    assert_eq!(
                                        actual,
                                        legacy_machine::<C>(
                                            (ids.get(..split).unwrap(), old_bytes),
                                            (ids.get(split..).unwrap(), bytes - old_bytes),
                                            limits,
                                            id,
                                            charge,
                                        )
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn old_timeline_and_split_machine_admission_agree_at_all_bounds() {
    compare::<0>();
    compare::<1>();
    compare::<4>();
}

#[test]
fn count_exhaustion_does_not_evaluate_identities_and_duplicate_precedes_bytes() {
    let ids = core::iter::from_fn(|| -> Option<u8> { panic!("must not inspect ids") });
    assert_eq!(
        admit_intent::<1, _, _>(1, u64::MAX, (1, 0), ids, 7, 1, ERRORS),
        Err(Refusal::Capacity)
    );
    assert_eq!(
        admit_intent::<2, _, _>(1, u64::MAX, (2, 0), [7].into_iter(), 7, 1, ERRORS),
        Err(Refusal::Duplicate)
    );
    assert_eq!(
        admit_intent::<{ usize::MAX }, _, _>(
            usize::MAX,
            0,
            (usize::MAX, 0),
            core::iter::empty(),
            7,
            0,
            ERRORS
        ),
        Err(Refusal::Capacity)
    );
}
