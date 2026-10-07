//! Numeric conversions in one place, so the lossy casts the lints deny are
//! each justified once.

/// `n` as `f64`. Exact below 2^53, far beyond any activity length or
/// power sum this crate handles.
#[allow(clippy::cast_precision_loss)]
pub(crate) const fn f64_from_usize(n: usize) -> f64 {
    n as f64
}

/// `x` rounded to the nearest integer and clamped to `0..=u16::MAX`;
/// `NaN` becomes 0.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn round_u16(x: f64) -> u16 {
    // `as` from float saturates and maps NaN to 0, which is the clamp we
    // want; the lints are about the general case.
    libm::round(x) as u16
}

/// `x` rounded to the nearest integer and clamped to `0..=u8::MAX`;
/// `NaN` becomes 0.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn round_u8(x: f64) -> u8 {
    libm::round(x) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_clamps() {
        assert_eq!(round_u16(2.5), 3);
        assert_eq!(round_u16(-4.0), 0);
        assert_eq!(round_u16(1e9), u16::MAX);
        assert_eq!(round_u16(f64::NAN), 0);
        assert_eq!(round_u8(300.0), u8::MAX);
    }
}
