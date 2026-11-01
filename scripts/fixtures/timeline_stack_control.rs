//! Positive control for sampled stack accounting, not a timeline implementation.
#[inline(never)]
fn control() {
    let mut bytes = [0_u8; 65_536];
    for (index, value) in bytes.iter_mut().enumerate() {
        *value = (index & 255) as u8;
    }
    std::hint::black_box(&mut bytes);
}
fn main() {
    control();
}
