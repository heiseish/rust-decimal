use crate::constants::{MAX_SCALE_U32, POWERS_10, U32_MASK};

/// Rescale `value` to `new_scale`, rounding half-up if the scale is reduced.
#[inline]
pub(crate) const fn rescale_internal(value: &mut [u32; 3], value_scale: &mut u32, new_scale: u32) {
    rescale::<true>(value, value_scale, new_scale);
}

#[inline(always)]
const fn rescale<const ROUND: bool>(value: &mut [u32; 3], value_scale: &mut u32, new_scale: u32) {
    if *value_scale == new_scale {
        return;
    }

    if is_all_zero(value) {
        *value_scale = if new_scale < MAX_SCALE_U32 {
            new_scale
        } else {
            MAX_SCALE_U32
        };
        return;
    }

    if *value_scale > new_scale {
        let mut diff = value_scale.wrapping_sub(new_scale);
        let mut remainder = 0u32;

        while let Some(diff_minus_one) = diff.checked_sub(1) {
            if is_all_zero(value) {
                *value_scale = new_scale;
                return;
            }
            diff = diff_minus_one;
            remainder = div_by_u32(value, 10);
        }

        // Round half-up: propagate carry using an index loop (no iterators in const).
        if ROUND && remainder >= 5 {
            let mut carry = true;
            let mut i = 0usize;
            while carry && i < 3 {
                let digit = value[i] as u64 + 1;
                value[i] = (digit & U32_MASK) as u32;
                carry = digit > U32_MASK;
                i += 1;
            }
        }
        *value_scale = new_scale;
    } else {
        let mut diff = new_scale.wrapping_sub(*value_scale);
        let mut working = [value[0], value[1], value[2]];

        while let Some(diff_minus_one) = diff.checked_sub(1) {
            if mul_by_10(&mut working) == 0 {
                value[0] = working[0];
                value[1] = working[1];
                value[2] = working[2];
                diff = diff_minus_one;
            } else {
                break;
            }
        }
        *value_scale = new_scale.wrapping_sub(diff);
    }
}

/// Truncate `value` to `desired_scale` (no rounding).
#[inline]
pub(crate) const fn truncate_internal(value: &mut [u32; 3], value_scale: &mut u32, desired_scale: u32) {
    rescale::<false>(value, value_scale, desired_scale);
}

#[inline(always)]
pub(crate) const fn add_by_internal_flattened(value: &mut [u32; 3], by: u32) -> u32 {
    manage_add_by_internal(by, value)
}

#[inline(always)]
pub(crate) const fn add_one_internal(value: &mut [u32; 3]) -> u32 {
    manage_add_by_internal(1, value)
}

/// Add `initial_carry` into the `N`-limb array `value`, propagating carry.
/// Returns any carry out of the top limb.
// `u64 as u32` casts are safe: we only ever keep the low 32 bits.
#[inline]
pub(crate) const fn manage_add_by_internal<const N: usize>(initial_carry: u32, value: &mut [u32; N]) -> u32 {
    let mut carry = initial_carry as u64;
    let mut i = 0usize;
    // Process first limb unconditionally.
    if N > 0 {
        let sum = value[0] as u64 + carry;
        value[0] = sum as u32;
        carry = sum >> 32;
        i = 1;
    }
    // Remaining limbs: bail out once carry is zero.
    while i < N && carry > 0 {
        let sum = value[i] as u64 + carry;
        value[i] = sum as u32;
        carry = sum >> 32;
        i += 1;
    }
    carry as u32
}

/// Subtract `by` from `value` (little-endian limb slices), returning the final borrow.
pub(crate) const fn sub_by_internal(value: &mut [u32], by: &[u32]) -> u32 {
    let mut overflow = 0u32;
    // Manual min to stay const-compatible.
    let limit = if value.len() < by.len() { value.len() } else { by.len() };
    let mut i = 0usize;
    while i < limit {
        let (lo, hi) = sub_part(value[i], by[i], overflow);
        value[i] = lo;
        overflow = hi;
        i += 1;
    }
    overflow
}

#[inline(always)]
const fn sub_part(left: u32, right: u32, overflow: u32) -> (u32, u32) {
    let part = 0x1_0000_0000u64 + left as u64 - (right as u64 + overflow as u64);
    let lo = part as u32;
    let hi = 1 - (part >> 32) as u32;
    (lo, hi)
}

/// Multiply a 3-limb value by 10. Returns the overflow word.
///
/// Fully unrolled — no loop overhead, and the compiler can schedule the three
/// dependent chains optimally.
#[inline]
pub(crate) const fn mul_by_10(bits: &mut [u32; 3]) -> u32 {
    let r0 = bits[0] as u64 * 10;
    bits[0] = r0 as u32;
    let r1 = bits[1] as u64 * 10 + (r0 >> 32);
    bits[1] = r1 as u32;
    let r2 = bits[2] as u64 * 10 + (r1 >> 32);
    bits[2] = r2 as u32;
    (r2 >> 32) as u32
}

/// Multiply an N-limb slice by `m`. Returns the overflow word.
#[inline]
pub(crate) const fn mul_by_u32(bits: &mut [u32], m: u32) -> u32 {
    let mut overflow = 0u32;
    let mut i = 0usize;
    while i < bits.len() {
        let (lo, hi) = mul_part(bits[i], m, overflow);
        bits[i] = lo;
        overflow = hi;
        i += 1;
    }
    overflow
}

