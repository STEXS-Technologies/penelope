//! Compare the optimized validator with the unchanged 0.2/initial-0.3 algorithm.
#![allow(clippy::unwrap_used)]
use penelope_domain::{ContentDigest, MAX_DEFINITION_STEPS, StepId};
use penelope_executor::engine::{
    GraphDefinitionError, GraphTransition, GraphTransitionOutcome, MAX_GRAPH_TRANSITIONS,
    ProcessGraphDefinition, RetryPolicy, StepPlan,
};
use proptest::prelude::*;
use std::{collections::BTreeSet, num::NonZeroU32};

// Frozen reference from b2e815f. Keep its independent tree-based traversal so
// generated definitions check exact acceptance and error precedence.
fn reference(graph: &ProcessGraphDefinition) -> Result<(), GraphDefinitionError> {
    if graph.steps.is_empty() {
        return Err(GraphDefinitionError::EmptySteps);
    }
    if graph.steps.len() > MAX_DEFINITION_STEPS {
        return Err(GraphDefinitionError::StepLimitExceeded);
    }
    if graph.transitions.len() > MAX_GRAPH_TRANSITIONS {
        return Err(GraphDefinitionError::TransitionLimitExceeded);
    }
    let step_ids: BTreeSet<&StepId> = graph.steps.iter().map(|step| &step.step_id).collect();
    if step_ids.len() != graph.steps.len() {
        return Err(GraphDefinitionError::DuplicateStep);
    }
    if !step_ids.contains(&graph.entry_step_id) {
        return Err(GraphDefinitionError::UnknownEntryStep);
    }
    let mut transition_keys = BTreeSet::new();
    for transition in &graph.transitions {
        if !step_ids.contains(&transition.from_step) {
            return Err(GraphDefinitionError::UnknownSourceStep);
        }
        if let Some(to_step) = &transition.to_step {
            if !step_ids.contains(to_step) {
                return Err(GraphDefinitionError::UnknownDestinationStep);
            }
        } else if transition.on == GraphTransitionOutcome::RetryableFailure {
            return Err(GraphDefinitionError::MissingDestination);
        }
        if !transition_keys.insert((&transition.from_step, transition.on)) {
            return Err(GraphDefinitionError::DuplicateTransition);
        }
    }
    let mut reachable = BTreeSet::new();
    let mut pending = vec![&graph.entry_step_id];
    while let Some(step_id) = pending.pop() {
        if !reachable.insert(step_id) {
            continue;
        }
        for transition in graph
            .transitions
            .iter()
            .filter(|edge| &edge.from_step == step_id)
        {
            if let Some(next) = &transition.to_step {
                pending.push(next);
            }
        }
    }
    if step_ids.iter().any(|step_id| !reachable.contains(step_id)) {
        return Err(GraphDefinitionError::UnreachableStep);
    }
    Ok(())
}

fn step(key: u16) -> StepId {
    format!("stp_{key}").parse().unwrap()
}
fn definition(
    keys: &[u16],
    entry: u16,
    edges: &[(u16, u8, Option<u16>)],
) -> ProcessGraphDefinition {
    ProcessGraphDefinition {
        definition_id: "def_parity".parse().unwrap(),
        definition_version: "dfv_one".parse().unwrap(),
        definition_digest: ContentDigest([1; 32]),
        steps: keys
            .iter()
            .map(|key| {
                StepPlan::canonical_command(
                    step(*key),
                    ContentDigest([2; 32]),
                    RetryPolicy::no_retry(),
                )
            })
            .collect(),
        entry_step_id: step(entry),
        transitions: edges
            .iter()
            .map(|(from, outcome, to)| GraphTransition {
                from_step: step(*from),
                on: match outcome {
                    0 => GraphTransitionOutcome::Succeeded,
                    1 => GraphTransitionOutcome::RetryableFailure,
                    _ => GraphTransitionOutcome::TerminalFailure,
                },
                to_step: to.map(step),
            })
            .collect(),
        max_step_visits: NonZeroU32::new(128).unwrap(),
    }
}

