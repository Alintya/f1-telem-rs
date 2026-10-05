//! Helpers used by generated code when the `serde` feature is enabled.

/// Fixed-length arrays of any length as serde tuples (serde's own `[T; N]` impls stop at 32).
///
/// Used as `#[serde(with = "crate::serde_support::array")]` on array fields.
pub(crate) mod array {
    use std::fmt;
    use std::marker::PhantomData;

    use serde::de::{self, Deserialize, Deserializer, SeqAccess, Visitor};
    use serde::ser::{Serialize, SerializeTuple, Serializer};

    pub fn serialize<S, T, const N: usize>(value: &[T; N], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        let mut tuple = serializer.serialize_tuple(N)?;
        for item in value {
            tuple.serialize_element(item)?;
        }
        tuple.end()
    }

    pub fn deserialize<'de, D, T, const N: usize>(deserializer: D) -> Result<[T; N], D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + Default + Copy,
    {
        struct ArrayVisitor<T, const N: usize>(PhantomData<T>);

        impl<'de, T, const N: usize> Visitor<'de> for ArrayVisitor<T, N>
        where
            T: Deserialize<'de> + Default + Copy,
        {
            type Value = [T; N];

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "an array of {N} elements")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<[T; N], A::Error> {
                let mut arr = [T::default(); N];
                for (i, slot) in arr.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }
                if seq.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::invalid_length(N + 1, &self));
                }
                Ok(arr)
            }
        }

        deserializer.deserialize_tuple(N, ArrayVisitor::<T, N>(PhantomData))
    }
}
