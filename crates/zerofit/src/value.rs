//! Decoded field values.

use core::fmt;

use crate::base_type::{BaseType, Endian};

/// A field value decoded from its base type, borrowing from the input where
/// possible.
///
/// Fields whose size is one element decode to a scalar variant. Fields holding
/// several elements decode to [`Value::Array`]. `String` and `Byte` fields
/// always decode to [`Value::String`] and [`Value::Bytes`].
///
/// A field whose size is zero or not a multiple of its base type's size cannot
/// be interpreted as that type; it decodes to [`Value::Bytes`] so no data is
/// lost. The FIT SDK treats such fields as byte arrays too.
///
/// ```
/// use zerofit::{BaseType, Endian, Value};
///
/// let v = Value::decode(BaseType::UInt16, Endian::Big, &[0x01, 0x02]);
/// assert_eq!(v, Value::UInt16(0x0102));
/// assert!(!v.is_invalid());
///
/// let invalid = Value::decode(BaseType::UInt16, Endian::Little, &[0xFF, 0xFF]);
/// assert!(invalid.is_invalid());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    /// [`BaseType::Enum`].
    Enum(u8),
    /// [`BaseType::SInt8`].
    SInt8(i8),
    /// [`BaseType::UInt8`].
    UInt8(u8),
    /// [`BaseType::SInt16`].
    SInt16(i16),
    /// [`BaseType::UInt16`].
    UInt16(u16),
    /// [`BaseType::SInt32`].
    SInt32(i32),
    /// [`BaseType::UInt32`].
    UInt32(u32),
    /// [`BaseType::String`].
    String(FitStr<'a>),
    /// [`BaseType::Float32`].
    Float32(f32),
    /// [`BaseType::Float64`].
    Float64(f64),
    /// [`BaseType::UInt8z`].
    UInt8z(u8),
    /// [`BaseType::UInt16z`].
    UInt16z(u16),
    /// [`BaseType::UInt32z`].
    UInt32z(u32),
    /// [`BaseType::Byte`], or any field that cannot be read as its base type.
    Bytes(&'a [u8]),
    /// [`BaseType::SInt64`].
    SInt64(i64),
    /// [`BaseType::UInt64`].
    UInt64(u64),
    /// [`BaseType::UInt64z`].
    UInt64z(u64),
    /// Several elements of a numeric base type.
    Array(Array<'a>),
}

impl<'a> Value<'a> {
    /// Decodes a field's bytes as `base_type` in byte order `endian`.
    ///
    /// Never fails: bytes that do not fit the base type decode to
    /// [`Value::Bytes`].
    #[must_use]
    pub fn decode(base_type: BaseType, endian: Endian, bytes: &'a [u8]) -> Self {
        match base_type {
            BaseType::String => return Self::String(FitStr::from_field(bytes)),
            BaseType::Byte => return Self::Bytes(bytes),
            _ => {}
        }
        let size = base_type.size();
        if bytes.len() == size {
            scalar(base_type, endian, bytes)
        } else if !bytes.is_empty() && bytes.len().checked_rem(size) == Some(0) {
            Self::Array(Array {
                base_type,
                endian,
                bytes,
            })
        } else {
            Self::Bytes(bytes)
        }
    }

    /// Returns `true` if this is the base type's "invalid" sentinel, meaning
    /// the device had no value for the field.
    ///
    /// Strings are invalid when empty, byte arrays when every byte is `0xFF`,
    /// and numeric arrays when every element is invalid.
    #[must_use]
    pub fn is_invalid(&self) -> bool {
        match *self {
            Self::Enum(v) | Self::UInt8(v) => v == u8::MAX,
            Self::SInt8(v) => v == i8::MAX,
            Self::SInt16(v) => v == i16::MAX,
            Self::UInt16(v) => v == u16::MAX,
            Self::SInt32(v) => v == i32::MAX,
            Self::UInt32(v) => v == u32::MAX,
            Self::String(s) => s.as_bytes().is_empty(),
            Self::Float32(v) => v.to_bits() == u32::MAX,
            Self::Float64(v) => v.to_bits() == u64::MAX,
            Self::UInt8z(v) => v == 0,
            Self::UInt16z(v) => v == 0,
            Self::UInt32z(v) => v == 0,
            Self::UInt64z(v) => v == 0,
            Self::Bytes(b) => b.iter().all(|&x| x == 0xFF),
            Self::SInt64(v) => v == i64::MAX,
            Self::UInt64(v) => v == u64::MAX,
            Self::Array(a) => a.iter().all(|v| v.is_invalid()),
        }
    }

    /// Returns the value as an `i64` if it is an integer scalar (any signed or
    /// unsigned integer or enum type) that fits.
    ///
    /// ```
    /// use zerofit::Value;
    /// assert_eq!(Value::UInt16(7).as_i64(), Some(7));
    /// assert_eq!(Value::UInt64(u64::MAX).as_i64(), None);
    /// assert_eq!(Value::Float32(1.0).as_i64(), None);
    /// ```
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::Enum(v) | Self::UInt8(v) | Self::UInt8z(v) => Some(i64::from(v)),
            Self::SInt8(v) => Some(i64::from(v)),
            Self::SInt16(v) => Some(i64::from(v)),
            Self::UInt16(v) | Self::UInt16z(v) => Some(i64::from(v)),
            Self::SInt32(v) => Some(i64::from(v)),
            Self::UInt32(v) | Self::UInt32z(v) => Some(i64::from(v)),
            Self::SInt64(v) => Some(v),
            Self::UInt64(v) | Self::UInt64z(v) => i64::try_from(v).ok(),
            Self::String(_)
            | Self::Float32(_)
            | Self::Float64(_)
            | Self::Bytes(_)
            | Self::Array(_) => None,
        }
    }

    /// Returns the value as an `f64` if it is a numeric scalar. 64-bit
    /// integers beyond 2^53 lose precision.
    ///
    /// ```
    /// use zerofit::Value;
    /// assert_eq!(Value::SInt8(-3).as_f64(), Some(-3.0));
    /// assert_eq!(Value::Float32(0.5).as_f64(), Some(0.5));
    /// ```
    #[must_use]
    // Precision loss for huge 64-bit integers is documented above.
    #[allow(clippy::cast_precision_loss)]
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Float32(v) => Some(f64::from(v)),
            Self::Float64(v) => Some(v),
            Self::UInt64(v) | Self::UInt64z(v) => Some(v as f64),
            _ => self.as_i64().map(|v| v as f64),
        }
    }
}