#[inline(always)]
pub(crate) const fn mul_part(left: u32, right: u32, high: u32) -> (u32, u32) {
    let result = left as u64 * right as u64 + high as u64;
    ((result & 0xFFFF_FFFF) as u32, (result >> 32) as u32)
}

/// Divide an `N`-limb value by `divisor` in-place. Returns the remainder.
///
/// Uses `%` alongside `/` so the compiler emits a single `div` instruction for both.
pub(crate) const fn div_by_u32<const N: usize>(bits: &mut [u32; N], divisor: u32) -> u32 {
    if divisor == 0 {
        panic!("Internal error: divide by zero");
    }
    if divisor == 1 {
        return 0;
    }
    let d = divisor as u64;
    let mut rem = 0u32;
    let mut i = N;
    while i > 0 {
        i -= 1;
        let temp = ((rem as u64) << 32) | bits[i] as u64;
        bits[i] = (temp / d) as u32;
        rem = (temp % d) as u32;
    }
    rem
}

/// Divide a fixed 3-limb value by `POWERS_10[POWER]`. Returns the remainder.
/// Fully unrolled; intended for small constant powers (POWER < 10).
pub(crate) const fn div_by_power<const POWER: usize>(bits: &mut [u32; 3]) -> u32 {
    let d = POWERS_10[POWER] as u64;

    let temp = bits[2] as u64;
    bits[2] = (temp / d) as u32;
    let rem = temp % d;

    let temp = (rem << 32) | bits[1] as u64;
    bits[1] = (temp / d) as u32;
    let rem = temp % d;

    let temp = (rem << 32) | bits[0] as u64;
    bits[0] = (temp / d) as u32;
    (temp % d) as u32
}

/// Left-shift a multi-limb value by 1, feeding `carry` into the LSB.
/// Returns the carry out of the MSB.
#[inline]
pub(crate) const fn shl1_internal(bits: &mut [u32], carry: u32) -> u32 {
    let mut carry = carry;
    let mut i = 0usize;
    while i < bits.len() {
        let out = bits[i] >> 31;
        bits[i] = (bits[i] << 1) | carry;
        carry = out;
        i += 1;
    }
    carry
}

/// Compare two 3-limb values, returning their `Ordering`.
#[inline]
pub(crate) const fn cmp_internal(left: &[u32; 3], right: &[u32; 3]) -> core::cmp::Ordering {
    let left_hi = left[2];
    let right_hi = right[2];
    let left_lo = ((left[1] as u64) << 32) | left[0] as u64;
    let right_lo = ((right[1] as u64) << 32) | right[0] as u64;

    // Manual branches — `u64::cmp` / `u32::cmp` are not yet stable const.
    if left_hi < right_hi {
        return core::cmp::Ordering::Less;
    }
    if left_hi > right_hi {
        return core::cmp::Ordering::Greater;
    }
    if left_lo < right_lo {
        return core::cmp::Ordering::Less;
    }
    if left_lo > right_lo {
        return core::cmp::Ordering::Greater;
    }
    core::cmp::Ordering::Equal
}

/// Returns `true` if every limb is zero.
#[inline]
pub(crate) const fn is_all_zero<const N: usize>(bits: &[u32; N]) -> bool {
    let mut i = 0usize;
    while i < N {
        if bits[i] != 0 {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::prelude::*;

    fn to_mantissa_array_with_scale(value: &str) -> ([u32; 3], u32) {
        let v = Decimal::from_str(value).unwrap();
        (v.mantissa_array3(), v.scale())
    }

    #[test]
    fn it_can_rescale_internal() {
        let tests = &[
            ("1", 0, "1", 0),
            ("1", 1, "1.0", 1),
            ("1", 5, "1.00000", 5),
            ("1", 10, "1.0000000000", 10),
            ("1", 20, "1.00000000000000000000", 20),
            (
                "0.6386554621848739495798319328",
                27,
                "0.638655462184873949579831933",
                27,
            ),
            ("843.65000000", 25, "843.6500000000000000000000000", 25),
            ("843.65000000", 30, "843.6500000000000000000000000", 25),
            ("0", 130, "0.000000000000000000000000000000", 28),
        ];

        for &(value_raw, new_scale, expected_value, expected_scale) in tests {
            let (expected_value, _) = to_mantissa_array_with_scale(expected_value);
            let (mut value, mut value_scale) = to_mantissa_array_with_scale(value_raw);
            rescale_internal(&mut value, &mut value_scale, new_scale);
            assert_eq!(value, expected_value);
            assert_eq!(
                value_scale, expected_scale,
                "value: {value_raw}, requested scale: {new_scale}"
            );
        }
    }

    #[test]
    fn test_shl1_internal() {
        struct TestCase {
            given: [u32; 3],
            given_carry: u32,
            expected: [u32; 3],
            expected_carry: u32,
        }
        let tests = [
            TestCase {
                given: [1, 0, 0],
                given_carry: 0,
                expected: [2, 0, 0],
                expected_carry: 0,
            },
            TestCase {
                given: [1, 0, 2147483648],
                given_carry: 1,
                expected: [3, 0, 0],
                expected_carry: 1,
            },
        ];
        for case in &tests {
            let mut test = case.given;
            let carry = shl1_internal(&mut test, case.given_carry);
            assert_eq!(
                test, case.expected,
                "Bits: {:?} << 1 | {}",
                case.given, case.given_carry
            );
            assert_eq!(
                carry, case.expected_carry,
                "Carry: {:?} << 1 | {}",
                case.given, case.given_carry
            );
        }
    }
}
