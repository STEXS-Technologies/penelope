//! Reused serialization preserves JSON bytes, size boundaries and error recovery.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use super::*;
use serde::ser::{Error as _, SerializeSeq as _};

#[test]
fn reused_encoding_matches_json_at_exact_size_boundaries() {
    let mut buffer = Vec::new();
    for value in [
        serde_json::json!({"long": "a\n\"é".repeat(300), "values": [true, null, 123]}),
        serde_json::json!("x"),
        serde_json::json!({"nested": [{"empty": []}], "escaped": "\\\r\t"}),
        serde_json::json!(false),
    ] {
        let expected = serde_json::to_vec(&value).unwrap();
        let size = u32::try_from(expected.len()).unwrap();
        assert_eq!(
            encode_into(&value, size - 1, &mut buffer),
            Err(PortError::QuotaExceeded)
        );
        assert_eq!(encode(&value, size - 1), Err(PortError::QuotaExceeded));
        for limit in [size, size + 1, u32::MAX] {
            encode_into(&value, limit, &mut buffer).unwrap();
            assert_eq!(buffer, expected);
            assert_eq!(encode(&value, limit), Ok(expected.clone()));
        }
    }
}

struct PartialFailure;
impl Serialize for PartialFailure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(2))?;
        sequence.serialize_element("partial")?;
        Err(S::Error::custom("injected serialization failure"))
    }
}
#[test]
fn failed_or_oversized_encoding_cannot_contaminate_a_later_value() {
    let mut buffer = vec![9; 4096];
    assert_eq!(
        encode_into(&PartialFailure, u32::MAX, &mut buffer),
        Err(PortError::Invariant)
    );
    assert_eq!(encode(&PartialFailure, u32::MAX), Err(PortError::Invariant));
    assert!(!buffer.is_empty());
    encode_into(&"ok", 4, &mut buffer).unwrap();
    assert_eq!(buffer, b"\"ok\"");
    assert_eq!(
        encode_into(&"large".repeat(1024), 4, &mut buffer),
        Err(PortError::QuotaExceeded)
    );
    encode_into(&"ok", 4, &mut buffer).unwrap();
    assert_eq!(buffer, b"\"ok\"");
    assert_eq!(
        encode_into(&(), 0, &mut buffer),
        Err(PortError::QuotaExceeded)
    );
    encode_into(&(), 4, &mut buffer).unwrap();
    assert_eq!(buffer, b"null");
}
