use crate::constants::{MAX_I32_SCALE, POWERS_10, SCALE_MASK, SCALE_SHIFT, SIGN_MASK, U32_MASK, U32_MAX};
use crate::decimal::{CalculationResult, Decimal};
use crate::ops::common::{Buf24, Dec64};

#[inline(always)]
pub(crate) const fn add_impl(d1: &Decimal, d2: &Decimal) -> CalculationResult {
    add_sub_internal(d1, d2, false)
}

#[inline(always)]
pub(crate) const fn sub_impl(d1: &Decimal, d2: &Decimal) -> CalculationResult {
    add_sub_internal(d1, d2, true)
}

#[inline]
const fn add_sub_internal(d1: &Decimal, d2: &Decimal, subtract: bool) -> CalculationResult {
    // Handle zero operands cheaply.
    if d1.is_zero() {
        let mut result = *d2;
        if subtract && !d2.is_zero() {
            result.set_sign_negative(d2.is_sign_positive());
        }
        return CalculationResult::Ok(result);
    }
    if d2.is_zero() {
        return CalculationResult::Ok(*d1);
    }

    let flags = d1.flags() ^ d2.flags();
    // XOR of sign bits tells us whether the *effective* operation flips.
    let subtract = subtract ^ ((flags & SIGN_MASK) != 0);
    let rescale = (flags & SCALE_MASK) != 0;

    // ── Fast path: both values fit in 32 bits ────────────────────────────────
    // BUG FIX: original had `d1.mid() | d1.hi() == 0` which, due to Rust's
    // operator precedence (| < ==), parsed as `d1.mid() | (d1.hi() == 0)`
    // — a type error.  Corrected to `(d1.mid() | d1.hi()) == 0`.
    if (d1.mid() | d1.hi()) == 0 && (d2.mid() | d2.hi()) == 0 {
        if rescale {
            // rescale_factor > 0 means d2 has a larger scale → rescale d2 up.
            let rescale_factor = ((d2.flags() & SCALE_MASK) as i32 - (d1.flags() & SCALE_MASK) as i32) >> SCALE_SHIFT;
            if rescale_factor < 0 {
                if let Some(rescaled) = rescale32(d2.lo(), -rescale_factor) {
                    return fast_add(d1.lo(), rescaled, d1.flags(), subtract);
                }
            } else if let Some(rescaled) = rescale32(d1.lo(), rescale_factor) {
                return fast_add(
                    rescaled,
                    d2.lo(),
                    (d2.flags() & SCALE_MASK) | (d1.flags() & SIGN_MASK),
                    subtract,
                );
            }
            // Fall through to the 64-bit path if rescaling overflowed.
        } else {
            return fast_add(d1.lo(), d2.lo(), d1.flags(), subtract);
        }
    }

    // ── 64-bit path ──────────────────────────────────────────────────────────
    let d1 = Dec64::new(d1);
    let d2 = Dec64::new(d2);

    if rescale {
        let rescale_factor = d2.scale as i32 - d1.scale as i32;
        if rescale_factor < 0 {
            let negative = subtract ^ d1.negative;
            let scale = d1.scale;
            unaligned_add(d2, d1, negative, scale, -rescale_factor, subtract)
        } else {
            let negative = d1.negative;
            let scale = d2.scale;
            unaligned_add(d1, d2, negative, scale, rescale_factor, subtract)
        }
    } else {
        let neg = d1.negative;
        let scale = d1.scale;
        aligned_add(d1, d2, neg, scale, subtract)
    }
}

/// Multiply a 32-bit value by `10^rescale_factor`, returning `None` on overflow.
#[inline(always)]
const fn rescale32(num: u32, rescale_factor: i32) -> Option<u32> {
    if rescale_factor > MAX_I32_SCALE {
        return None;
    }
    num.checked_mul(POWERS_10[rescale_factor as usize])
}

/// Add or subtract two single-word (≤32-bit) decimals.
#[inline(always)]
const fn fast_add(lo1: u32, lo2: u32, flags: u32, subtract: bool) -> CalculationResult {
    if subtract {
        // Guarantee the larger value is always on the left to avoid underflow.
        if lo1 < lo2 {
            return CalculationResult::Ok(Decimal::from_parts_raw(lo2 - lo1, 0, 0, flags ^ SIGN_MASK));
        }
        return CalculationResult::Ok(Decimal::from_parts_raw(lo1 - lo2, 0, 0, flags));
    }
    // Addition: detect carry into the mid word.
    let lo = lo1.wrapping_add(lo2);
    let mid = (lo < lo1) as u32; // branchless carry
    CalculationResult::Ok(Decimal::from_parts_raw(lo, mid, 0, flags))
}

