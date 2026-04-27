use crate::constants::{MAX_I32_SCALE, POWERS_10};
use crate::decimal::Decimal;
use crate::ops::common::Dec64;
use core::cmp::Ordering;

#[inline]
pub(crate) const fn cmp_impl(d1: &Decimal, d2: &Decimal) -> Ordering {
    // Read flags once — is_zero() and is_sign_negative() both touch the same
    // fields; pulling them up front lets the compiler reuse the loads.
    let d1_neg = d1.is_sign_negative();
    let d2_neg = d2.is_sign_negative();
    let d1_zero = d1.is_zero();
    let d2_zero = d2.is_zero();

    // Fused zero/sign table — handles all degenerate cases in 4 branches
    // instead of the original 6, with no repeated flag reads.
    if d1_zero & d2_zero {
        return Ordering::Equal;
    }
    if d1_zero {
        return if d2_neg { Ordering::Greater } else { Ordering::Less };
    }
    if d2_zero {
        return if d1_neg { Ordering::Less } else { Ordering::Greater };
    }
    if d1_neg != d2_neg {
        return if d1_neg { Ordering::Less } else { Ordering::Greater };
    }

    let d1 = Dec64::new(d1);
    let d2 = Dec64::new(d2);
    // Negative: ordering flips — −0.5 < −0.01
    if d1_neg {
        cmp_internal(&d2, &d1)
    } else {
        cmp_internal(&d1, &d2)
    }
}

pub(in crate::ops) const fn cmp_internal(d1: &Dec64, d2: &Dec64) -> Ordering {
    let mut d1_low = d1.low64;
    let mut d1_high = d1.hi;
    let mut d2_low = d2.low64;
    let mut d2_high = d2.hi;

    if d1.scale != d2.scale {
        let raw_diff = d2.scale as i32 - d1.scale as i32;
        if raw_diff < 0 {
            if !rescale(&mut d2_low, &mut d2_high, (-raw_diff) as u32) {
                return Ordering::Less;
            }
        } else if !rescale(&mut d1_low, &mut d1_high, raw_diff as u32) {
            return Ordering::Greater;
        }
    }

    if d1_high < d2_high {
        return Ordering::Less;
    }
    if d1_high > d2_high {
        return Ordering::Greater;
    }
    if d1_low < d2_low {
        return Ordering::Less;
    }
    if d1_low > d2_low {
        return Ordering::Greater;
    }
    Ordering::Equal
}

/// Scale `(low64, high)` up by `10^diff` in-place.
/// Returns `false` if the result overflows 96 bits.
///
/// Uses u128 to collapse what was 3 separate u64 multiplications + manual
/// carry chains into a single widening multiply per loop iteration.
/// The compiler lowers `u128 * u128` on x86-64 to two MUL instructions;
/// the overflow check is a single comparison of the top 32 bits.
#[inline]
const fn rescale(low64: &mut u64, high: &mut u32, diff: u32) -> bool {
    let mut diff = diff as i32;
    loop {
        let power = if diff >= MAX_I32_SCALE {
            POWERS_10[9]
        } else {
            POWERS_10[diff as usize]
        } as u128;

        // Pack the 96-bit value into a u128, multiply, then check/unpack.
        let val = (*low64 as u128) | ((*high as u128) << 64);
        let result = val * power;

        // Overflow: result must fit in 96 bits (bits 96–127 must all be zero).
        if result >> 96 != 0 {
            return false;
        }

        *low64 = result as u64;
        *high = (result >> 64) as u32;

        diff -= MAX_I32_SCALE;
        if diff <= 0 {
            break;
        }
    }
    true
}
