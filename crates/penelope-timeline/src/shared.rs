//! Optional hosted profile. One lock covers state, head, reservations and intents.
extern crate std;
use crate::{LiveTimeline, TimelineError};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

type Shared<State, Effect, const SESSIONS: usize, const EFFECTS: usize> =
    Arc<RwLock<LiveTimeline<State, Effect, SESSIONS, EFFECTS>>>;

/// Shared hosted publication; cloning a handle never clones or rewinds metadata.
/// Do not acquire a write lock while retaining a read guard on the same value.
/// A poisoned write lock fails closed and requires application reconstruction.
pub struct SharedTimeline<State, Effect, const SESSIONS: usize, const EFFECTS: usize> {
    inner: Shared<State, Effect, SESSIONS, EFFECTS>,
}
impl<State, Effect, const SESSIONS: usize, const EFFECTS: usize> Clone
    for SharedTimeline<State, Effect, SESSIONS, EFFECTS>
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}
impl<State, Effect, const SESSIONS: usize, const EFFECTS: usize>
    SharedTimeline<State, Effect, SESSIONS, EFFECTS>
{
    /// Allocates one shared lock around a complete live publication.
    #[must_use]
    pub fn new(live: LiveTimeline<State, Effect, SESSIONS, EFFECTS>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(live)),
        }
    }
    /// Borrows a consistent state/head pair until the returned guard is released.
    /// # Errors
    /// Refuses a poisoned publication rather than guessing which operation completed.
    #[allow(clippy::type_complexity)]
    pub fn read(
        &self,
    ) -> Result<RwLockReadGuard<'_, LiveTimeline<State, Effect, SESSIONS, EFFECTS>>, TimelineError>
    {
        self.inner
            .read()
            .map_err(|_poison| TimelineError::PublicationUnavailable)
    }
    /// Acquires exclusive ownership for validated append, correction and handoff.
    /// Replay may execute off-lock and use its pinned revision at publication time.
    /// # Errors
    /// Refuses a poisoned publication; no raw mutable state accessor is exposed.
    #[allow(clippy::type_complexity)]
    pub fn write(
        &self,
    ) -> Result<RwLockWriteGuard<'_, LiveTimeline<State, Effect, SESSIONS, EFFECTS>>, TimelineError>
    {
        self.inner
            .write()
            .map_err(|_poison| TimelineError::PublicationUnavailable)
    }
}