/// Add or subtract two aligned (same-scale) 96-bit decimals.
const fn aligned_add(lhs: Dec64, rhs: Dec64, negative: bool, scale: u32, subtract: bool) -> CalculationResult {
    if subtract {
        let mut result = Dec64 {
            negative,
            scale,
            low64: lhs.low64.wrapping_sub(rhs.low64),
            hi: lhs.hi.wrapping_sub(rhs.hi),
        };

        // Borrow from hi when low64 underflowed.
        if result.low64 > lhs.low64 {
            result.hi = result.hi.wrapping_sub(1);
            // If hi also underflowed the result is negative; flip the sign.
            if result.hi >= lhs.hi {
                flip_sign(&mut result);
            }
        } else if result.hi > lhs.hi {
            flip_sign(&mut result);
        }
        CalculationResult::Ok(result.to_decimal())
    } else {
        let mut result = Dec64 {
            negative,
            scale,
            low64: lhs.low64.wrapping_add(rhs.low64),
            hi: lhs.hi.wrapping_add(rhs.hi),
        };

        // Carry into hi when low64 wrapped.
        if result.low64 < lhs.low64 {
            result.hi = result.hi.wrapping_add(1);
            if result.hi <= lhs.hi {
                // hi also overflowed → need to reduce scale.
                if result.scale == 0 {
                    return CalculationResult::Overflow;
                }
                reduce_scale(&mut result);
            }
        } else if result.hi < lhs.hi {
            if result.scale == 0 {
                return CalculationResult::Overflow;
            }
            reduce_scale(&mut result);
        }
        CalculationResult::Ok(result.to_decimal())
    }
}

/// Negate a `Dec64` in-place (two's complement across 96 bits).
#[inline(always)]
const fn flip_sign(result: &mut Dec64) {
    result.hi = !result.hi;
    let low64 = (result.low64 as i64).wrapping_neg() as u64;
    if low64 == 0 {
        result.hi = result.hi.wrapping_add(1);
    }
    result.low64 = low64;
    result.negative = !result.negative;
}

/// Divide a 96-bit `Dec64` by 10 in-place and round, decrementing the scale.
///
/// Uses `%`/`/` so the compiler can emit a single `div` instruction for both
/// quotient and remainder (the original used a manual multiply-subtract).
const fn reduce_scale(result: &mut Dec64) {
    let mut low64 = result.low64;
    let mut hi = result.hi;

    // ── Divide the 96-bit value by 10 using 64-bit arithmetic ────────────
    // Step 1: hi portion (treat as (hi + 2^32) / 10 to include the implicit
    //         carry bit from the overflow detection in the caller).
    let num_hi = (hi as u64) + (1u64 << 32);
    hi = (num_hi / 10) as u32;
    let rem = num_hi % 10;

    // Step 2: mid portion.
    let num_mid = (rem << 32) + (low64 >> 32);
    let div_mid = (num_mid / 10) as u32;
    let rem = num_mid % 10;

    // Step 3: lo portion.
    let num_lo = (rem << 32) + (low64 & U32_MASK);
    let div_lo = (num_lo / 10) as u32;
    let remainder = (num_lo % 10) as u32;

    low64 = ((div_mid as u64) << 32) | (div_lo as u64);

    // Round: half-up, tie-to-odd.
    if remainder >= 5 && (remainder > 5 || (low64 & 1) != 0) {
        low64 = low64.wrapping_add(1);
        if low64 == 0 {
            hi = hi.wrapping_add(1);
        }
    }

    result.low64 = low64;
    result.hi = hi;
    result.scale -= 1;
}

