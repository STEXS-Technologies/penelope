//! Bounded duplicate identity lookup, preserving the first conflicting position.
/// Returns the earliest lease that conflicts with a later lease. Keep this
/// position separate from lease validation so callers preserve error precedence.
pub(super) fn first_duplicate_index<T, Token: Copy + Eq>(
    leases: &[T],
    identity: impl Fn(&T) -> (&str, Token),
) -> Option<usize> {
    // Small batches avoid sorting; oversized batches retain a safe fallback.
    // Public callers enforce the 128-lease bound before reaching this helper.
    if leases.len() <= 16 || leases.len() > 128 {
        return leases.iter().enumerate().find_map(|(index, lease)| {
            let (action, token) = identity(lease);
            leases
                .iter()
                .skip(index.saturating_add(1))
                .any(|other| {
                    let (other_action, other_token) = identity(other);
                    action == other_action || token == other_token
                })
                .then_some(index)
        });
    }
    let mut names = [("", 0_usize); 128];
    let first_token = identity(leases.first()?).1;
    let mut tokens = [first_token; 128];
    for ((entry, token), (index, lease)) in names
        .iter_mut()
        .zip(tokens.iter_mut())
        .zip(leases.iter().enumerate())
    {
        let (action, value) = identity(lease);
        *entry = (action, index);
        *token = value;
    }
    let names = names.get_mut(..leases.len())?;
    names.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let action_duplicate = names
        .windows(2)
        .filter(|pair| pair.first().map(|entry| entry.0) == pair.last().map(|entry| entry.0))
        .flat_map(|pair| pair.iter().map(|entry| entry.1))
        .min();
    // Tokens are opaque equality-only fences. Compare the compact local copies
    // rather than imposing an ordering or exposing their underlying value.
    let tokens = tokens.get(..leases.len())?;
    let token_duplicate = tokens.iter().enumerate().find_map(|(index, token)| {
        tokens
            .iter()
            .skip(index.saturating_add(1))
            .any(|other| other == token)
            .then_some(index)
    });
    action_duplicate.into_iter().chain(token_duplicate).min()
}

/// Select one validation loop per batch. In each loop, a lease's own failure
/// still takes precedence over a conflict starting at that same position.
#[inline(always)]
pub(super) fn validate_batch<T, Token: Copy + Eq>(
    leases: &[T],
    identity: impl Fn(&T) -> (&str, Token),
    validate: impl Fn(&T) -> Result<(), penelope_ports::PortError>,
) -> Result<(), penelope_ports::PortError> {
    if leases.len() <= 16 {
        for (index, lease) in leases.iter().enumerate() {
            validate(lease)?;
            let (action, token) = identity(lease);
            if leases.iter().skip(index.saturating_add(1)).any(|other| {
                let (other_action, other_token) = identity(other);
                action == other_action || token == other_token
            }) {
                return Err(penelope_ports::PortError::Invariant);
            }
        }
    } else {
        let duplicate = first_duplicate_index(leases, identity);
        for (index, lease) in leases.iter().enumerate() {
            validate(lease)?;
            if duplicate == Some(index) {
                return Err(penelope_ports::PortError::Invariant);
            }
        }
    }
    Ok(())
}