proptest! {
    #[test]
    fn arbitrary_graphs_preserve_exact_validation_errors(
        keys in prop::collection::vec(0u16..132, 0..=129),
        entry in 0u16..132,
        edges in prop::collection::vec((0u16..132, 0u8..3, prop::option::of(0u16..132)), 0..=513),
    ) {
        let graph = definition(&keys, entry, &edges);
        prop_assert_eq!(graph.validate(), reference(&graph));
    }

    #[test]
    fn reachable_chains_cycles_and_reordered_steps_remain_valid(
        count in 1u16..=128,
        reverse in any::<bool>(),
        cycle in any::<bool>(),
    ) {
        let mut keys: Vec<_> = (0..count).collect();
        if reverse { keys.reverse(); }
        let edges: Vec<_> = (0..count).map(|from| {
            let next = from.checked_add(1).unwrap();
            (from, 0, if next < count { Some(next) } else if cycle { Some(0) } else { None })
        }).collect();
        let graph = definition(&keys, 0, &edges);
        prop_assert_eq!(reference(&graph), Ok(()));
        prop_assert_eq!(graph.validate(), Ok(()));
    }
}

#[test]
fn bounds_references_duplicate_keys_and_disconnected_cycles_preserve_errors() {
    let cases = [
        (definition(&[], 0, &[]), GraphDefinitionError::EmptySteps),
        (
            definition(&(0..129).collect::<Vec<_>>(), 0, &[]),
            GraphDefinitionError::StepLimitExceeded,
        ),
        (
            definition(&[0], 0, &vec![(0, 0, None); 513]),
            GraphDefinitionError::TransitionLimitExceeded,
        ),
        (
            definition(&[0, 0], 0, &[]),
            GraphDefinitionError::DuplicateStep,
        ),
        (
            definition(&[0], 1, &[]),
            GraphDefinitionError::UnknownEntryStep,
        ),
        (
            definition(&[0], 0, &[(1, 0, None)]),
            GraphDefinitionError::UnknownSourceStep,
        ),
        (
            definition(&[0], 0, &[(0, 0, Some(1))]),
            GraphDefinitionError::UnknownDestinationStep,
        ),
        (
            definition(&[0], 0, &[(0, 1, None)]),
            GraphDefinitionError::MissingDestination,
        ),
        (
            definition(&[0], 0, &[(0, 0, None), (0, 0, None)]),
            GraphDefinitionError::DuplicateTransition,
        ),
        (
            definition(
                &[0, 1, 2],
                0,
                &[(0, 0, None), (1, 0, Some(2)), (2, 0, Some(1))],
            ),
            GraphDefinitionError::UnreachableStep,
        ),
    ];
    for (graph, error) in cases {
        assert_eq!(reference(&graph), Err(error));
        assert_eq!(graph.validate(), Err(error));
    }
}

#[test]
fn maximum_size_branching_graph_preserves_reachability_through_cycles() {
    let count = u16::try_from(MAX_DEFINITION_STEPS).unwrap();
    let mut keys: Vec<_> = (0..count).collect();
    let mut edges = Vec::new();
    for from in 0..count {
        let successor = from.checked_add(1).unwrap();
        let next = if successor == count { 0 } else { successor };
        edges.extend([(from, 0, Some(next)), (from, 1, Some(0)), (from, 2, None)]);
    }
    keys.reverse();
    edges.reverse();
    let graph = definition(&keys, 0, &edges);
    assert_eq!(reference(&graph), Ok(()));
    assert_eq!(graph.validate(), Ok(()));
}

#[test]
fn every_bounded_source_and_outcome_key_rejects_duplicates_with_original_precedence() {
    let count = u16::try_from(MAX_DEFINITION_STEPS).unwrap();
    let keys: Vec<_> = (0..count).rev().collect();
    let edges: Vec<_> = (0..count)
        .flat_map(|from| {
            let successor = from.checked_add(1).unwrap();
            let next = if successor == count { 0 } else { successor };
            [(from, 0, Some(next)), (from, 1, Some(0)), (from, 2, None)]
        })
        .collect();
    let base = definition(&keys, 0, &edges);
    assert_eq!(base.validate(), Ok(()));
    for (source, outcome, destination) in edges {
        let mut graph = base.clone();
        let duplicate = definition(&[source], source, &[(source, outcome, destination)])
            .transitions
            .remove(0);
        graph.transitions.push(duplicate);
        assert_eq!(
            reference(&graph),
            Err(GraphDefinitionError::DuplicateTransition)
        );
        assert_eq!(graph.validate(), reference(&graph));
        // Destination validation still precedes duplicate-key classification.
        graph.transitions.last_mut().unwrap().to_step = Some(step(count));
        assert_eq!(
            reference(&graph),
            Err(GraphDefinitionError::UnknownDestinationStep)
        );
        assert_eq!(graph.validate(), reference(&graph));
    }
}