/// Add/subtract two decimals with different scales.
///
/// `lhs` is the number with the *smaller* scale (i.e. the larger magnitude),
/// and `rhs` is the number to be rescaled by `rescale_factor` powers of 10.
const fn unaligned_add(
    lhs: Dec64,
    rhs: Dec64,
    negative: bool,
    scale: u32,
    rescale_factor: i32,
    subtract: bool,
) -> CalculationResult {
    let mut lhs = lhs;
    let mut low64 = lhs.low64;
    let mut high = lhs.hi;
    let mut rescale_factor = rescale_factor;

    // ── Attempt to stay within 96 bits ───────────────────────────────────────

    if high == 0 {
        if low64 <= U32_MAX {
            // Single 32-bit word — scale it directly.
            loop {
                if rescale_factor <= MAX_I32_SCALE {
                    low64 *= POWERS_10[rescale_factor as usize] as u64;
                    lhs.low64 = low64;
                    return aligned_add(lhs, rhs, negative, scale, subtract);
                }
                rescale_factor -= MAX_I32_SCALE;
                low64 *= POWERS_10[9] as u64;
                if low64 > U32_MAX {
                    break;
                }
            }
        }

        // Two-word (64-bit) scaling.
        while high == 0 {
            let power = if rescale_factor <= MAX_I32_SCALE {
                POWERS_10[rescale_factor as usize] as u64
            } else {
                POWERS_10[9] as u64
            };

            let tmp_lo = (low64 & U32_MASK) * power;
            let tmp_hi = (low64 >> 32) * power + (tmp_lo >> 32);
            low64 = (tmp_lo & U32_MASK) | (tmp_hi << 32);
            high = (tmp_hi >> 32) as u32;

            rescale_factor -= MAX_I32_SCALE;
            if rescale_factor <= 0 {
                lhs.low64 = low64;
                lhs.hi = high;
                return aligned_add(lhs, rhs, negative, scale, subtract);
            }
        }
    }

    // ── Try to stay within 96 bits with a 32-bit high word ───────────────────
    let mut tmp64: u64;
    loop {
        let power = if rescale_factor <= MAX_I32_SCALE {
            POWERS_10[rescale_factor as usize] as u64
        } else {
            POWERS_10[9] as u64
        };

        let tmp_lo = (low64 & U32_MASK) * power;
        tmp64 = (low64 >> 32) * power + (tmp_lo >> 32);
        low64 = (tmp_lo & U32_MASK) | (tmp64 << 32);
        tmp64 >>= 32;
        tmp64 += (high as u64) * power;

        rescale_factor -= MAX_I32_SCALE;

        if tmp64 > U32_MAX || scale > Decimal::MAX_SCALE {
            // Spilled above 96 bits — must use the 192-bit buffer.
            break;
        }

        high = tmp64 as u32;
        if rescale_factor <= 0 {
            lhs.low64 = low64;
            lhs.hi = high;
            return aligned_add(lhs, rhs, negative, scale, subtract);
        }
    }

    // ── 192-bit buffer path ───────────────────────────────────────────────────
    let mut buffer = Buf24::zero();
    buffer.set_low64(low64);
    buffer.set_mid64(tmp64); // tmp64 holds data[2..3]

    let mut upper_word = buffer.upper_word();

    while rescale_factor > 0 {
        let power = if rescale_factor <= MAX_I32_SCALE {
            POWERS_10[rescale_factor as usize] as u64
        } else {
            POWERS_10[9] as u64
        };

        // Multiply the entire buffer by `power` (up to `upper_word`).
        tmp64 = 0;
        let mut i = 0usize;
        loop {
            tmp64 = tmp64.wrapping_add(buffer.data[i] as u64 * power);
            buffer.data[i] = tmp64 as u32;
            tmp64 >>= 32;
            if i >= upper_word {
                break;
            }
            i += 1;
        }

        if tmp64 & U32_MASK > 0 {
            upper_word += 1;
            buffer.data[upper_word] = tmp64 as u32;
        }

        rescale_factor -= MAX_I32_SCALE;
    }

    // ── Perform the aligned add/subtract in the buffer ───────────────────────
    tmp64 = buffer.low64();
    let tmp_hi = buffer.data[2];
    let rhs_low64 = rhs.low64;
    let rhs_hi = rhs.hi;

    let (result_low64, result_hi);

    if subtract {
        result_low64 = tmp64.wrapping_sub(rhs_low64);
        result_hi = tmp_hi.wrapping_sub(rhs_hi);

        let carry = if result_low64 > tmp64 {
            let borrow_hi = result_hi.wrapping_sub(1);
            // borrow_hi underflowed means tmp_hi was 0 → net borrow into higher words
            borrow_hi >= tmp_hi
        } else {
            result_hi > tmp_hi
        };

        // Fix up the carry into the higher buffer words.
        let result_hi = if result_low64 > tmp64 {
            result_hi.wrapping_sub(1)
        } else {
            result_hi
        };

        if carry {
            let mut i = 3usize;
            while i < 6 {
                buffer.data[i] = buffer.data[i].wrapping_sub(1);
                if buffer.data[i] != u32::MAX {
                    break;
                }
                i += 1;
            }

            // If the buffer collapsed to ≤ 96 bits, return directly.
            if buffer.data[upper_word] == 0 && upper_word < 3 {
                return CalculationResult::Ok(Decimal::from_parts(
                    result_low64 as u32,
                    (result_low64 >> 32) as u32,
                    result_hi,
                    negative,
                    scale,
                ));
            }
        }

        buffer.set_low64(result_low64);
        buffer.data[2] = result_hi;
    } else {
        result_low64 = rhs_low64.wrapping_add(tmp64);
        result_hi = rhs_hi.wrapping_add(tmp_hi);

        let carry = if result_low64 < tmp64 {
            let carried_hi = result_hi.wrapping_add(1);
            carried_hi <= tmp_hi
        } else {
            result_hi < tmp_hi
        };

        let result_hi = if result_low64 < tmp64 {
            result_hi.wrapping_add(1)
        } else {
            result_hi
        };

        if carry {
            let mut i = 3usize;
            while i < 6 {
                if upper_word < i {
                    buffer.data[i] = 1;
                    upper_word = i;
                    break;
                }
                buffer.data[i] = buffer.data[i].wrapping_add(1);
                if buffer.data[i] != 0 {
                    break;
                }
                i += 1;
            }
        }

        buffer.set_low64(result_low64);
        buffer.data[2] = result_hi;
    }

    // Rescale the 192-bit buffer down to 96 bits and return.
    match buffer.rescale(upper_word, scale) {
        Some(scale) => CalculationResult::Ok(Decimal::from_parts(
            buffer.data[0],
            buffer.data[1],
            buffer.data[2],
            negative,
            scale,
        )),
        None => CalculationResult::Overflow,
    }
}
