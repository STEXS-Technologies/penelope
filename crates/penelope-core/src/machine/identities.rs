//! Machine roles are distinct from saga, task and timeline protocol identities.
//!
//! ```compile_fail
//! use penelope_core::{machine::MachineRevision, timeline::TimelineRevision};
//! let revision: MachineRevision = TimelineRevision::new(1);
//! ```
//! ```compile_fail
//! use penelope_core::machine::{MachineId, MachineEventId};
//! let event: MachineEventId = MachineId::from_bytes([1; 32]);
//! ```

macro_rules! identifier {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name([u8; 32]);
        impl $name {
            /// Assigns bytes to this role; does not authenticate them.
            pub const fn from_bytes(value: [u8; 32]) -> Self {
                Self(value)
            }
            /// Explicit bytes for storage/integration adapters.
            pub const fn bytes(self) -> [u8; 32] {
                self.0
            }
        }
    };
}
identifier!(
    MachineTenantId,
    "Tenant identity for an application machine."
);
identifier!(MachineId, "Machine instance identity within its tenant.");
identifier!(MachineDefinitionId, "Pinned model/code fingerprint.");
identifier!(MachineEventId, "Caller-assigned immutable event identity.");
identifier!(
    MachineCommitId,
    "Content identity of a complete machine commit record."
);
identifier!(
    MachineSourceId,
    "Application-pinned source namespace for machine causation."
);
identifier!(
    MachineCauseId,
    "Occurrence identity within a pinned causation source."
);
identifier!(
    MachineCorrelationId,
    "Application correlation identity, never authority."
);
identifier!(
    MachineIntentId,
    "Stable application effect idempotency identity."
);

macro_rules! counter {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(u64);
        impl $name {
            pub const ZERO: Self = Self(0);
            /// Constructs an explicit role-specific value.
            pub const fn new(value: u64) -> Self {
                Self(value)
            }
            pub const fn get(self) -> u64 {
                self.0
            }
            /// Advances without wrapping or saturating an identity.
            pub const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }
    };
}
counter!(
    MachineRevision,
    "Accepted transition sequence within one machine."
);
counter!(
    MachineLogicalTime,
    "Supplied logical time, not an ambient clock."
);
counter!(TransitionId, "Stable application-defined transition code.");
counter!(RejectCode, "Bounded application-defined refusal code.");
counter!(
    MachineEventType,
    "Stable application-defined event kind within the pinned event schema."
);

identifier!(
    MachineLeaseOwner,
    "Host-authenticated machine delivery worker identity."
);
counter!(
    MachineLeaseToken,
    "Opaque machine delivery fencing token; zero is not a live lease."
);
counter!(
    MachineLeaseTick,
    "Trusted hosted lease time, independent of captured event logical time."
);
counter!(
    MachineDeliveryAttempt,
    "Durable handoff attempt count; reclaim never resets uncertainty."
);
