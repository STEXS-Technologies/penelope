//! Application-level reconciliation using only the public timeline API.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use penelope_core::task::Digest;
use penelope_timeline::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Player {
    position: u64,
    velocity: u64,
}
impl HistoryPayload for Player {
    fn digest(&self) -> Digest {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&self.position.to_le_bytes());
        bytes[8..].copy_from_slice(&self.velocity.to_le_bytes());
        Digest::of(&bytes)
    }
    fn byte_len(&self) -> u64 {
        16
    }
}
struct Movement;
impl RewindableComputation for Movement {
    type State = Player;
    type Input = u64;
    type Effect = ();
    type Error = TimelineError;

    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut Player,
        acceleration: &u64,
        _context: StepContext,
        _effects: &mut S,
    ) -> Result<(), TimelineError> {
        state.velocity = state
            .velocity
            .checked_add(*acceleration)
            .ok_or(TimelineError::ValidationFailed)?;
        state.position = state
            .position
            .checked_add(state.velocity)
            .ok_or(TimelineError::ValidationFailed)?;
        Ok(())
    }
}

#[test]
fn delayed_server_snapshot_reconciles_unacknowledged_inputs_and_prediction_continues() {
    // Also cover a genesis correction and a fully acknowledged current tick.
    for acknowledged in [0_u64, 2, 6] {
        let scope = TimelineScope {
            tenant: TimelineTenantId::from_digest(Digest([1; 32])),
            timeline: TimelineId::from_digest(Digest([2; 32])),
            model: TimelineModelId::from_digest(Digest([3; 32])),
            schema: TimelineSchemaId::from_digest(Digest([4; 32])),
        };
        let genesis = TimelineHead::new(
            scope,
            TimelineBranchId::ZERO,
            TimelineStep::ZERO,
            TimelineRevision::ZERO,
            None,
        )
        .unwrap();
        let mut client = LiveTimeline::<Player, (), 1, 0>::new(
            genesis,
            Player::default(),
            EffectLimits {
                max_intents: 0,
                max_payload_bytes: 0,
            },
        );
        let valid = |_state: &Player, _head: TimelineHead| Ok(());
        let mut retained = Vec::new();
        for tick in 1_u64..=6 {
            let record = client
                .apply_input(
                    &Movement,
                    TimelineInputId::from_digest(Digest::of(&tick.to_le_bytes())),
                    &1,
                    &valid,
                )
                .unwrap();
            retained.push(CapturedInput::new(record, 1_u64).unwrap());
        }
        let retained: [CapturedInput<u64>; 6] = retained.try_into().ok().unwrap();
        let predicted = (client.read().0, *client.read().1);
        assert_eq!(
            predicted.1,
            Player {
                position: 21,
                velocity: 6
            }
        );

        // Independent server formula: at the acknowledged tick the authoritative
        // position differs by 17. Its complete post-tick state includes velocity.
        let server_snapshot = Player {
            position: acknowledged * (acknowledged + 1) / 2 + 17,
            velocity: acknowledged,
        };
        let snapshot = CapturedSnapshot::capture(
            predicted.0,
            TimelineStep::new(acknowledged),
            TimelineSnapshotId::from_digest(server_snapshot.digest()),
            server_snapshot,
        )
        .unwrap();
        // Application ACK handling selects only inputs AFTER the snapshot tick.
        let suffix: Vec<_> = retained
            .iter()
            .map(CapturedInput::record)
            .filter(|record| record.step.get() > acknowledged)
            .collect();
        let mut correction = client
            .begin_correction(
                CorrectionRequest {
                    correction: Correction::State(snapshot.anchor()),
                    inputs: &suffix,
                    observation: AuthorityObservation {
                        scope,
                        authority: TimelineAuthorityId::from_digest(Digest([7; 32])),
                        epoch: AuthorityEpoch::ZERO,
                        sequence: AuthoritySequence::new(1),
                        step: snapshot.anchor().step,
                        digest: snapshot.anchor().digest,
                    },
                    scratch: Player::default(),
                    limits: ReplayLimits {
                        max_retained_inputs: 6,
                        max_replay_steps: 6,
                    },
                },
                &snapshot,
            )
            .unwrap();
        let mut calls = 0;
        loop {
            let phase = correction.advance(&Movement, &retained, 1).unwrap();
            calls += 1;
            // Rendering can keep the old coherent prediction until publication.
            assert_eq!(correction.read(), (predicted.0, &predicted.1));
            if phase != ReplayPhase::Replaying {
                break;
            }
        }
        assert_eq!(calls, (6 - acknowledged).max(1));
        correction.publish(&valid).unwrap();
        assert_eq!(client.read().0.step(), TimelineStep::new(6));
        assert_ne!(client.read().0.branch(), predicted.0.branch());
        // Closed-form server result; replay must not apply acknowledged inputs twice.
        assert_eq!(
            *client.read().1,
            Player {
                position: 38,
                velocity: 6
            }
        );
        let next = client
            .apply_input(
                &Movement,
                TimelineInputId::from_digest(Digest([8; 32])),
                &1,
                &valid,
            )
            .unwrap();
        assert_eq!(next.step, TimelineStep::new(7));
        assert_eq!(next.branch, client.read().0.branch());
        assert_eq!(
            *client.read().1,
            Player {
                position: 45,
                velocity: 7
            }
        );
    }
}
