//! Admit identifier text directly from Serde without an intermediate String.
use super::implementation::DomainError;
use core::{fmt, marker::PhantomData, str::FromStr};
use serde::{
    Deserializer,
    de::{Error, Unexpected, Visitor},
};

struct IdentifierVisitor<T>(PhantomData<T>);
impl<'de, T> Visitor<'de> for IdentifierVisitor<T>
where
    T: FromStr<Err = DomainError> + TryFrom<String, Error = DomainError>,
{
    type Value = T;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string")
    }
    fn visit_str<E: Error>(self, value: &str) -> Result<T, E> {
        value.parse().map_err(E::custom)
    }
    fn visit_string<E: Error>(self, value: String) -> Result<T, E> {
        T::try_from(value).map_err(E::custom)
    }
    fn visit_bytes<E: Error>(self, value: &[u8]) -> Result<T, E> {
        match core::str::from_utf8(value) {
            Ok(text) => self.visit_str(text),
            Err(_utf8) => Err(E::invalid_value(Unexpected::Bytes(value), &self)),
        }
    }
    fn visit_byte_buf<E: Error>(self, value: Vec<u8>) -> Result<T, E> {
        match String::from_utf8(value) {
            Ok(text) => self.visit_string(text),
            Err(error) => Err(E::invalid_value(
                Unexpected::Bytes(&error.into_bytes()),
                &self,
            )),
        }
    }
}

pub(super) fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr<Err = DomainError> + TryFrom<String, Error = DomainError>,
{
    deserializer.deserialize_string(IdentifierVisitor(PhantomData))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::implementation::ActionId;
    use super::*;
    use serde::{
        Deserialize,
        de::value::{
            BorrowedBytesDeserializer, BorrowedStrDeserializer, BytesDeserializer,
            Error as ValueError, StrDeserializer, StringDeserializer,
        },
    };

    #[test]
    fn visitor_preserves_borrowed_owned_and_byte_string_admission() {
        let expected: ActionId = "act_value".parse().unwrap();
        assert_eq!(
            ActionId::deserialize(BorrowedStrDeserializer::<ValueError>::new("act_value")).unwrap(),
            expected
        );
        assert_eq!(
            ActionId::deserialize(StrDeserializer::<ValueError>::new("act_value")).unwrap(),
            expected
        );
        assert_eq!(
            ActionId::deserialize(StringDeserializer::<ValueError>::new(String::from(
                "act_value"
            )))
            .unwrap(),
            expected
        );
        assert_eq!(
            ActionId::deserialize(BytesDeserializer::<ValueError>::new(b"act_value")).unwrap(),
            expected
        );
        assert_eq!(
            ActionId::deserialize(BorrowedBytesDeserializer::<ValueError>::new(b"act_value"))
                .unwrap(),
            expected
        );
        let visitor = IdentifierVisitor::<ActionId>(PhantomData);
        assert_eq!(
            visitor
                .visit_byte_buf::<ValueError>(b"act_value".to_vec())
                .unwrap(),
            expected
        );
    }

    #[test]
    fn invalid_utf8_and_noncanonical_text_stay_rejected() {
        for bytes in [b"prc_wrong".as_slice(), &[0xff], b"act_\0bad".as_slice()] {
            assert!(ActionId::deserialize(BytesDeserializer::<ValueError>::new(bytes)).is_err());
            assert!(
                IdentifierVisitor::<ActionId>(PhantomData)
                    .visit_byte_buf::<ValueError>(bytes.to_vec())
                    .is_err()
            );
        }
        for json in ["null", "true", "1", "[]", "{}", "\"prc_wrong\"", "\"act_\""] {
            assert!(serde_json::from_str::<ActionId>(json).is_err());
        }
    }
}
