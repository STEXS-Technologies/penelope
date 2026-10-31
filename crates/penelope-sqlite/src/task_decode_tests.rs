//! Differential validation against the public-read decoder and core Serde format.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use super::*;
fn record(byte: u8) -> TaskRecord {
    TaskRecord {
        version: 1,
        scope: TaskScope {
            tenant: TaskTenantId::from_digest(Digest([byte; 32])),
            task: TaskId::from_digest(Digest([byte ^ 0x55; 32])),
            definition: TaskDefinitionFingerprint::from_digest(Digest([byte; 32])),
            input: TaskInputDigest::from_digest(Digest([byte; 32])),
        },
        sequence: TaskSequence::ZERO,
        generation: TaskGeneration::new(1),
        tick: TaskTick::new(1),
        event: TaskExecutionEvent::Started,
    }
}
fn compare(bytes: &[u8], checksum: &[u8], record: TaskRecord) -> bool {
    let expected = crate::tasks::decode(bytes, checksum, record.scope, record.sequence);
    let actual = Decoder::new(record.scope).decode(bytes, checksum, record.sequence);
    assert_eq!(actual, expected, "bytes={bytes:?}");
    actual.is_ok()
}
#[test]
fn replay_scope_decoder_matches_core_json_forms_and_mutated_records() {
    let mut directed = 0;
    for byte in [0, 1, 9, 10, 99, 100, 255] {
        let record = record(byte);
        let original = serde_json::to_vec(&record).unwrap();
        let original_text = std::str::from_utf8(&original).unwrap();
        let scope_text = serde_json::to_string(&record.scope).unwrap();
        let value = serde_json::to_value(record).unwrap();
        let mut wrong_canonical = record;
        wrong_canonical.scope = self::record(byte.wrapping_add(1)).scope;
        let shortened_length = original.len() - scope_text.len() + 4;
        let mut at_scratch_limit = original.clone();
        at_scratch_limit.resize(original.len() + 4096 - shortened_length, b' ');
        let mut over_scratch_limit = at_scratch_limit.clone();
        over_scratch_limit.push(b' ');
        let mut large = original.clone();
        large.resize(original.len() + 8192, b' ');
        let root_prefix = original_text.strip_suffix('}').unwrap();
        let mut cases = vec![
            (
                "duplicate-scope-tail",
                format!("{root_prefix},\"scope\":{scope_text}}}").into_bytes(),
                false,
            ),
            (
                "duplicate-version-tail",
                format!("{root_prefix},\"version\":1}}").into_bytes(),
                false,
            ),
            (
                "unknown-tail",
                format!("{root_prefix},\"extra\":1}}").into_bytes(),
                false,
            ),
            ("canonical", original.clone(), true),
            ("scratch-limit", at_scratch_limit, true),
            ("scratch-over-limit", over_scratch_limit, true),
            ("large-fallback", large, true),
            (
                "wrong-canonical-scope",
                serde_json::to_vec(&wrong_canonical).unwrap(),
                false,
            ),
            ("pretty", serde_json::to_vec_pretty(&record).unwrap(), true),
            ("reordered", serde_json::to_vec(&value).unwrap(), true),
            (
                "root-sequence",
                serde_json::to_vec(&serde_json::json!([
                    record.version,
                    record.scope,
                    record.sequence,
                    record.generation,
                    record.tick,
                    record.event
                ]))
                .unwrap(),
                true,
            ),
            (
                "escaped-keys",
                original_text
                    .replace("\"scope\"", r#""\u0073cope""#)
                    .replace("\"tenant\"", r#""\u0074enant""#)
                    .into_bytes(),
                true,
            ),
            (
                "duplicate-root",
                format!("{{\"scope\":{scope_text},{}", &original_text[1..]).into_bytes(),
                false,
            ),
            ("trailing", [original.as_slice(), b"{}"].concat(), false),
            ("malformed", b"{".to_vec(), false),
        ];
        let duplicate_scope = format!(
            "{{\"tenant\":{},{}",
            serde_json::to_string(&record.scope.tenant).unwrap(),
            &scope_text[1..]
        );
        cases.push((
            "duplicate-scope",
            original_text
                .replacen(&scope_text, &duplicate_scope, 1)
                .into_bytes(),
            false,
        ));
        let deep_scope = format!("{}0{}", "[".repeat(140), "]".repeat(140));
        cases.push((
            "deep-scope",
            original_text
                .replacen(&scope_text, &deep_scope, 1)
                .into_bytes(),
            false,
        ));
        for (name, replacement, valid) in [
            (
                "scope-sequence",
                serde_json::json!([
                    record.scope.tenant,
                    record.scope.task,
                    record.scope.definition,
                    record.scope.input
                ]),
                true,
            ),
            ("null-scope", serde_json::Value::Null, false),
            ("number-scope", serde_json::json!(1), false),
            (
                "wrong-scope",
                serde_json::to_value(self::record(byte.wrapping_add(1)).scope).unwrap(),
                false,
            ),
        ] {
            let mut changed = value.clone();
            changed["scope"] = replacement;
            cases.push((name, serde_json::to_vec(&changed).unwrap(), valid));
        }
        for (name, field, replacement) in [
            ("version", "version", serde_json::json!(2)),
            ("sequence", "sequence", serde_json::json!(1)),
            ("generation-type", "generation", serde_json::json!("one")),
            ("unknown-root", "extra", serde_json::json!(1)),
        ] {
            let mut changed = value.clone();
            changed[field] = replacement;
            cases.push((name, serde_json::to_vec(&changed).unwrap(), false));
        }
        let mut changed = value.clone();
        changed["scope"]["extra"] = serde_json::json!(1);
        cases.push((
            "unknown-scope",
            serde_json::to_vec(&changed).unwrap(),
            false,
        ));
        let mut wrong_digest = value;
        wrong_digest["scope"]["tenant"][0] = serde_json::json!("one");
        cases.push((
            "digest-type",
            serde_json::to_vec(&wrong_digest).unwrap(),
            false,
        ));
        for (name, bytes, valid) in cases {
            let checksum = Digest::of(&bytes);
            assert_eq!(
                compare(&bytes, &checksum.0, record),
                valid,
                "case={name} byte={byte}"
            );
            let mut bad = checksum.0;
            bad[0] ^= 1;
            assert!(!compare(&bytes, &bad, record));
            directed += 1;
        }
    }
    let record = record(227);
    let original = serde_json::to_vec(&record).unwrap();
    let mut random = 0x86d3_7b92_a154_c08f_u64;
    for sample in 0..10000 {
        random = random
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let index = usize::try_from(random % u64::try_from(original.len()).unwrap()).unwrap();
        let mut bytes = original.clone();
        match sample & 7 {
            0 => bytes[index] ^= u8::try_from((random >> 32) & 255).unwrap(),
            1 => bytes.insert(index, u8::try_from((random >> 32) & 255).unwrap()),
            2 => {
                bytes.remove(index);
            }
            3 => bytes[index] = 255,
            4 => bytes.push(u8::try_from((random >> 32) & 255).unwrap()),
            5 => bytes.truncate(index),
            6 => bytes.swap(index, (index + 1) % original.len()),
            _ => bytes[index] = b' ',
        }
        let checksum = Digest::of(&bytes);
        compare(&bytes, &checksum.0, record);
    }
    println!("directed={directed} checksum_controls={directed} mutations=10000");
}
#[test]
fn scope_encoding_bound_covers_every_digest_byte_and_fails_on_short_buffers() {
    for byte in 0..=255 {
        let record = record(byte);
        let expected = serde_json::to_vec(&record.scope).unwrap();
        let mut buffer = [0; 1024];
        let length = encode_scope(record.scope, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], expected);
        let bytes = serde_json::to_vec(&record).unwrap();
        assert!(compare(&bytes, &Digest::of(&bytes).0, record));
        let mut short = vec![0; length - 1];
        assert_eq!(
            encode_scope(record.scope, &mut short),
            Err(TaskPortError::Corrupt)
        );
    }
    println!("scope_byte_boundaries=256");
}

#[test]
fn scratch_reuse_preserves_shorter_records_and_owned_results() {
    let record = record(227);
    let original = serde_json::to_vec(&record).unwrap();
    let pretty = serde_json::to_vec_pretty(&record).unwrap();
    let mut padded = original.clone();
    padded.resize(4096, b' ');
    let mut decoder = Decoder::new(record.scope);
    let held = decoder
        .decode(&padded, &Digest::of(&padded).0, record.sequence)
        .unwrap();
    for _ in 0..16 {
        for bytes in [&original, &pretty, &padded] {
            assert_eq!(
                decoder.decode(bytes, &Digest::of(bytes).0, record.sequence),
                Ok(record)
            );
            assert_eq!(held, record);
        }
    }
}