/// Decodes exactly one element. `bytes.len()` must equal `base_type.size()`;
/// otherwise the bytes are returned uninterpreted.
fn scalar(base_type: BaseType, endian: Endian, bytes: &[u8]) -> Value<'_> {
    macro_rules! read {
        ($t:ty, $n:literal) => {{
            let Some(&arr) = bytes.first_chunk::<$n>() else {
                return Value::Bytes(bytes);
            };
            match endian {
                Endian::Little => <$t>::from_le_bytes(arr),
                Endian::Big => <$t>::from_be_bytes(arr),
            }
        }};
    }
    match base_type {
        BaseType::Enum => Value::Enum(read!(u8, 1)),
        BaseType::SInt8 => Value::SInt8(read!(i8, 1)),
        BaseType::UInt8 => Value::UInt8(read!(u8, 1)),
        BaseType::SInt16 => Value::SInt16(read!(i16, 2)),
        BaseType::UInt16 => Value::UInt16(read!(u16, 2)),
        BaseType::SInt32 => Value::SInt32(read!(i32, 4)),
        BaseType::UInt32 => Value::UInt32(read!(u32, 4)),
        BaseType::String => Value::String(FitStr::from_field(bytes)),
        BaseType::Float32 => Value::Float32(read!(f32, 4)),
        BaseType::Float64 => Value::Float64(read!(f64, 8)),
        BaseType::UInt8z => Value::UInt8z(read!(u8, 1)),
        BaseType::UInt16z => Value::UInt16z(read!(u16, 2)),
        BaseType::UInt32z => Value::UInt32z(read!(u32, 4)),
        BaseType::Byte => Value::Bytes(bytes),
        BaseType::SInt64 => Value::SInt64(read!(i64, 8)),
        BaseType::UInt64 => Value::UInt64(read!(u64, 8)),
        BaseType::UInt64z => Value::UInt64z(read!(u64, 8)),
    }
}

/// Several elements of one numeric base type, decoded lazily.
///
/// ```
/// use zerofit::{BaseType, Endian, Value};
///
/// let Value::Array(a) = Value::decode(BaseType::UInt8, Endian::Little, &[1, 2, 0xFF]) else {
///     unreachable!()
/// };
/// assert_eq!(a.len(), 3);
/// assert_eq!(a.get(1), Some(Value::UInt8(2)));
/// let valid: Vec<_> = a.iter().filter(|v| !v.is_invalid()).collect();
/// assert_eq!(valid, [Value::UInt8(1), Value::UInt8(2)]);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Array<'a> {
    base_type: BaseType,
    endian: Endian,
    bytes: &'a [u8],
}

