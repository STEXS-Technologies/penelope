//! Bounded single-active-path hierarchy over the same candidate/publication path.
use super::*;
use thiserror::Error;

/// An internal transition retains the active leaf and runs no exit/entry hooks.
/// An external transition exits/enters around the active paths' least common
/// ancestor. An external self-transition exits and reenters the active leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Internal,
    External,
}

/// A guarded selection. Constructing a plan does not perform its actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HierarchyPlan<Node> {
    pub target: Node,
    pub kind: TransitionKind,
    pub transition: TransitionId,
}

/// Only Unhandled bubbles from the active leaf toward its parents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyDecision<Node> {
    Selected(HierarchyPlan<Node>),
    Rejected { reason: RejectCode },
    Unhandled,
}

/// Consumer-owned nested enum model. Node identity and parent relationships must
/// be deterministic and immutable for the pinned definition. There is one active
/// leaf path, no orthogonal regions and no implicit completion-transition loop.
/// Guard selection has no sink: unselected handlers cannot leak intents.
/// External exit paths start at the active leaf, including when selection bubbles
/// to a parent. Parent selection does not introduce a separate transition domain.
pub trait HierarchyModel {
    type State;
    type Event;
    type Effect;
    type Error;
    type Node: Copy + Eq;

    fn definition(&self) -> MachineDefinition;
    fn leaf(&self, state: &Self::State) -> Self::Node;
    fn parent(&self, node: Self::Node) -> Option<Self::Node>;
    /// Checks the state's application invariants, independently of path validation.
    /// # Errors
    /// Returns the concrete application invariant error.
    fn validate(&self, state: &Self::State) -> Result<(), Self::Error>;
    /// Evaluates guards for one node. No action or intent is executed here.
    /// # Errors
    /// An evaluation error aborts dispatch without trying a parent.
    fn select(
        &self,
        node: Self::Node,
        current: &Self::State,
        event: &Self::Event,
        context: &TransitionContext,
    ) -> Result<HierarchyDecision<Self::Node>, Self::Error>;
    /// Constructs isolated candidate state. No Clone bound or mutable live state.
    /// # Errors
    /// Refuses construction failures before any exit/action/entry hook runs.
    fn candidate(
        &self,
        plan: HierarchyPlan<Self::Node>,
        current: &Self::State,
        event: &Self::Event,
        context: &TransitionContext,
    ) -> Result<Self::State, Self::Error>;
    /// Stages exit intents and/or updates isolated candidate data.
    /// # Errors
    /// An error discards the entire attempt, including earlier hooks' intents.
    fn exit<S: IntentSink<Self::Effect>>(
        &self,
        _node: Self::Node,
        _current: &Self::State,
        _next: &mut Self::State,
        _context: &TransitionContext,
        _intents: &mut S,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Runs after exits, before entries. Stages intents rather than performing I/O.
    /// # Errors
    /// An error discards the complete candidate and staged intents.
    fn action<S: IntentSink<Self::Effect>>(
        &self,
        _plan: HierarchyPlan<Self::Node>,
        _current: &Self::State,
        _next: &mut Self::State,
        _context: &TransitionContext,
        _intents: &mut S,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Stages entry intents from the common ancestor toward the target leaf.
    /// # Errors
    /// An error discards the complete candidate and staged intents.
    fn enter<S: IntentSink<Self::Effect>>(
        &self,
        _node: Self::Node,
        _current: &Self::State,
        _next: &mut Self::State,
        _context: &TransitionContext,
        _intents: &mut S,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Optional no-allocation hierarchy adapter. DEPTH bounds every path traversal;
/// it does not add a second owner, publication boundary or transition engine.
pub struct Hierarchical<M, const DEPTH: usize> {
    model: M,
}
impl<M, const DEPTH: usize> Hierarchical<M, DEPTH> {
    pub const fn new(model: M) -> Self {
        Self { model }
    }
}

/// Hierarchy validation is distinct from an application guard/evaluation fault.
#[derive(Debug, Error)]
pub enum HierarchyFailure<ModelError> {
    #[error(transparent)]
    Kernel(#[from] MachineError),
    #[error("hierarchical model evaluation failed")]
    Model(ModelError),
}

struct StatePath<Node, const DEPTH: usize> {
    nodes: [Option<Node>; DEPTH],
    len: usize,
}
impl<Node: Copy + Eq, const DEPTH: usize> StatePath<Node, DEPTH> {
    fn build<M: HierarchyModel<Node = Node>>(model: &M, leaf: Node) -> Result<Self, MachineError> {
        let mut path = Self {
            nodes: [None; DEPTH],
            len: 0,
        };
        let mut next = Some(leaf);
        while let Some(node) = next {
            if path.nodes.iter().flatten().any(|old| *old == node) {
                return Err(MachineError::InvalidHierarchy);
            }
            let slot = path
                .nodes
                .get_mut(path.len)
                .ok_or(MachineError::HierarchyDepth)?;
            *slot = Some(node);
            path.len = path
                .len
                .checked_add(1)
                .ok_or(MachineError::HierarchyDepth)?;
            next = model.parent(node);
        }
        Ok(path)
    }
    fn iter(&self) -> impl DoubleEndedIterator<Item = Node> + '_ {
        self.nodes.iter().take(self.len).flatten().copied()
    }
}
impl<M: HierarchyModel, const DEPTH: usize> Machine for Hierarchical<M, DEPTH> {
    type State = M::State;
    type Event = M::Event;
    type Effect = M::Effect;
    type Error = HierarchyFailure<M::Error>;
    fn definition(&self) -> MachineDefinition {
        self.model.definition()
    }
    fn validate(&self, state: &M::State) -> Result<(), Self::Error> {
        StatePath::<_, DEPTH>::build(&self.model, self.model.leaf(state))?;
        self.model.validate(state).map_err(HierarchyFailure::Model)
    }
    fn transition<S: IntentSink<M::Effect>>(
        &self,
        current: &M::State,
        event: &M::Event,
        context: &TransitionContext,
        intents: &mut S,
    ) -> Result<Transition<M::State>, Self::Error> {
        let source = StatePath::<_, DEPTH>::build(&self.model, self.model.leaf(current))?;
        let mut selected = None;
        for node in source.iter() {
            match self
                .model
                .select(node, current, event, context)
                .map_err(HierarchyFailure::Model)?
            {
                HierarchyDecision::Selected(plan) => {
                    selected = Some(plan);
                    break;
                }
                HierarchyDecision::Rejected { reason } => {
                    return Ok(Transition::Rejected { reason });
                }
                HierarchyDecision::Unhandled => {}
            }
        }
        let Some(plan) = selected else {
            return Ok(Transition::Unhandled);
        };
        if plan.kind == TransitionKind::Internal && plan.target != self.model.leaf(current) {
            return Err(MachineError::InvalidHierarchy.into());
        }
        let target = StatePath::<_, DEPTH>::build(&self.model, plan.target)?;
        let mut next = self
            .model
            .candidate(plan, current, event, context)
            .map_err(HierarchyFailure::Model)?;
        if self.model.leaf(&next) != plan.target {
            return Err(MachineError::InvalidHierarchy.into());
        }
        let (exits, entries) = if plan.kind == TransitionKind::Internal {
            (0, 0)
        } else {
            let common = source
                .iter()
                .rev()
                .zip(target.iter().rev())
                .take_while(|(left, right)| left == right)
                .count();
            let exits = source
                .len
                .checked_sub(common)
                .ok_or(MachineError::InvalidHierarchy)?;
            let entries = target
                .len
                .checked_sub(common)
                .ok_or(MachineError::InvalidHierarchy)?;
            if exits == 0 && entries == 0 {
                (1, 1)
            } else {
                (exits, entries)
            }
        };
        for node in source.iter().take(exits) {
            self.model
                .exit(node, current, &mut next, context, intents)
                .map_err(HierarchyFailure::Model)?;
            intents.status()?;
        }
        self.model
            .action(plan, current, &mut next, context, intents)
            .map_err(HierarchyFailure::Model)?;
        intents.status()?;
        // Take before reversing: enter only the target suffix below the LCA.
        for node in target.nodes.iter().take(entries).rev().flatten().copied() {
            self.model
                .enter(node, current, &mut next, context, intents)
                .map_err(HierarchyFailure::Model)?;
            intents.status()?;
        }
        if self.model.leaf(&next) != plan.target {
            return Err(MachineError::InvalidHierarchy.into());
        }
        Ok(Transition::Accepted {
            next,
            transition: plan.transition,
        })
    }
}
