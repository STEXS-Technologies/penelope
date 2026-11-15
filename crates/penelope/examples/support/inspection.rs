use penelope::diagnostics::PreparedDecision;

/// One inspection function, with each engine retaining its own state/record types.
pub fn inspect<D: PreparedDecision>(name: &str, decision: &D) {
    println!(
        "{name}: predecessor={}, planned intents={}",
        decision.before().is_some(),
        decision.intents().count()
    );
}
