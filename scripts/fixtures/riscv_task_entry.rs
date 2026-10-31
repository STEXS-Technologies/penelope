// Freestanding reference ABI shims for the stack qualification binary.
#[unsafe(no_mangle)]
pub extern "C" fn _start(selector: u8) -> ! {
    qualify_task_path(selector);
    loop { core::hint::spin_loop(); }
}
#[panic_handler]
fn panic(_information: &core::panic::PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}
// SAFETY: compiler calls these C memory primitives with valid byte ranges.
// Volatile byte accesses avoid transforming their bodies back into intrinsics.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(destination: *mut core::ffi::c_void, source: *const core::ffi::c_void, count: usize) -> *mut core::ffi::c_void {
    let dst = destination.cast::<u8>();
    let src = source.cast::<u8>();
    for index in 0..count {
        unsafe { core::ptr::write_volatile(dst.add(index), core::ptr::read_volatile(src.add(index))); }
    }
    destination
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(destination: *mut core::ffi::c_void, value: i32, count: usize) -> *mut core::ffi::c_void {
    let dst = destination.cast::<u8>();
    for index in 0..count {
        unsafe { core::ptr::write_volatile(dst.add(index), value as u8); }
    }
    destination
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(left: *const core::ffi::c_void, right: *const core::ffi::c_void, count: usize) -> i32 {
    let left = left.cast::<u8>();
    let right = right.cast::<u8>();
    for index in 0..count {
        let a = unsafe { core::ptr::read_volatile(left.add(index)) };
        let b = unsafe { core::ptr::read_volatile(right.add(index)) };
        if a != b { return i32::from(a) - i32::from(b); }
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(destination: *mut core::ffi::c_void, source: *const core::ffi::c_void, count: usize) -> *mut core::ffi::c_void {
    let dst = destination.cast::<u8>();
    let src = source.cast::<u8>();
    if (dst as usize) < (src as usize) {
        unsafe { memcpy(destination, source, count) }
    } else {
        for index in (0..count).rev() {
            unsafe { core::ptr::write_volatile(dst.add(index), core::ptr::read_volatile(src.add(index))); }
        }
        destination
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(left: *const core::ffi::c_void, right: *const core::ffi::c_void, count: usize) -> i32 {
    unsafe { memcmp(left, right, count) }
}
