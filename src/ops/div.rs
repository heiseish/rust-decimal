use crate::constants::{MAX_SCALE_I32, POWERS_10};
use crate::decimal::{CalculationResult, Decimal};
use crate::ops::common::{Buf12, Buf16, Dec64};

use core::ops::BitXor;

impl Buf12 {
    /// Add a 32-bit value into the 96-bit buffer.
    /// Returns `Err(DivError::Overflow)` if the result exceeds 96 bits.
    #[inline(always)]
    const fn add32(&mut self, value: u32) -> Result<(), DivError> {
        let value = value as u64;
        let new = self.low64().wrapping_add(value);
        self.set_low64(new);
        if new < value {
            self.data[2] = self.data[2].wrapping_add(1);
            if self.data[2] == 0 {
                return Err(DivError::Overflow);
            }
        }
        Ok(())
    }

    /// Divide the 96-bit value by a 32-bit divisor in-place.
    /// Returns the 32-bit remainder.
    #[inline]
    const fn div32(&mut self, divisor: u32) -> u32 {
        let d = divisor as u64;
        if self.data[2] != 0 {
            let high64 = self.high64();
            let q_high = high64 / d;
            self.set_high64(q_high);
            let rem = high64 % d;
            let combined = (rem << 32) | (self.data[0] as u64);
            if combined == 0 {
                return 0;
            }
            let q_lo = (combined / d) as u32;
            self.data[0] = q_lo;
            (combined % d) as u32
        } else {
            let low64 = self.low64();
            if low64 == 0 {
                return 0;
            }
            let q = low64 / d;
            self.set_low64(q);
            (low64 % d) as u32
        }
    }

    /// Attempt to divide the 96-bit value by a constant power of 10 exactly.
    /// Returns `true` only if the division has no remainder.
    #[inline(always)]
    const fn div32_const(&mut self, pow: u32) -> bool {
        let pow64 = pow as u64;
        let high64 = self.high64();
        let lo = self.data[0] as u64;
        let q_high = high64 / pow64;
        let rem_high = high64 % pow64;
        let combined = (rem_high << 32) + lo;
        if combined % pow64 != 0 {
            return false;
        }
        self.set_high64(q_high);
        self.data[0] = (combined / pow64) as u32;
        true
    }
}

impl Buf16 {
    /// Partial divide by a 64-bit divisor (must truly require 64 bits).
    /// Returns the 32-bit quotient; `self` is overwritten with the remainder.
    #[inline]
    pub(super) const fn partial_divide_64(&mut self, divisor: u64) -> u32 {
        debug_assert!(divisor > self.mid64());

        if self.data[2] == 0 {
            let low64 = self.low64();
            if low64 < divisor {
                return 0;
            }
            let q = low64 / divisor;
            self.set_low64(low64 - q * divisor);
            return q as u32;
        }

        let divisor_hi32 = (divisor >> 32) as u32;
        if self.data[2] >= divisor_hi32 {
            // Quotient is at most u32::MAX; count down from there.
            let mut low64 = self.low64().wrapping_sub(divisor << 32).wrapping_add(divisor);
            let mut quotient = u32::MAX;
            while low64 >= divisor {
                quotient = quotient.wrapping_sub(1);
                low64 = low64.wrapping_add(divisor);
            }
            self.set_low64(low64);
            return quotient;
        }

        let mid64 = self.mid64();
        let divisor_hi64 = divisor_hi32 as u64;
        if mid64 < divisor_hi64 {
            return 0;
        }

        let mut quotient = mid64 / divisor_hi64;
        let mut remainder = (self.data[0] as u64) | ((mid64 - quotient * divisor_hi64) << 32);
        let product = quotient * (divisor & 0xFFFF_FFFF);
        remainder = remainder.wrapping_sub(product);

        // Correct if we went negative.
        if remainder > product.bitxor(u64::MAX) {
            loop {
                quotient = quotient.wrapping_sub(1);
                remainder = remainder.wrapping_add(divisor);
                if remainder < divisor {
                    break;
                }
            }
        }

        self.set_low64(remainder);
        quotient as u32
    }

