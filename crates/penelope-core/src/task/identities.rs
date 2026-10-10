//! Distinct fixed-size identities and counters for recoverable execution.
//!
//! Types preserve their v1 wire representation while preventing accidental
//! substitution between unrelated roles.
//!
//! ```compile_fail
//! use penelope_core::task::{Digest, TaskId, TaskTenantId};
//! let tenant: TaskTenantId = TaskId::from_digest(Digest([1; 32]));
//! ```
//! ```compile_fail
//! use penelope_core::task::{TaskGeneration, TaskSequence};
//! let generation: TaskGeneration = TaskSequence::new(1);
//! ```
//! ```compile_fail
//! use penelope_core::task::{Digest, TaskDefinitionFingerprint, TaskInputDigest};
//! let definition: TaskDefinitionFingerprint = TaskInputDigest::from_digest(Digest([1; 32]));
//! ```
//! ```compile_fail
//! use penelope_core::task::{TaskGeneration, TaskTick};
//! let tick: TaskTick = TaskGeneration::new(1);
//! ```

use super::Digest;

macro_rules! digest_identity {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(transparent))]
        pub struct $name(Digest);

        impl $name {
            /// Assigns an application-supplied digest to this identity role.
            #[must_use]
            pub const fn from_digest(digest: Digest) -> Self {
                Self(digest)
            }

            /// Returns the underlying content identity for explicit encoding.
            #[must_use]
            pub const fn digest(self) -> Digest {
                self.0
            }
        }
    };
}

digest_identity!(
    TaskTenantId,
    "Tenant identity in the portable task protocol."
);
digest_identity!(
    TaskId,
    "Stable task and effect-idempotency identity across generations."
);
digest_identity!(
    TaskDefinitionFingerprint,
    "Identity of the complete pinned task definition."
);
digest_identity!(
    TaskInputDigest,
    "Identity of the immutable input pinned to an execution scope."
);

macro_rules! counter {
    ($name:ident, $integer:ty, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(transparent))]
        pub struct $name($integer);

        impl $name {
            /// Zero is valid for an initial intent, history cursor or clock.
            pub const ZERO: Self = Self(0);

            /// Constructs this counter from its wire value.
            #[must_use]
            pub const fn new(value: $integer) -> Self {
                Self(value)
            }

            /// Returns the value for explicit encoding or resource comparisons.
            #[must_use]
            pub const fn get(self) -> $integer {
                self.0
            }

            /// Advances by an amount without wrapping.
            #[must_use]
            pub const fn checked_add(self, amount: $integer) -> Option<Self> {
                match self.0.checked_add(amount) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            /// Advances by an amount, saturating at the wire representation limit.
            #[must_use]
            pub const fn saturating_add(self, amount: $integer) -> Self {
                Self(self.0.saturating_add(amount))
            }

            /// Subtracts an amount without wrapping below zero.
            #[must_use]
            pub const fn saturating_sub(self, amount: $integer) -> Self {
                Self(self.0.saturating_sub(amount))
            }
        }
    };
}

counter!(
    TaskGeneration,
    u32,
    "Execution generation; zero identifies pre-execution intent."
);
counter!(
    TaskSequence,
    u32,
    "Ordered task-history position, independent of generation."
);
counter!(
    TaskTick,
    u64,
    "Absolute monotonic time, distinct from a duration or counter."
);