impl<'a> Array<'a> {
    /// Base type of every element.
    #[must_use]
    pub const fn base_type(&self) -> BaseType {
        self.base_type
    }

    /// Number of elements.
    #[must_use]
    pub const fn len(&self) -> usize {
        // `Array` is only built with a non-empty length that is a multiple of
        // the element size, and element sizes are never zero.
        match self.bytes.len().checked_div(self.base_type.size()) {
            Some(n) => n,
            None => 0,
        }
    }

    /// Always `false`: arrays have at least one element.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The raw bytes of all elements.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Element `index`, or `None` if out of range.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Value<'a>> {
        self.iter().nth(index)
    }

    /// Iterates over the elements, including invalid ones (see
    /// [`Value::is_invalid`]).
    #[must_use]
    pub fn iter(&self) -> ArrayIter<'a> {
        ArrayIter {
            base_type: self.base_type,
            endian: self.endian,
            chunks: self.bytes.chunks_exact(self.base_type.size()),
        }
    }
}

impl fmt::Debug for Array<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> IntoIterator for Array<'a> {
    type Item = Value<'a>;
    type IntoIter = ArrayIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &Array<'a> {
    type Item = Value<'a>;
    type IntoIter = ArrayIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over the elements of an [`Array`].
#[derive(Debug, Clone)]
pub struct ArrayIter<'a> {
    base_type: BaseType,
    endian: Endian,
    chunks: core::slice::ChunksExact<'a, u8>,
}

impl<'a> Iterator for ArrayIter<'a> {
    type Item = Value<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.chunks
            .next()
            .map(|c| scalar(self.base_type, self.endian, c))
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.chunks
            .nth(n)
            .map(|c| scalar(self.base_type, self.endian, c))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.chunks.size_hint()
    }
}

impl ExactSizeIterator for ArrayIter<'_> {}

/// A FIT string: the bytes of a `String` field up to (not including) the
/// first NUL.
///
/// FIT strings are specified as UTF-8, but devices do not always comply, so
/// conversion to `&str` is fallible and never allocates.
///
/// ```
/// use zerofit::{BaseType, Endian, Value};
///
/// let Value::String(s) = Value::decode(BaseType::String, Endian::Little, b"Edge 540\0\0\0") else {
///     unreachable!()
/// };
/// assert_eq!(s.to_str(), Ok("Edge 540"));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FitStr<'a>(&'a [u8]);

impl<'a> FitStr<'a> {
    /// Takes a string field's bytes up to the first NUL.
    fn from_field(bytes: &'a [u8]) -> Self {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        Self(bytes.get(..end).unwrap_or(bytes))
    }

    /// The string's bytes, without terminator or padding.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.0
    }

    /// The string as UTF-8.
    ///
    /// # Errors
    ///
    /// Returns the UTF-8 error if the bytes are not valid UTF-8.
    pub const fn to_str(&self) -> Result<&'a str, core::str::Utf8Error> {
        core::str::from_utf8(self.0)
    }
}

impl fmt::Debug for FitStr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_str() {
            Ok(s) => fmt::Debug::fmt(s, f),
            Err(_) => write!(f, "FitStr({:02x?})", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::format;
    use std::vec::Vec;

    #[test]
    fn scalars_little_and_big_endian() {
        let b = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let le = Endian::Little;
        let be = Endian::Big;
        assert_eq!(
            Value::decode(BaseType::UInt16, le, &b[..2]),
            Value::UInt16(0x0201)
        );
        assert_eq!(
            Value::decode(BaseType::UInt16, be, &b[..2]),
            Value::UInt16(0x0102)
        );
        assert_eq!(
            Value::decode(BaseType::UInt32, be, &b[..4]),
            Value::UInt32(0x0102_0304)
        );
        assert_eq!(
            Value::decode(BaseType::UInt64, le, &b),
            Value::UInt64(0x0807_0605_0403_0201)
        );
        assert_eq!(
            Value::decode(BaseType::SInt8, le, &[0xFE]),
            Value::SInt8(-2)
        );
        assert_eq!(Value::decode(BaseType::Enum, be, &[3]), Value::Enum(3));
    }

    #[test]
    fn sentinels() {
        let le = Endian::Little;
        let cases: [(BaseType, &[u8]); 15] = [
            (BaseType::Enum, &[0xFF]),
            (BaseType::SInt8, &[0x7F]),
            (BaseType::UInt8, &[0xFF]),
            (BaseType::SInt16, &[0xFF, 0x7F]),
            (BaseType::UInt16, &[0xFF, 0xFF]),
            (BaseType::SInt32, &[0xFF, 0xFF, 0xFF, 0x7F]),
            (BaseType::UInt32, &[0xFF; 4]),
            (BaseType::Float32, &[0xFF; 4]),
            (BaseType::Float64, &[0xFF; 8]),
            (BaseType::UInt8z, &[0]),
            (BaseType::UInt16z, &[0; 2]),
            (BaseType::UInt32z, &[0; 4]),
            (
                BaseType::SInt64,
                &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F],
            ),
            (BaseType::UInt64, &[0xFF; 8]),
            (BaseType::UInt64z, &[0; 8]),
        ];
        for (t, bytes) in cases {
            assert!(Value::decode(t, le, bytes).is_invalid(), "{t:?}");
        }
        assert!(Value::decode(BaseType::String, le, &[0, b'a']).is_invalid());
        assert!(Value::decode(BaseType::Byte, le, &[0xFF, 0xFF]).is_invalid());
        assert!(!Value::decode(BaseType::Byte, le, &[0xFF, 0xFE]).is_invalid());
        // A NaN that is not the all-ones pattern is a valid float.
        assert!(!Value::Float32(f32::NAN).is_invalid());
    }

