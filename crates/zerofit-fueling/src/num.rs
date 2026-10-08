//! Numeric conversions in one place, so the lossy casts the lints deny are
//! each justified once.

/// `n` as `f64`. Exact below 2^53.
#[allow(clippy::cast_precision_loss)]
pub(crate) const fn f64_from_usize(n: usize) -> f64 {
    n as f64
}

/// `x` rounded down and clamped to `0..=u32::MAX`; `NaN` becomes 0.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn floor_u32(x: f64) -> u32 {
    // `as` from float saturates and maps NaN to 0: the clamp we want.
    libm_floor(x) as u32
}

/// `x` rounded to the nearest integer and clamped to `0..=u32::MAX`;
/// `NaN` becomes 0.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn round_u32(x: f64) -> u32 {
    libm_floor(x + 0.5) as u32
}

/// `floor` without `std` or `libm`: truncation toward zero, minus one for
/// negative non-integers. Values beyond 2^52 are already integers.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn libm_floor(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= 4_503_599_627_370_496.0 {
        return x;
    }
    let t = (x as i64) as f64;
    if t > x { t - 1.0 } else { t }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(floor_u32(3.99), 3);
        assert_eq!(floor_u32(-2.5), 0);
        assert_eq!(floor_u32(f64::NAN), 0);
        assert_eq!(floor_u32(1e20), u32::MAX);
        assert_eq!(round_u32(2.5), 3);
        assert_eq!(round_u32(2.49), 2);
        assert_eq!(libm_floor(-1.5), -2.0);
        assert_eq!(libm_floor(-2.0), -2.0);
    }
}
