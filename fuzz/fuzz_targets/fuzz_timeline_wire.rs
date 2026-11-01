#![no_main]
use libfuzzer_sys::fuzz_target;
use penelope_core::timeline::{TIMELINE_RECORD_MAX_BYTES, TimelineRecord};

const FIXTURES: [&str; 7] = [
    include_str!("../../crates/penelope-core/tests/fixtures/timeline_v1/checkpoint-empty.hex"),
    include_str!("../../crates/penelope-core/tests/fixtures/timeline_v1/checkpoint-full.hex"),
    include_str!("../../crates/penelope-core/tests/fixtures/timeline_v1/input.hex"),
    include_str!("../../crates/penelope-core/tests/fixtures/timeline_v1/anchor.hex"),
    include_str!("../../crates/penelope-core/tests/fixtures/timeline_v1/fork-state.hex"),
    include_str!("../../crates/penelope-core/tests/fixtures/timeline_v1/fork-input.hex"),
    include_str!("../../crates/penelope-core/tests/fixtures/timeline_v1/intent.hex"),
];
fn canonical(bytes: &[u8]) {
    if let Ok(record) = TimelineRecord::decode(bytes) {
        let mut encoded = [0; TIMELINE_RECORD_MAX_BYTES];
        let len = record.encode(&mut encoded).unwrap();
        assert_eq!(len, record.encoded_len());
        assert_eq!(&encoded[..len], bytes);
        assert_eq!(TimelineRecord::decode(&encoded[..len]).unwrap(), record);
    }
}
fuzz_target!(|data: &[u8]| {
    if data.len() > 4096 {
        return;
    }
    canonical(data);
    // Reach every deep variant without requiring mutation to discover the 20-byte tag.
    for fixture in FIXTURES {
        let text: String = fixture.split_whitespace().collect();
        let mut bytes: Vec<_> = (0..text.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
            .collect();
        canonical(&bytes);
        for command in data.chunks(3) {
            let Some((&operation, rest)) = command.split_first() else {
                continue;
            };
            let Some((&offset, rest)) = rest.split_first() else {
                continue;
            };
            let Some(&value) = rest.first() else {
                continue;
            };
            match operation % 4 {
                0 => {
                    let index = (usize::from(offset) * 4) % bytes.len().max(1);
                    if let Some(byte) = bytes.get_mut(index) {
                        *byte ^= value;
                    }
                }
                1 => bytes.truncate(usize::from(offset) * 4),
                2 => {
                    if bytes.len() <= TIMELINE_RECORD_MAX_BYTES {
                        bytes.push(value);
                    }
                }
                _ => {
                    let index = usize::from(offset) % bytes.len().max(1);
                    if let Some(byte) = bytes.get_mut(index) {
                        *byte = value;
                    }
                }
            }
            canonical(&bytes);
        }
    }
});