    #[test]
    fn sentinel_respects_endianness() {
        // 0x7FFF big-endian is the SInt16 sentinel; little-endian it is 0xFF7F.
        assert!(Value::decode(BaseType::SInt16, Endian::Big, &[0x7F, 0xFF]).is_invalid());
        assert!(!Value::decode(BaseType::SInt16, Endian::Little, &[0x7F, 0xFF]).is_invalid());
    }

    #[test]
    fn arrays() {
        let v = Value::decode(BaseType::UInt16, Endian::Big, &[0, 1, 0, 2, 0xFF, 0xFF]);
        let Value::Array(a) = v else { panic!("{v:?}") };
        assert_eq!(a.len(), 3);
        assert!(!a.is_empty());
        assert_eq!(a.base_type(), BaseType::UInt16);
        assert_eq!(
            a.iter().collect::<Vec<_>>(),
            [Value::UInt16(1), Value::UInt16(2), Value::UInt16(0xFFFF)]
        );
        assert_eq!(a.get(1), Some(Value::UInt16(2)));
        assert_eq!(a.get(3), None);
        assert_eq!(a.iter().len(), 3);
        assert!(!v.is_invalid());
        assert_eq!(format!("{a:?}"), "[UInt16(1), UInt16(2), UInt16(65535)]");
    }

    #[test]
    fn array_of_all_invalid_is_invalid() {
        let v = Value::decode(BaseType::UInt8, Endian::Little, &[0xFF, 0xFF, 0xFF]);
        assert!(v.is_invalid());
    }

    #[test]
    fn size_mismatch_falls_back_to_bytes() {
        let le = Endian::Little;
        assert_eq!(
            Value::decode(BaseType::UInt32, le, &[1, 2, 3]),
            Value::Bytes(&[1, 2, 3])
        );
        assert_eq!(
            Value::decode(BaseType::UInt16, le, &[1, 2, 3]),
            Value::Bytes(&[1, 2, 3])
        );
        assert_eq!(Value::decode(BaseType::UInt16, le, &[]), Value::Bytes(&[]));
        // An empty field carries no value.
        assert!(Value::decode(BaseType::UInt16, le, &[]).is_invalid());
    }