    /// Partial divide by a 96-bit divisor.
    /// Returns the 32-bit quotient; `self` is overwritten with the remainder.
    #[inline]
    pub(super) const fn partial_divide_96(&mut self, divisor: &Buf12) -> u32 {
        let dividend = self.high64();
        let divisor_hi = divisor.data[2];
        if dividend < divisor_hi as u64 {
            return 0;
        }

        let mut quo = (dividend / divisor_hi as u64) as u32;
        let mut remainder = (dividend as u32).wrapping_sub(quo.wrapping_mul(divisor_hi));

        let mut prod1 = quo as u64 * divisor.data[0] as u64;
        let mut prod2 = quo as u64 * divisor.data[1] as u64;
        prod2 += prod1 >> 32;
        prod1 = (prod1 & 0xFFFF_FFFF) | (prod2 << 32);
        prod2 >>= 32;

        let mut num = self.low64();
        num = num.wrapping_sub(prod1);
        remainder = remainder.wrapping_sub(prod2 as u32);

        if num > prod1.bitxor(u64::MAX) {
            remainder = remainder.wrapping_sub(1);
            if remainder < (prod2 as u32).bitxor(u32::MAX) {
                self.set_low64(num);
                self.data[2] = remainder;
                return quo;
            }
        } else if remainder <= (prod2 as u32).bitxor(u32::MAX) {
            self.set_low64(num);
            self.data[2] = remainder;
            return quo;
        }

        let prod1 = divisor.low64();
        loop {
            quo = quo.wrapping_sub(1);
            num = num.wrapping_add(prod1);
            remainder = remainder.wrapping_add(divisor_hi);
            if num < prod1 {
                let tmp = remainder;
                remainder = remainder.wrapping_add(1);
                if tmp < divisor_hi {
                    break;
                }
            }
            if remainder < divisor_hi {
                break;
            }
        }

        self.set_low64(num);
        self.data[2] = remainder;
        quo
    }
}

enum DivError {
    Overflow,
}

