#![no_main]
use libfuzzer_sys::fuzz_target;
use penelope_ports::task::*;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 {
        return;
    }
    let reference = BlobRef {
        digest: Digest::of(data),
        bytes: data.len() as u32,
        schema: 1,
    };
    assert!(reference.verify(data, 1).is_ok());
    assert_eq!(reference.verify(data, 2), Err(TaskError::SchemaMismatch));
    let mut corrupt = data.to_vec();
    if let Some(first) = corrupt.first_mut() {
        *first ^= 1;
    } else {
        corrupt.push(0);
    }
    assert_eq!(reference.verify(&corrupt, 1), Err(TaskError::CorruptBlob));
    if let Ok(parsed) = serde_json::from_slice::<BlobRef>(data) {
        let _validation = parsed.verify(&corrupt, 1);
    }
});
