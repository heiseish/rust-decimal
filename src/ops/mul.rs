use crate::constants::{BIG_POWERS_10, MAX_I64_SCALE, U32_MAX};
use crate::decimal::{CalculationResult, Decimal};
use crate::ops::common::Buf24;

pub(crate) const fn mul_impl(d1: &Decimal, d2: &Decimal) -> CalculationResult {
    if d1.is_zero() || d2.is_zero() {
        return CalculationResult::Ok(Decimal::ZERO);
    }

    let mut scale = d1.scale() + d2.scale();
    let negative = d1.is_sign_negative() ^ d2.is_sign_negative();
    let mut product = Buf24::zero();

    // BUG FIX (×2): original `d1.hi() | d1.mid() == 0` parsed as
    // `d1.hi() | (d1.mid() == 0)` — u32 | bool type error.
    // Corrected form: `(d1.hi() | d1.mid()) == 0`.
    if (d1.hi() | d1.mid()) == 0 {
        if (d2.hi() | d2.mid()) == 0 {
            // ── Both operands fit in 32 bits ──────────────────────────────────
            let mut low64 = d1.lo() as u64 * d2.lo() as u64;

            if scale > Decimal::MAX_SCALE {
                if scale > Decimal::MAX_SCALE + MAX_I64_SCALE {
                    return CalculationResult::Ok(Decimal::ZERO);
                }
                scale -= Decimal::MAX_SCALE + 1;
                let power = BIG_POWERS_10[scale as usize];
                let tmp = low64 / power;
                let remainder = low64 - tmp * power;
                low64 = tmp;

                // Round half-up, tie-to-odd.
                let half = power >> 1;
                if remainder >= half && (remainder > half || (low64 & 1) > 0) {
                    low64 += 1;
                }
                scale = Decimal::MAX_SCALE;
            }

            return CalculationResult::Ok(Decimal::from_parts(
                low64 as u32,
                (low64 >> 32) as u32,
                0,
                negative,
                scale,
            ));
        }
        // Left is 32-bit, right is 64- or 96-bit.
        mul_by_32bit_lhs(d1.lo() as u64, d2, &mut product);
    } else if (d2.hi() | d2.mid()) == 0 {
        // Right is 32-bit, left is 64- or 96-bit.
        mul_by_32bit_lhs(d2.lo() as u64, d1, &mut product);
    } else {
        // ── Full 96×96 long multiplication (9 partial products) ──────────────

        // 1: lo × lo
        let mut tmp = d1.lo() as u64 * d2.lo() as u64;
        product.data[0] = tmp as u32;

        // 2: lo × mid
        let mut tmp2 = (d1.lo() as u64 * d2.mid() as u64).wrapping_add(tmp >> 32);

        // 3: mid × lo  (accumulate with step 2)
        tmp = d1.mid() as u64 * d2.lo() as u64;
        tmp = tmp.wrapping_add(tmp2);
        product.data[1] = tmp as u32;

        // Detect carry from the wrapping add.
        tmp2 = if tmp < tmp2 {
            (tmp >> 32) | (1u64 << 32)
        } else {
            tmp >> 32
        };

        // 4: mid × mid
        tmp = d1.mid() as u64 * d2.mid() as u64 + tmp2;

        if (d1.hi() | d2.hi()) > 0 {
            // 5: lo × hi
            tmp2 = d1.lo() as u64 * d2.hi() as u64;
            tmp = tmp.wrapping_add(tmp2);
            // Branchless carry detection.
            let mut tmp3 = (tmp < tmp2) as u64;

            // 6: hi × lo
            tmp2 = d1.hi() as u64 * d2.lo() as u64;
            tmp = tmp.wrapping_add(tmp2);
            product.data[2] = tmp as u32;
            tmp3 += (tmp < tmp2) as u64;

            tmp2 = (tmp3 << 32) | (tmp >> 32);

            // 7: mid × hi
            tmp = d1.mid() as u64 * d2.hi() as u64;
            tmp = tmp.wrapping_add(tmp2);
            tmp3 = (tmp < tmp2) as u64;

            // 8: hi × mid
            tmp2 = d1.hi() as u64 * d2.mid() as u64;
            tmp = tmp.wrapping_add(tmp2);
            product.data[3] = tmp as u32;
            tmp3 += (tmp < tmp2) as u64;

            tmp = (tmp3 << 32) | (tmp >> 32);

            // 9: hi × hi
            product.set_high64(d1.hi() as u64 * d2.hi() as u64 + tmp);
        } else {
            product.set_mid64(tmp);
        }
    }

    // Rescale if the product exceeds 96 bits or MAX_SCALE.
    let upper_word = product.upper_word();
    if upper_word > 2 || scale > Decimal::MAX_SCALE {
        scale = match product.rescale(upper_word, scale) {
            Some(s) => s,
            None => return CalculationResult::Overflow,
        };
    }

    CalculationResult::Ok(Decimal::from_parts(
        product.data[0],
        product.data[1],
        product.data[2],
        negative,
        scale,
    ))
}

/// Multiply a 64- or 96-bit `Decimal` by a 32-bit LHS (`d1` as `u64`).
#[inline(always)]
const fn mul_by_32bit_lhs(d1: u64, d2: &Decimal, product: &mut Buf24) {
    let mut tmp = d1 * d2.lo() as u64;
    product.data[0] = tmp as u32;

    tmp = (d1 * d2.mid() as u64).wrapping_add(tmp >> 32);
    product.data[1] = tmp as u32;
    tmp >>= 32;

    if d2.hi() > 0 {
        tmp = tmp.wrapping_add(d1 * d2.hi() as u64);
        if tmp > U32_MAX {
            product.set_mid64(tmp);
        } else {
            product.data[2] = tmp as u32;
        }
    } else {
        product.data[2] = tmp as u32;
    }
}
