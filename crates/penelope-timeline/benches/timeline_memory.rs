//! Fixed resident capacity matrix without constructing large stack objects.
#![allow(clippy::arithmetic_side_effects)]
use penelope_timeline::{FixedHistory, LiveTimeline, ReplaySession, StagedEffects};
use std::mem::size_of;

fn profile<
    const I: usize,
    const A: usize,
    const B: usize,
    const S: usize,
    const E: usize,
    const W: usize,
>() {
    let history = size_of::<FixedHistory<[u8; W], u64, I, A, B>>();
    let live = size_of::<LiveTimeline<[u8; W], u64, S, E>>();
    let session = size_of::<ReplaySession<'_, [u8; W], u64, E>>();
    let effects = size_of::<StagedEffects<u64, E>>();
    let domain = history + live + S * session;
    println!(
        "memory_profile,{I},{A},{B},{S},{E},{W},8,{history},{live},{session},{effects},{domain}"
    );
}
fn horizons<const W: usize>() {
    // Snapshot intervals 1 and 8; all slots are resident even before admission.
    profile::<8, 9, 2, 2, 0, W>();
    profile::<8, 2, 2, 2, 0, W>();
    profile::<32, 33, 2, 2, 0, W>();
    profile::<32, 5, 2, 2, 0, W>();
    profile::<128, 129, 2, 2, 0, W>();
    profile::<128, 17, 2, 2, 0, W>();
    profile::<512, 513, 2, 2, 0, W>();
    profile::<512, 65, 2, 2, 0, W>();
    // Independent branch, active session and intent capacity changes.
    profile::<512, 65, 1, 0, 0, W>();
    profile::<512, 65, 4, 2, 0, W>();
    profile::<512, 65, 4, 4, 0, W>();
    profile::<512, 65, 4, 4, 8, W>();
    profile::<512, 65, 4, 4, 32, W>();
}
fn main() {
    println!(
        "kind,inputs,anchors,branches,sessions,intents,state_bytes,input_bytes,history_bytes,live_bytes,session_bytes,effect_buffer_bytes,domain_bytes"
    );
    horizons::<8>();
    horizons::<1024>();
    horizons::<4096>();
}