pub(crate) const fn div_impl(dividend: &Decimal, divisor: &Decimal) -> CalculationResult {
    if divisor.is_zero() {
        return CalculationResult::DivByZero;
    }
    if dividend.is_zero() {
        return CalculationResult::Ok(Decimal::ZERO);
    }

    let dividend = Dec64::new(dividend);
    let divisor = Dec64::new(divisor);

    let mut scale = dividend.scale as i32 - divisor.scale as i32;
    let sign_negative = dividend.negative ^ divisor.negative;
    let mut require_unscale = false;
    let mut quotient = Buf12::from_dec64(&dividend);
    let divisor = Buf12::from_dec64(&divisor);

    // BUG FIX: original `divisor.data[2] | divisor.data[1] == 0` parsed as
    // `divisor.data[2] | (divisor.data[1] == 0)` — type mismatch.
    if (divisor.data[2] | divisor.data[1]) == 0 {
        // ── 32-bit divisor ────────────────────────────────────────────────────
        let divisor32 = divisor.data[0];
        let mut remainder = quotient.div32(divisor32);
        let mut power_scale = 0usize;

        loop {
            if remainder == 0 {
                if scale >= 0 {
                    break;
                }
                power_scale = if (-scale) < 9 { (-scale) as usize } else { 9 };
            } else {
                require_unscale = true;
                let will_overflow = if scale == MAX_SCALE_I32 {
                    true
                } else {
                    match quotient.find_scale(scale) {
                        Some(s) => {
                            power_scale = s;
                        }
                        None => return CalculationResult::Overflow,
                    }
                    power_scale == 0
                };
                if will_overflow {
                    let tmp = remainder << 1;
                    let round = if tmp < remainder {
                        true
                    } else if tmp >= divisor32 {
                        tmp > divisor32 || (quotient.data[0] & 0x1) > 0
                    } else {
                        false
                    };
                    if round {
                        match round_up(&mut quotient, scale) {
                            Ok(s) => scale = s,
                            Err(_) => return CalculationResult::Overflow,
                        }
                    }
                    break;
                }
            }

            let power = POWERS_10[power_scale];
            scale += power_scale as i32;
            if increase_scale(&mut quotient, power as u64) > 0 {
                return CalculationResult::Overflow;
            }

            let rem_scaled = remainder as u64 * power as u64;
            let rem_q = (rem_scaled / divisor32 as u64) as u32;
            remainder = (rem_scaled % divisor32 as u64) as u32;
            if let Err(DivError::Overflow) = quotient.add32(rem_q) {
                match unscale_from_overflow(&mut quotient, scale, remainder != 0) {
                    Ok(adj) => scale = adj,
                    Err(_) => return CalculationResult::Overflow,
                }
                break;
            }
        }
    } else {
        // ── 64- or 96-bit divisor ─────────────────────────────────────────────
        let mut power_scale = if divisor.data[2] == 0 {
            divisor.data[1].leading_zeros()
        } else {
            divisor.data[2].leading_zeros()
        } as usize;

        let mut remainder = Buf16::zero();
        remainder.set_low64(quotient.low64() << power_scale);
        let tmp_high = ((quotient.data[1] as u64) + ((quotient.data[2] as u64) << 32)) >> (32 - power_scale);
        remainder.set_high64(tmp_high);

        let divisor64 = divisor.low64() << power_scale;

        if divisor.data[2] == 0 {
            // ── 64-bit divisor ────────────────────────────────────────────────
            quotient.data[2] = 0;
            let rem_lo = remainder.data[0];
            remainder.data[0] = remainder.data[1];
            remainder.data[1] = remainder.data[2];
            remainder.data[2] = remainder.data[3];
            quotient.data[1] = remainder.partial_divide_64(divisor64);
            remainder.data[2] = remainder.data[1];
            remainder.data[1] = remainder.data[0];
            remainder.data[0] = rem_lo;
            quotient.data[0] = remainder.partial_divide_64(divisor64);

            loop {
                let rem_low64 = remainder.low64();
                if rem_low64 == 0 {
                    if scale >= 0 {
                        break;
                    }
                    power_scale = if (-scale) < 9 { (-scale) as usize } else { 9 };
                } else {
                    require_unscale = true;
                    let will_overflow = if scale == MAX_SCALE_I32 {
                        true
                    } else {
                        match quotient.find_scale(scale) {
                            Some(s) => {
                                power_scale = s;
                            }
                            None => return CalculationResult::Overflow,
                        }
                        power_scale == 0
                    };
                    if will_overflow {
                        let mut tmp = remainder.low64();
                        let round = if (tmp as i64) < 0 {
                            true
                        } else {
                            tmp <<= 1;
                            if tmp > divisor64 {
                                true
                            } else {
                                tmp == divisor64 && quotient.data[0] & 0x1 != 0
                            }
                        };
                        if round {
                            match round_up(&mut quotient, scale) {
                                Ok(s) => scale = s,
                                Err(_) => return CalculationResult::Overflow,
                            }
                        }
                        break;
                    }
                }

                let power = POWERS_10[power_scale];
                scale += power_scale as i32;
                if increase_scale(&mut quotient, power as u64) > 0 {
                    return CalculationResult::Overflow;
                }
                increase_scale64(&mut remainder, power as u64);

                let tmp = remainder.partial_divide_64(divisor64);
                if let Err(DivError::Overflow) = quotient.add32(tmp) {
                    match unscale_from_overflow(&mut quotient, scale, remainder.low64() != 0) {
                        Ok(adj) => scale = adj,
                        Err(_) => return CalculationResult::Overflow,
                    }
                    break;
                }
            }
        } else {
            // ── 96-bit divisor ────────────────────────────────────────────────
            let divisor_mid = divisor.data[1];
            let divisor_hi = divisor.data[2];
            let mut divisor = divisor;
            divisor.set_low64(divisor64);
            divisor.data[2] = ((divisor_mid as u64 + ((divisor_hi as u64) << 32)) >> (32 - power_scale)) as u32;

            let quo = remainder.partial_divide_96(&divisor);
            quotient.set_low64(quo as u64);
            quotient.data[2] = 0;

            loop {
                let mut rem_low64 = remainder.low64();
                if rem_low64 == 0 && remainder.data[2] == 0 {
                    if scale >= 0 {
                        break;
                    }
                    power_scale = if (-scale) < 9 { (-scale) as usize } else { 9 };
                } else {
                    require_unscale = true;
                    let will_overflow = if scale == MAX_SCALE_I32 {
                        true
                    } else {
                        match quotient.find_scale(scale) {
                            Some(s) => {
                                power_scale = s;
                            }
                            None => return CalculationResult::Overflow,
                        }
                        power_scale == 0
                    };
                    if will_overflow {
                        let round = if (remainder.data[2] as i32) < 0 {
                            true
                        } else {
                            let tmp = remainder.data[1] >> 31;
                            rem_low64 <<= 1;
                            remainder.set_low64(rem_low64);
                            remainder.data[2] = (remainder.data[2] << 1) + tmp;
                            let rem_hi = remainder.data[2];
                            let div_hi = divisor.data[2];
                            if rem_hi < div_hi {
                                false
                            } else if rem_hi > div_hi {
                                true
                            } else {
                                let div_low64 = divisor.low64();
                                rem_low64 > div_low64 || (rem_low64 == div_low64 && (quotient.data[0] & 1) != 0)
                            }
                        };
                        if round {
                            match round_up(&mut quotient, scale) {
                                Ok(s) => scale = s,
                                Err(_) => return CalculationResult::Overflow,
                            }
                        }
                        break;
                    }
                }

                let power = POWERS_10[power_scale];
                scale += power_scale as i32;
                if increase_scale(&mut quotient, power as u64) > 0 {
                    return CalculationResult::Overflow;
                }

                let mut tmp_remainder = Buf12 {
                    data: [remainder.data[0], remainder.data[1], remainder.data[2]],
                };
                let overflow = increase_scale(&mut tmp_remainder, power as u64);
                remainder.data[0] = tmp_remainder.data[0];
                remainder.data[1] = tmp_remainder.data[1];
                remainder.data[2] = tmp_remainder.data[2];
                remainder.data[3] = overflow;

                let tmp = remainder.partial_divide_96(&divisor);
                if let Err(DivError::Overflow) = quotient.add32(tmp) {
                    let non_zero = (remainder.low64() | remainder.high64()) != 0;
                    match unscale_from_overflow(&mut quotient, scale, non_zero) {
                        Ok(adj) => scale = adj,
                        Err(_) => return CalculationResult::Overflow,
                    }
                    break;
                }
            }
        }
    }

    if require_unscale {
        scale = unscale(&mut quotient, scale);
    }

    CalculationResult::Ok(Decimal::from_parts(
        quotient.data[0],
        quotient.data[1],
        quotient.data[2],
        sign_negative,
        scale as u32,
    ))
}

