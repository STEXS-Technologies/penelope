//! Timeline identities deliberately have no conversions to task clocks or generations.
//!
//! ```compile_fail
//! use penelope_core::{task::TaskTick, timeline::TimelineStep};
//! let step: TimelineStep = TaskTick::new(1);
//! ```
//! ```compile_fail
//! use penelope_core::timeline::{TimelineRevision, TimelineBranchId};
//! let branch: TimelineBranchId = TimelineRevision::new(1);
//! ```

use crate::task::Digest;

macro_rules! digest_identity {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(Digest);
        impl $name {
            /// Assigns a content identity to this role; does not establish authority.
            #[must_use]
            pub const fn from_digest(value: Digest) -> Self {
                Self(value)
            }
            /// Returns the underlying byte identity for explicit encoding.
            #[must_use]
            pub const fn digest(self) -> Digest {
                self.0
            }
        }
    };
}

digest_identity!(TimelineTenantId, "Tenant identity for a timeline.");
digest_identity!(TimelineId, "Stable timeline identity within a tenant.");
digest_identity!(TimelineModelId, "Pinned model/code identity.");
digest_identity!(TimelineSchemaId, "Pinned state/input schema identity.");
digest_identity!(TimelineInputId, "Immutable input identity.");
digest_identity!(TimelineSnapshotId, "Immutable snapshot identity.");

macro_rules! counter {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(u64);
        impl $name {
            /// Zero-valued protocol counter.
            pub const ZERO: Self = Self(0);
            /// Constructs this role from an explicit protocol value.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }
            /// Returns the fixed-width protocol value.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
            /// Advances without wrapping or saturating identity.
            #[must_use]
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
    TimelineStep,
    "Logical post-input step, independent of wall-clock time."
);
counter!(TimelineRevision, "Monotonic live-head mutation epoch.");
counter!(
    TimelineBranchId,
    "Monotonic branch reservation identity within a scope."
);
counter!(InputRevision, "Immutable version of a logical input.");
