use crate::constants::{MAX_I32_SCALE, POWERS_10, U32_MASK, U32_MAX};
use crate::decimal::Decimal;
use crate::ops::common::Dec64;

use core::cmp::Ordering;

#[inline]
pub(crate) const fn cmp_impl(d1: &Decimal, d2: &Decimal) -> Ordering {
    let d1_zero = d1.is_zero();
    let d2_zero = d2.is_zero();

    if d2_zero {
        return if d1_zero {
            Ordering::Equal
        } else if d1.is_sign_negative() {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    if d1_zero {
        return if d2.is_sign_negative() {
            Ordering::Greater
        } else {
            Ordering::Less
        };
    }

    let d1_neg = d1.is_sign_negative();
    let d2_neg = d2.is_sign_negative();
    if d1_neg != d2_neg {
        return if d1_neg { Ordering::Less } else { Ordering::Greater };
    }

    let d1 = Dec64::new(d1);
    let d2 = Dec64::new(d2);
    // For negative numbers the ordering flips: −0.5 < −0.01.
    if d1_neg {
        cmp_internal(&d2, &d1)
    } else {
        cmp_internal(&d1, &d2)
    }
}

/// Compare two `Dec64` magnitudes (sign ignored).
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

    // Manual branches — `.cmp()` on primitives is not yet stable-const.
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

/// Multiply `(low64, high)` by `10^diff` in-place.
/// Returns `false` if the result overflows 96 bits.
#[inline]
const fn rescale(low64: &mut u64, high: &mut u32, diff: u32) -> bool {
    let mut diff = diff as i32;
    loop {
        let power = if diff >= MAX_I32_SCALE {
            POWERS_10[9]
        } else {
            POWERS_10[diff as usize]
        } as u64;

        let lo32 = (*low64 & U32_MASK) * power;
        let mid = (*low64 >> 32) * power + (lo32 >> 32);
        *low64 = (lo32 & U32_MASK) | (mid << 32);
        let hi = (mid >> 32) + (*high as u64) * power;

        if hi > U32_MAX {
            return false;
        }
        *high = hi as u32;

        diff -= MAX_I32_SCALE;
        if diff <= 0 {
            break;
        }
    }
    true
}
