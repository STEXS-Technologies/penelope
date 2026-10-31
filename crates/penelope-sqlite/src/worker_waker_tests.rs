//! Adversarial RawWaker callbacks exercise registration races and unwinding.
//! Unsafe code is limited to the standard Arc-backed RawWaker ownership bridge.
#![allow(unsafe_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use std::sync::{
    Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
struct Callback {
    completion: Weak<Mutex<Completion<u32>>>,
    complete_on_clone: bool,
    panic_on_clone: AtomicBool,
    panic_on_drop: AtomicBool,
    clones: AtomicUsize,
}
impl Callback {
    fn waker(self: &Arc<Self>) -> Waker {
        let raw = std::task::RawWaker::new(Arc::into_raw(self.clone()).cast(), &VTABLE);
        // SAFETY: every raw pointer owns one Arc reference; VTABLE preserves it.
        unsafe { Waker::from_raw(raw) }
    }
}
unsafe fn clone_callback(data: *const ()) -> std::task::RawWaker {
    // SAFETY: the source waker owns an Arc and remains borrowed for this call.
    let callback = unsafe { &*data.cast::<Callback>() };
    callback.clones.fetch_add(1, Ordering::SeqCst);
    let completion = callback.completion.upgrade().unwrap();
    let mut state = completion
        .try_lock()
        .expect("clone callback held completion lock");
    if callback.complete_on_clone {
        state.result = Some(Ok(7));
    }
    drop(state);
    assert!(
        !callback.panic_on_clone.swap(false, Ordering::SeqCst),
        "injected clone panic"
    );
    // SAFETY: add exactly one owning reference for the returned cloned waker.
    unsafe { Arc::increment_strong_count(data.cast::<Callback>()) };
    std::task::RawWaker::new(data, &VTABLE)
}
unsafe fn drop_callback(data: *const ()) {
    // SAFETY: drop/wake consumes exactly one owning raw Arc reference.
    let callback = unsafe { Arc::from_raw(data.cast::<Callback>()) };
    let panic = callback.panic_on_drop.swap(false, Ordering::SeqCst);
    drop(callback);
    assert!(!panic, "injected raw waker drop panic");
}
unsafe fn wake_by_ref_callback(_data: *const ()) {}
static VTABLE: std::task::RawWakerVTable = std::task::RawWakerVTable::new(
    clone_callback,
    drop_callback,
    wake_by_ref_callback,
    drop_callback,
);
fn fixture(
    complete_on_clone: bool,
    panic_on_clone: bool,
    panic_on_drop: bool,
) -> (Response<u32>, Arc<Callback>) {
    let completion = Arc::new(Mutex::new(Completion {
        result: None,
        waker: None,
    }));
    let callback = Arc::new(Callback {
        completion: Arc::downgrade(&completion),
        complete_on_clone,
        panic_on_clone: AtomicBool::new(panic_on_clone),
        panic_on_drop: AtomicBool::new(panic_on_drop),
        clones: AtomicUsize::new(0),
    });
    (Response { completion }, callback)
}
#[test]
fn cloning_executor_waker_can_complete_response_without_lock_or_lost_result() {
    let (mut response, callback) = fixture(true, false, false);
    let waker = callback.waker();
    assert_eq!(
        Pin::new(&mut response).poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Ok(7))
    );
    assert_eq!(callback.clones.load(Ordering::SeqCst), 1);
}
#[test]
fn panicking_clone_or_temporary_waker_drop_preserves_unpoisoned_result() {
    for panic_on_clone in [false, true] {
        let (mut response, callback) = fixture(true, panic_on_clone, !panic_on_clone);
        let waker = callback.waker();
        let polled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Pin::new(&mut response).poll(&mut Context::from_waker(&waker))
        }));
        assert!(polled.is_err());
        assert_eq!(response.completion.lock().unwrap().result, Some(Ok(7)));
        assert_eq!(
            Pin::new(&mut response).poll(&mut Context::from_waker(&waker)),
            Poll::Ready(Ok(7))
        );
    }
}
#[test]
fn repeated_pending_polls_reuse_registration_without_cloning_executor_waker() {
    let (mut response, callback) = fixture(false, false, false);
    let waker = callback.waker();
    for _ in 0..10_000 {
        assert!(
            Pin::new(&mut response)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
    }
    response.completion.lock().unwrap().result = Some(Ok(7));
    assert_eq!(
        Pin::new(&mut response).poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Ok(7))
    );
    assert_eq!(callback.clones.load(Ordering::SeqCst), 1);
}
