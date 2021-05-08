//! Wrapper types that wrap `std` types to implement
//! `serde::{Serialize, Deserialize}`
//!
//! We can't enable `serde/std` or `serde/alloc` in Driver mode because they
//! would also be enabled in Target mode and we would have to use the
//! still-unstable `#[alloc_error_handler]` attribute (otherwise the compiler
//! would complain whether `alloc` is actually used in the resulting binary or
//! not). This means we don't have access to the `(Ser|Deser)ialize`
//! implementations of `alloc`'s types.
//!
//! The coherence rules prevent us from implementing them by ourselves.
//! Therefore, this module wraps `alloc`'s types and implements
//! `(Ser|Deser)ialize` on these wrapper types.
use serde::{de, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serde<T>(pub T);

impl<T: Serialize> Serialize for Serde<Vec<T>> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.0.iter())
    }
}

impl Serialize for Serde<String> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de, T> de::Deserialize<'de> for Serde<Vec<T>>
where
    T: de::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct VecVisitor<T> {
            marker: std::marker::PhantomData<T>,
        }

        impl<'de, T> de::Visitor<'de> for VecVisitor<T>
        where
            T: de::Deserialize<'de>,
        {
            type Value = Serde<Vec<T>>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a sequence")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut values = Vec::with_capacity(size_hint::cautious(seq.size_hint()));

                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }

                Ok(Serde(values))
            }
        }

        let visitor = VecVisitor {
            marker: std::marker::PhantomData,
        };
        deserializer.deserialize_seq(visitor)
    }
}

impl<'de> de::Deserialize<'de> for Serde<String> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct StringVisitor;

        impl<'de> de::Visitor<'de> for StringVisitor {
            type Value = Serde<String>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Serde(v.to_owned()))
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                match std::str::from_utf8(v) {
                    Ok(s) => Ok(Serde(s.to_owned())),
                    Err(_) => Err(de::Error::invalid_value(
                        serde::de::Unexpected::Bytes(v),
                        &self,
                    )),
                }
            }
        }

        deserializer
            .deserialize_str(StringVisitor)
            .map(|st| st.to_owned())
    }
}

mod size_hint {
    #[inline]
    pub fn cautious(hint: Option<usize>) -> usize {
        std::cmp::min(hint.unwrap_or(0), 4096)
    }
}