/// Multiply a 96-bit `Buf12` by `power`, returning any overflow word.
#[inline(always)]
const fn increase_scale(num: &mut Buf12, power: u64) -> u32 {
    let mut tmp = num.data[0] as u64 * power;
    num.data[0] = tmp as u32;
    tmp >>= 32;
    tmp += num.data[1] as u64 * power;
    num.data[1] = tmp as u32;
    tmp >>= 32;
    tmp += num.data[2] as u64 * power;
    num.data[2] = tmp as u32;
    (tmp >> 32) as u32
}

/// Multiply the low 96 bits of a `Buf16` by `power`.
#[inline(always)]
const fn increase_scale64(num: &mut Buf16, power: u64) {
    let mut tmp = num.data[0] as u64 * power;
    num.data[0] = tmp as u32;
    tmp >>= 32;
    tmp += num.data[1] as u64 * power;
    num.set_mid64(tmp);
}

/// Reverse a scale-up overflow by dividing by 10 and rounding.
/// Uses `%` so the compiler emits one `div` for both quotient and remainder.
const fn unscale_from_overflow(num: &mut Buf12, scale: i32, sticky: bool) -> Result<i32, DivError> {
    let scale = scale - 1;
    if scale < 0 {
        return Err(DivError::Overflow);
    }
    const HIGH_BIT: u64 = 0x1_0000_0000;
    num.data[2] = (HIGH_BIT / 10) as u32;
    let tmp = ((HIGH_BIT % 10) << 32) + num.data[1] as u64;
    let val1 = (tmp / 10) as u32;
    num.data[1] = val1;
    let tmp = ((tmp % 10) << 32) + num.data[0] as u64;
    let val0 = (tmp / 10) as u32;
    num.data[0] = val0;
    let remainder = (tmp % 10) as u32;
    if remainder > 5 || (remainder == 5 && (sticky || num.data[0] & 0x1 > 0)) {
        let _ = num.add32(1);
    }
    Ok(scale)
}

#[inline(always)]
const fn round_up(num: &mut Buf12, scale: i32) -> Result<i32, DivError> {
    let low64 = num.low64().wrapping_add(1);
    num.set_low64(low64);
    if low64 != 0 {
        return Ok(scale);
    }
    let hi = num.data[2].wrapping_add(1);
    num.data[2] = hi;
    if hi != 0 {
        return Ok(scale);
    }
    unscale_from_overflow(num, scale, true)
}

/// Remove trailing decimal zeros, reducing the scale accordingly.
const fn unscale(num: &mut Buf12, scale: i32) -> i32 {
    let mut scale = scale;
    while num.data[0] == 0 && scale >= 8 && num.div32_const(100_000_000) {
        scale -= 8;
    }
    if (num.data[0] & 0xF) == 0 && scale >= 4 && num.div32_const(10_000) {
        scale -= 4;
    }
    if (num.data[0] & 0x3) == 0 && scale >= 2 && num.div32_const(100) {
        scale -= 2;
    }
    if (num.data[0] & 0x1) == 0 && scale >= 1 && num.div32_const(10) {
        scale -= 1;
    }
    scale
}
