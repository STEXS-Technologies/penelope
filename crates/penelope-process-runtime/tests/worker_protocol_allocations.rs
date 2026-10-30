//! Isolated executable measuring protocol allocations after fixture setup.
#![allow(clippy::unwrap_used)]
#[path = "support/protocol_fixture.rs"]
mod fixture;
use penelope_process_runtime::worker::{read_request, write_proposal};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Allocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
// SAFETY: pointers/layouts pass unchanged to System; the counter cannot allocate.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's valid layout is forwarded unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the pointer and its original layout are forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}
fn count<T>(operation: impl FnOnce() -> T) -> usize {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    drop(black_box(operation()));
    ALLOCATIONS.load(Ordering::Relaxed).saturating_sub(before)
}
fn main() {
    let metadata = fixture::metadata();
    let mut frame = [0; 61];
    for checkpoint in [false, true] {
        assert_eq!(
            count(|| write_proposal(
                &mut Cursor::new(&mut frame[..]),
                black_box(&metadata),
                checkpoint,
                1,
                black_box(&[0; 8])
            )
            .unwrap()),
            0
        );
    }
    let packet = fixture::packet(&metadata).unwrap();
    assert_eq!(
        count(|| read_request(&mut Cursor::new(black_box(&packet)), 64, 64).unwrap()),
        2
    );
    let mut invalid = metadata;
    invalid.definition.budget.max_input_bytes = 3;
    invalid.scope.definition = invalid.definition.fingerprint();
    let rejected_packet = fixture::packet(&invalid).unwrap();
    assert_eq!(
        count(
            || read_request(&mut Cursor::new(black_box(&rejected_packet)), 64, 64)
                .err()
                .unwrap()
        ),
        1
    );
    println!("Protocol allocations: proposal=0, valid request=2, pinned-bound rejection=1");
}
