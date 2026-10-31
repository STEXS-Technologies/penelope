use super::{
    legacy,
    ports::{Authority, Clock, Error, run},
};
use penelope_domain::*;
use penelope_executor::services::drain_timers;
use penelope_ports::{inbox::PendingInbox, payloads::ProcessPayloadStore, *};
use penelope_sqlite::{SqliteLimits, SqliteProcessStore};
use std::{
    hint::black_box,
    num::{NonZeroU16, NonZeroU64},
    path::Path,
    sync::Arc,
    time::Instant,
};

fn open(path: &Path) -> Result<SqliteProcessStore, Error> {
    Ok(SqliteProcessStore::open(
        path,
        SqliteLimits::default(),
        Arc::new(Clock),
        30_000,
        16,
    )?)
}
// Same synthetic business kernel as application_workloads, in both paths.
#[inline(never)]
fn kernel(mut value: u64, work: u32) -> u64 {
    for _ in 0..work {
        value = value
            .wrapping_mul(6_364_136_223_846_793_005)
            .rotate_left(17)
            .wrapping_add(1_442_695_040_888_963_407);
    }
    value
}
fn seed(input: &ProcessInput) -> Result<u64, Error> {
    Ok(u64::from_le_bytes(
        input
            .payload_digest
            .0
            .get(..8)
            .ok_or("short digest")?
            .try_into()?,
    ))
}
fn direct(inputs: &[(ProcessInput, Vec<u8>)], work: u32) -> Result<(u128, u64), Error> {
    let began = Instant::now();
    let mut checksum = 0_u64;
    for (input, _) in inputs {
        checksum =
            checksum.rotate_left(1) ^ black_box(kernel(black_box(seed(input)?), black_box(work)));
    }
    Ok((began.elapsed().as_nanos(), checksum))
}
fn pending(store: &SqliteProcessStore, scope: &ProcessScope) -> Result<Vec<ProcessInput>, Error> {
    run(store.pending_inputs(
        &scope.tenant_id,
        &scope.process_id,
        NonZeroU16::new(64).ok_or("pending bound")?,
    ))
}

