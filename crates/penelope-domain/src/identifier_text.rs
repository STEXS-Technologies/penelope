//! Private immutable text: short identifiers clone without heap or shared state.

use core::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};

// With a boxed long-string fallback this fits in the same 24 bytes as String
// on the measured 64-bit host. Keep the threshold independent of input validity.
const INLINE_CAPACITY: usize = core::mem::size_of::<String>().saturating_sub(2);

#[derive(Clone)]
pub(super) enum IdentifierText {
    Inline {
        bytes: [u8; INLINE_CAPACITY],
        length: u8,
    },
    Heap(Box<str>),
}

impl IdentifierText {
    #[inline]
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Inline { bytes, length } => bytes.get(..usize::from(*length)).unwrap_or_default(),
            Self::Heap(text) => text.as_bytes(),
        }
    }

    #[inline]
    pub(super) fn as_str(&self) -> &str {
        match self {
            Self::Inline { bytes, length } => {
                // Constructors copy complete UTF-8 strings and bound the length.
                // Safe checked access/decoding retains the crate's no-unsafe rule.
                core::str::from_utf8(bytes.get(..usize::from(*length)).unwrap_or_default())
                    .unwrap_or_default()
            }
            Self::Heap(text) => text,
        }
    }

    fn inline(value: &str) -> Option<Self> {
        let mut bytes = [0; INLINE_CAPACITY];
        bytes
            .get_mut(..value.len())?
            .copy_from_slice(value.as_bytes());
        Some(Self::Inline {
            bytes,
            length: u8::try_from(value.len()).ok()?,
        })
    }
}

impl From<&str> for IdentifierText {
    fn from(value: &str) -> Self {
        Self::inline(value).unwrap_or_else(|| Self::Heap(Box::from(value)))
    }
}

impl From<String> for IdentifierText {
    fn from(value: String) -> Self {
        Self::inline(&value).unwrap_or_else(|| Self::Heap(value.into_boxed_str()))
    }
}

impl fmt::Debug for IdentifierText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), formatter)
    }
}

impl PartialEq for IdentifierText {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl Eq for IdentifierText {}
impl PartialOrd for IdentifierText {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for IdentifierText {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}
impl Hash for IdentifierText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::hash_map::DefaultHasher;

    fn hash(value: &impl Hash) -> u64 {
        let mut state = DefaultHasher::new();
        value.hash(&mut state);
        state.finish()
    }

    proptest! {
        #[test]
        fn storage_preserves_string_semantics(first in any::<String>(), second in any::<String>()) {
            let borrowed = IdentifierText::from(first.as_str());
            let owned = IdentifierText::from(first.clone());
            let other = IdentifierText::from(second.as_str());
            prop_assert_eq!(borrowed.as_str(), first.as_str());
            prop_assert_eq!(owned.as_str(), first.as_str());
            let cloned = borrowed.clone();
            prop_assert_eq!(cloned.as_str(), first.as_str());
            prop_assert_eq!(borrowed.cmp(&other), first.cmp(&second));
            prop_assert_eq!(borrowed == other, first == second);
            prop_assert_eq!(hash(&borrowed), hash(&first));
            prop_assert_eq!(format!("{borrowed:?}"), format!("{first:?}"));
        }
    }

    #[test]
    fn boundary_lengths_and_multibyte_text_round_trip() {
        for length in 0..=128 {
            let text = "a".repeat(length);
            let value = IdentifierText::from(text.as_str());
            assert_eq!(value.as_str(), text);
            assert_eq!(value.clone().as_str(), text);
        }
        for text in ["é", "αβγδεζηθικλ", "αβγδεζηθικλμ", "👩🏽‍💻"]
        {
            assert_eq!(IdentifierText::from(text).as_str(), text);
        }
    }
}
