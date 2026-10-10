//! Wire compatibility and arithmetic boundaries for task identity roles.
#![allow(clippy::unwrap_used)]
use penelope_core::task::*;

#[test]
fn scoped_identity_roles_preserve_exact_content_and_wire_values() {
    let digest = Digest([7; 32]);
    macro_rules! check {
        ($($ty:ty),+ $(,)?) => { $(
            let value = <$ty>::from_digest(digest);
            assert_eq!(value.digest(), digest);
            assert_eq!(core::mem::size_of::<$ty>(), 32);
            assert_eq!(serde_json::to_value(value).unwrap(), serde_json::to_value(digest).unwrap());
            assert_eq!(serde_json::from_value::<$ty>(serde_json::to_value(digest).unwrap()).unwrap(), value);
        )+ };
    }
    check!(
        TaskTenantId,
        TaskId,
        TaskDefinitionFingerprint,
        TaskInputDigest
    );
}

#[test]
fn task_counters_reject_wire_overflow_and_arithmetic_never_wraps() {
    macro_rules! check {
        ($ty:ty, $integer:ty) => {
            let maximum = <$ty>::new(<$integer>::MAX);
            assert_eq!(maximum.get(), <$integer>::MAX);
            assert_eq!(
                core::mem::size_of::<$ty>(),
                core::mem::size_of::<$integer>()
            );
            assert_eq!(maximum.checked_add(1), None);
            assert_eq!(maximum.saturating_add(1), maximum);
            assert_eq!(maximum.saturating_sub(1), <$ty>::new(<$integer>::MAX - 1));
            assert_eq!(<$ty>::ZERO.saturating_sub(1), <$ty>::ZERO);
            assert_eq!(<$ty>::ZERO.checked_add(1), Some(<$ty>::new(1)));
            assert_eq!(
                serde_json::to_value(maximum).unwrap(),
                serde_json::to_value(<$integer>::MAX).unwrap()
            );
            assert_eq!(
                serde_json::from_value::<$ty>(serde_json::to_value(<$integer>::MAX).unwrap())
                    .unwrap(),
                maximum
            );
            assert!(serde_json::from_str::<$ty>("-1").is_err());
            assert!(serde_json::from_str::<$ty>("18446744073709551616").is_err());
        };
    }
    check!(TaskGeneration, u32);
    check!(TaskSequence, u32);
    check!(TaskTick, u64);
}