pub fn measure(
    batch: u16,
    width: usize,
    work: u32,
    reopen: bool,
    round: u32,
) -> Result<serde_json::Value, Error> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("timer-workload.db");
    let mut store = open(&path)?;
    let suffix = "x".repeat(width.checked_sub(4).ok_or("short scope")?);
    let scope = ProcessScope::new(
        format!("tnt_{suffix}").parse()?,
        format!("prc_{suffix}").parse()?,
        format!("def_{suffix}").parse()?,
        format!("dfv_{suffix}").parse()?,
        ContentDigest([3; 32]),
    );
    let mut timers = Vec::new();
    let mut expected = Vec::new();
    for index in 0..batch {
        let timer = TimerSchedule {
            action: ProcessAction::new(
                scope.clone(),
                format!(
                    "act_{}{:04}",
                    "x".repeat(width.checked_sub(8).ok_or("short action")?),
                    index
                )
                .parse()?,
                format!("stp_{index:03}").parse()?,
                0,
                ProcessActionKind::Timer,
                ContentDigest([4; 32]),
            ),
            due_at: LogicalTime(100),
        };
        expected.push(legacy::timer_input(&TimerLease {
            timer: timer.clone(),
            owner: "pri_expected".parse()?,
            token: OutboxLeaseToken::new(NonZeroU64::MIN),
            lease_expires_at: LogicalTime(u64::MAX),
        })?);
        timers.push(timer);
    }
    expected.sort_by(|a, b| a.0.input_id.as_str().cmp(b.0.input_id.as_str()));
    let initial = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            "out_start".parse()?,
            CausationId::Input("inp_start".parse()?),
            OutcomeActor::System,
            LogicalTime(100),
            ProcessOutcomeKind::Started,
            ContentDigest([6; 32]),
        ),
    );
    let start = AtomicProcessCommit::new(
        0,
        Some(ProcessInput::new(
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            "inp_start".parse()?,
            ProcessInputKind::CanonicalEvent,
            ContentDigest([5; 32]),
        )),
        vec![initial.clone()],
        timers.iter().map(|timer| timer.action.clone()).collect(),
    )?;
    let request = TimerClaimRequest::new(
        scope.clone(),
        "pri_worker".parse()?,
        LogicalTime(u64::MAX),
        NonZeroU16::new(batch).ok_or("batch bound")?,
    )?;
    let direct_first = round & 1 == 0;
    let mut direct_result = if direct_first {
        Some(direct(&expected, work)?)
    } else {
        None
    };
    let schedule_began = Instant::now();
    run(store.commit(&start))?.validate_for(&start)?;
    for timer in &timers {
        run(store.schedule(timer))?;
    }
    let schedule_ns = schedule_began.elapsed().as_nanos();
    let delivery_began = Instant::now();
    let report = drain(&store, &request)?;
    let delivery_ns = delivery_began.elapsed().as_nanos();
    if report.claimed != batch || report.acknowledged != batch || report.duplicate_inputs != 0 {
        return Err("incomplete timer delivery".into());
    }
    let reopen_ns = if reopen {
        let reopen_began = Instant::now();
        drop(store);
        store = open(&path)?;
        reopen_began.elapsed().as_nanos()
    } else {
        0
    };
    let consume_began = Instant::now();
    let mut checksum = 0_u64;
    let mut commits = Vec::new();
    let mut outcomes = vec![initial];
    loop {
        let discovered = pending(&store, &scope)?;
        if discovered.is_empty() {
            break;
        }
        for input in discovered {
            let index = commits.len();
            let (wanted, _) = expected.get(index).ok_or("unexpected pending input")?;
            if &input != wanted {
                return Err("timer identity differs from legacy".into());
            }
            let value = black_box(kernel(black_box(seed(&input)?), black_box(work)));
            checksum = checksum.rotate_left(1) ^ value;
            let result = value.to_le_bytes();
            let digest = ContentDigest::sha256(&result);
            if run(store.put_payload(&scope, digest, &result))? != digest {
                return Err("result payload receipt differs".into());
            }
            let sequence = u64::try_from(index)?
                .checked_add(1)
                .ok_or("sequence overflow")?;
            let outcome = ProcessOutcome::new(
                scope.clone(),
                sequence,
                ProcessOutcomeFact::new(
                    format!("out_timer_{index:03}").parse()?,
                    CausationId::Input(input.input_id.clone()),
                    OutcomeActor::System,
                    LogicalTime(100),
                    ProcessOutcomeKind::TimerFired,
                    digest,
                ),
            );
            let commit =
                AtomicProcessCommit::new(sequence, Some(input), vec![outcome.clone()], vec![])?;
            let receipt = run(store.commit(&commit))?;
            receipt.validate_for(&commit)?;
            if !receipt.duplicate_input {
                return Err("timer input was not previously accepted".into());
            }
            outcomes.push(outcome);
            commits.push(commit);
        }
    }
    let consume_ns = consume_began.elapsed().as_nanos();
    if !direct_first {
        direct_result = Some(direct(&expected, work)?);
    }
    let (direct_ns, direct_checksum) = direct_result.ok_or("missing direct result")?;
    if checksum != direct_checksum || commits.len() != usize::from(batch) {
        return Err("business computation differs".into());
    }
    // All verification below is outside timing. Verify both bytes and public state
    // after reopening, including idempotent commit retries and empty rediscovery.
    drop(store);
    store = open(&path)?;
    for (input, payload) in &expected {
        if run(store.read_payload(&scope, input.payload_digest))? != *payload {
            return Err("timer payload did not survive reopen".into());
        }
    }
    let mut durable_checksum = 0_u64;
    for (commit, outcome) in commits.iter().zip(outcomes.iter().skip(1)) {
        let receipt = run(store.commit(commit))?;
        receipt.validate_for(commit)?;
        if !receipt.duplicate_input {
            return Err("commit retry was not duplicate".into());
        }
        let bytes = run(store.read_payload(&scope, outcome.payload_digest))?;
        let value = u64::from_le_bytes(bytes.as_slice().try_into()?);
        let input = commit.input.as_ref().ok_or("missing committed input")?;
        if value != kernel(seed(input)?, work) {
            return Err("durable business result differs".into());
        }
        durable_checksum = durable_checksum.rotate_left(1) ^ value;
    }
    let replay = run(store.read_outcomes(&OutcomeReplayRequest::new(
        scope.clone(),
        0,
        NonZeroU16::new(512).ok_or("replay bound")?,
    )?))?;
    replay.validate()?;
    if replay.outcomes != outcomes
        || replay.next_sequence.is_some()
        || durable_checksum != checksum
        || !pending(&store, &scope)?.is_empty()
        || drain(&store, &request)?.claimed != 0
    {
        return Err("durable timer state differs after reopen".into());
    }
    Ok(
        serde_json::json!({"schema":1,"batch":batch,"width":width,"work":work,"reopen":reopen,"round":round,"direct_first":direct_first,"direct_ns":direct_ns,"schedule_ns":schedule_ns,"delivery_ns":delivery_ns,"reopen_ns":reopen_ns,"consume_ns":consume_ns,"managed_ns":schedule_ns+delivery_ns+reopen_ns+consume_ns,"checksum":checksum,"verified":true,"consumed":commits.len(),"outcomes":outcomes.len()}),
    )
}
fn drain(
    store: &SqliteProcessStore,
    request: &TimerClaimRequest,
) -> Result<penelope_executor::services::TimerDrainReport, Error> {
    run(async {
        drain_timers(&Authority, store, store, request, &Clock)
            .await
            .map_err(|error| error.source)
    })
}