    #[test]
    fn strings() {
        let s = |b: &'static [u8]| match Value::decode(BaseType::String, Endian::Little, b) {
            Value::String(s) => s,
            other => panic!("{other:?}"),
        };
        assert_eq!(s(b"abc\0\0").to_str(), Ok("abc"));
        assert_eq!(s(b"abc").to_str(), Ok("abc"));
        assert_eq!(s(b"\0abc").as_bytes(), b"");
        assert_eq!(s("héllo\0".as_bytes()).to_str(), Ok("héllo"));
        assert!(s(&[0xC3, 0x28, 0]).to_str().is_err());
        assert_eq!(format!("{:?}", s(b"hi\0")), "\"hi\"");
        assert_eq!(format!("{:?}", s(&[0xC3, 0x28])), "FitStr([c3, 28])");
    }

    #[test]
    fn numeric_conversions() {
        assert_eq!(Value::SInt64(-5).as_i64(), Some(-5));
        assert_eq!(Value::UInt64z(5).as_i64(), Some(5));
        assert_eq!(
            Value::UInt64(u64::MAX).as_f64(),
            Some(1.844_674_407_370_955_2e19)
        );
        assert_eq!(Value::Float64(2.5).as_i64(), None);
        assert_eq!(Value::Bytes(&[1]).as_f64(), None);
        assert_eq!(Value::String(FitStr(b"x")).as_i64(), None);
    }

    /// Encodes `bits` (the low `size` bytes) in the given byte order.
    fn encode(bits: u64, size: usize, endian: Endian) -> Vec<u8> {
        let le = bits.to_le_bytes();
        let mut out = le[..size].to_vec();
        if endian == Endian::Big {
            out.reverse();
        }
        out
    }

    fn endian() -> impl Strategy<Value = Endian> {
        prop_oneof![Just(Endian::Little), Just(Endian::Big)]
    }

    fn numeric_type() -> impl Strategy<Value = BaseType> {
        proptest::sample::select(
            BaseType::ALL
                .iter()
                .copied()
                .filter(|t| !matches!(t, BaseType::String | BaseType::Byte))
                .collect::<Vec<_>>(),
        )
    }

    /// Reinterprets a decoded scalar as its raw bits, zero-extended.
    #[allow(clippy::cast_sign_loss)]
    fn bits_of(v: Value<'_>) -> u64 {
        match v {
            Value::Enum(x) | Value::UInt8(x) | Value::UInt8z(x) => u64::from(x),
            Value::SInt8(x) => u64::from(x as u8),
            Value::SInt16(x) => u64::from(x as u16),
            Value::UInt16(x) | Value::UInt16z(x) => u64::from(x),
            Value::SInt32(x) => u64::from(x as u32),
            Value::UInt32(x) | Value::UInt32z(x) => u64::from(x),
            Value::Float32(x) => u64::from(x.to_bits()),
            Value::Float64(x) => x.to_bits(),
            Value::SInt64(x) => x as u64,
            Value::UInt64(x) | Value::UInt64z(x) => x,
            other => panic!("not a scalar: {other:?}"),
        }
    }

    proptest! {
        #[test]
        fn scalar_round_trip(t in numeric_type(), e in endian(), bits in any::<u64>()) {
            let size = t.size();
            let mask = if size == 8 { u64::MAX } else { (1u64 << (size * 8)) - 1 };
            let bits = bits & mask;
            let bytes = encode(bits, size, e);
            let v = Value::decode(t, e, &bytes);
            prop_assert_eq!(bits_of(v), bits);
        }

        #[test]
        fn array_round_trip(
            t in numeric_type(),
            e in endian(),
            elems in proptest::collection::vec(any::<u64>(), 2..16),
        ) {
            let size = t.size();
            let mask = if size == 8 { u64::MAX } else { (1u64 << (size * 8)) - 1 };
            let elems: Vec<u64> = elems.iter().map(|b| b & mask).collect();
            let bytes: Vec<u8> = elems.iter().flat_map(|&b| encode(b, size, e)).collect();
            let Value::Array(a) = Value::decode(t, e, &bytes) else {
                panic!("expected array");
            };
            prop_assert_eq!(a.len(), elems.len());
            let decoded: Vec<u64> = a.iter().map(bits_of).collect();
            prop_assert_eq!(decoded, elems);
        }

        #[test]
        fn only_the_sentinel_is_invalid(t in numeric_type(), e in endian(), bits in any::<u64>()) {
            let size = t.size();
            let mask = if size == 8 { u64::MAX } else { (1u64 << (size * 8)) - 1 };
            let bits = bits & mask;
            let sentinel = match t {
                BaseType::UInt8z | BaseType::UInt16z | BaseType::UInt32z | BaseType::UInt64z => 0,
                BaseType::SInt8 | BaseType::SInt16 | BaseType::SInt32 | BaseType::SInt64 => mask >> 1,
                _ => mask,
            };
            let bytes = encode(bits, size, e);
            let v = Value::decode(t, e, &bytes);
            prop_assert_eq!(v.is_invalid(), bits == sentinel);
        }

        #[test]
        fn decode_never_panics(
            t in proptest::sample::select(BaseType::ALL.to_vec()),
            e in endian(),
            bytes in proptest::collection::vec(any::<u8>(), 0..64),
        ) {
            let v = Value::decode(t, e, &bytes);
            let _ = v.is_invalid();
            let _ = v.as_f64();
            if let Value::Array(a) = v {
                prop_assert_eq!(a.iter().count(), a.len());
            }
        }
    }
}
