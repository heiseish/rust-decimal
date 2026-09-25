// Wide (192-bit mantissa) decimal arithmetic for maintaining precision
// in iterative operations like exponentiation by squaring.
//
// A standard Decimal has a 96-bit mantissa (~28.9 decimal digits).
// When multiplying two 96-bit values, the product can be up to 192 bits.
// The existing mul_impl immediately rescales this back to 96 bits, losing
// precision. In iterative multiplication (e.g. powu), this precision loss
// compounds at each step.
//
// DecWide keeps a 192-bit mantissa (~57.8 decimal digits) throughout the
// computation, only truncating to 96 bits at the very end.

use crate::constants::POWERS_10;
use crate::Decimal;

/// Maximum power of 10 that fits in a u32 (10^9 = 1,000,000,000).
/// Duplicated here so `wide.rs` compiles regardless of `legacy-ops`.
const MAX_I32_SCALE: i32 = 9;

/// Extended precision decimal with 192-bit mantissa.
/// Used as an intermediate representation to avoid precision loss in
/// iterative operations.
#[derive(Clone, Debug)]
pub(crate) struct DecWide {
    /// 192-bit mantissa stored as 6 × 32-bit words (little-endian)
    data: [u32; 6],
    scale: u32,
    negative: bool,
}

/// 384-bit buffer for intermediate multiplication results.
struct Buf48 {
    data: [u32; 12],
}

impl DecWide {
    #[inline]
    const fn is_zero(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.data[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    #[inline]
    pub const fn from_decimal(d: &Decimal) -> Self {
        let m = d.mantissa_array3();
        DecWide {
            data: [m[0], m[1], m[2], 0, 0, 0],
            scale: d.scale(),
            negative: d.is_sign_negative(),
        }
    }

    pub fn to_decimal(&self) -> Option<Decimal> {
        let mut data = self.data;
        let mut scale = self.scale as i32;
        let mut upper = upper_word_6(&data);

        if upper <= 2 && scale <= Decimal::MAX_SCALE as i32 {
            return Some(Decimal::from_parts(
                data[0],
                data[1],
                data[2],
                self.negative,
                scale as u32,
            ));
        }

        rescale_buf::<6, 2>(&mut data, &mut upper, &mut scale)?;

        Some(Decimal::from_parts(
            data[0],
            data[1],
            data[2],
            self.negative,
            scale as u32,
        ))
    }

    /// Multiply two DecWide values, keeping 192-bit precision.
    pub fn checked_mul(&self, other: &DecWide) -> Option<DecWide> {
        if self.is_zero() || other.is_zero() {
            return Some(DecWide {
                data: [0; 6],
                scale: 0,
                negative: false,
            });
        }

        let scale = self.scale + other.scale;
        let negative = self.negative ^ other.negative;

        let mut product = Buf48 { data: [0u32; 12] };
        let a = &self.data;
        let b = &other.data;

        let a_upper = upper_word_6(a);
        let b_upper = upper_word_6(b);

        for (i, &a_word) in a.iter().enumerate().take(a_upper + 1) {
            if a_word == 0 {
                continue;
            }
            let mut carry: u64 = 0;
            for (j, &b_word) in b.iter().enumerate().take(b_upper + 1) {
                let pos = i + j;
                carry += (a_word as u64) * (b_word as u64) + (product.data[pos] as u64);
                product.data[pos] = carry as u32;
                carry >>= 32;
            }
            let mut pos = i + b_upper + 1;
            while carry > 0 && pos < 12 {
                carry += product.data[pos] as u64;
                product.data[pos] = carry as u32;
                carry >>= 32;
                pos += 1;
            }
        }

        let mut upper = product.upper_word();
        let mut scale = scale as i32;

        if upper <= 5 {
            let mut data = [0u32; 6];
            data.copy_from_slice(&product.data[..6]);

            let max_wide_scale = 57i32;
            if scale > max_wide_scale {
                let mut excess = scale - max_wide_scale;
                let mut u = upper_word_6(&data);
                while excess > 0 {
                    let power_idx = (excess).min(MAX_I32_SCALE) as usize;
                    let power = POWERS_10[power_idx];
                    div_buf_by_power(&mut data, &mut u, power);
                    excess -= power_idx as i32;
                    scale -= power_idx as i32;
                }
            }

            return Some(DecWide {
                data,
                scale: scale as u32,
                negative,
            });
        }

        rescale_buf::<12, 5>(&mut product.data, &mut upper, &mut scale)?;

        let mut data = [0u32; 6];
        data.copy_from_slice(&product.data[..6]);
        Some(DecWide {
            data,
            scale: scale as u32,
            negative,
        })
    }

    /// Add two DecWide values, keeping 192-bit precision.
    pub fn checked_add(&self, other: &DecWide) -> Option<DecWide> {
        if self.is_zero() {
            return Some(other.clone());
        }
        if other.is_zero() {
            return Some(self.clone());
        }

        if self.negative != other.negative {
            // a + (-b) = a - b: flip other's sign and subtract
            return self.checked_sub_impl(other, !other.negative);
        }

        // Same sign: align scales, then add mantissas
        let (mut a, mut b) = (self.clone(), other.clone());
        align_scales(&mut a, &mut b)?;

        let mut carry = 0u64;
        let mut data = [0u32; 6];
        for (dest, (&a_word, &b_word)) in data.iter_mut().zip(a.data.iter().zip(b.data.iter())) {
            carry += a_word as u64 + b_word as u64;
            *dest = carry as u32;
            carry >>= 32;
        }

        if carry > 0 {
            // Overflow 192 bits - divide by 10 to make room
            let mut buf = [0u32; 7];
            buf[..6].copy_from_slice(&data);
            buf[6] = carry as u32;
            let mut scale = a.scale as i32;
            let mut remainder = 0u32;
            for i in (0..7).rev() {
                let num = (buf[i] as u64) + ((remainder as u64) << 32);
                buf[i] = (num / 10) as u32;
                remainder = (num % 10) as u32;
            }
            scale -= 1;
            if scale < 0 {
                return None;
            }
            data.copy_from_slice(&buf[..6]);
            if remainder >= 5 {
                add_one(&mut data);
            }
            return Some(DecWide {
                data,
                scale: scale as u32,
                negative: a.negative,
            });
        }

        Some(DecWide {
            data,
            scale: a.scale,
            negative: a.negative,
        })
    }

    /// Core subtraction with explicit sign for `other`.
    fn checked_sub_impl(&self, other: &DecWide, other_negative: bool) -> Option<DecWide> {
        if other.is_zero() {
            return Some(self.clone());
        }
        if self.is_zero() {
            return Some(DecWide {
                data: other.data,
                scale: other.scale,
                negative: !other_negative,
            });
        }

        if self.negative != other_negative {
            // Different effective signs: a - (-b) = a + b
            let mut b = other.clone();
            b.negative = self.negative; // same sign as self
            return self.checked_add(&b);
        }

        // Same effective sign: align and subtract
        let (mut a, mut b_val) = (self.clone(), other.clone());
        b_val.negative = other_negative;
        align_scales(&mut a, &mut b_val)?;

        let a_bigger = cmp_data(&a.data, &b_val.data) != core::cmp::Ordering::Less;
        let (big, small, neg) = if a_bigger {
            (&a.data, &b_val.data, a.negative)
        } else {
            (&b_val.data, &a.data, !a.negative)
        };

        let mut borrow = 0i64;
        let mut data = [0u32; 6];
        for i in 0..6 {
            let diff = big[i] as i64 - small[i] as i64 - borrow;
            if diff < 0 {
                data[i] = (diff + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                data[i] = diff as u32;
                borrow = 0;
            }
        }

        Some(DecWide {
            data,
            scale: a.scale,
            negative: neg,
        })
    }

    /// Divide by a small u32 value (for Taylor series: divide by i).
    pub fn checked_div_u32(&self, divisor: u32) -> Option<DecWide> {
        if divisor == 0 {
            return None;
        }
        if self.is_zero() || divisor == 1 {
            return Some(self.clone());
        }

        let mut data = self.data;
        let mut remainder = 0u64;

        for i in (0..6).rev() {
            let num = (data[i] as u64) + (remainder << 32);
            data[i] = (num / divisor as u64) as u32;
            remainder = num % divisor as u64;
        }

        let mut scale = self.scale;

        while remainder > 0 {
            let upper = upper_word_6(&data);
            let used_bits = if upper == 0 && data[0] == 0 {
                0
            } else {
                upper * 32 + (32 - data[upper].leading_zeros() as usize)
            };
            let free_digits = (((192 - used_bits as i32) * 77) >> 8).max(0) as u32;
            let extra_scale = free_digits.min(9);

            if extra_scale == 0 || scale + extra_scale > 57 {
                break;
            }

            let power = POWERS_10[extra_scale as usize];
            let mut carry = 0u64;
            for word in data.iter_mut() {
                carry += *word as u64 * power as u64;
                *word = carry as u32;
                carry >>= 32;
            }
            let rem_scaled = remainder * power as u64;
            let extra_quotient = rem_scaled / divisor as u64;
            remainder = rem_scaled % divisor as u64;
            let mut add_carry = extra_quotient;
            for word in data.iter_mut() {
                add_carry += *word as u64;
                *word = add_carry as u32;
                add_carry >>= 32;
                if add_carry == 0 {
                    break;
                }
            }
            scale += extra_scale;
        }

        // Round
        if remainder > 0 {
            let half = divisor as u64 / 2;
            if remainder > half || (remainder == half && (data[0] & 1) != 0) {
                add_one(&mut data);
            }
        }

        Some(DecWide {
            data,
            scale,
            negative: self.negative,
        })
    }

    /// Check if this value's magnitude is less than or equal to 1e-28.
    /// Uses a fast path that avoids the expensive to_decimal() rescale in most cases.
    #[inline]
    pub const fn magnitude_le_28(&self) -> bool {
        if self.is_zero() {
            return true;
        }
        // value = mantissa * 10^(-scale)
        // We want: mantissa * 10^(-scale) <= 10^(-28)
        // i.e.: mantissa <= 10^(scale - 28)
        //
        // Fast check: if the mantissa fits in one u32 word (< 4.3e9 < 10^10)
        // and scale >= 38, then value < 10^10 * 10^(-38) = 10^(-28). Done.
        //
        // If mantissa fits in two words (< 1.8e19 < 10^20)
        // and scale >= 48, then value < 10^20 * 10^(-48) = 10^(-28). Done.
        let upper = upper_word_6(&self.data);
        let min_scale = match upper {
            0 => 38,
            1 => 48,
            2 => 57,           // 10^29 < 2^97, so 3 words with scale >= 57 → value < 10^(-28)
            _ => return false, // Large mantissa, definitely > 1e-28
        };
        self.scale >= min_scale
    }

    /// Negate in place
    #[inline]
    pub fn negate(&mut self) {
        if !self.is_zero() {
            self.negative = !self.negative;
        }
    }

    pub const fn one() -> DecWide {
        DecWide::from_decimal(&Decimal::ONE)
    }
}

#[inline]
fn add_one<const N: usize>(data: &mut [u32; N]) {
    let mut carry = 1u64;
    for word in data.iter_mut() {
        carry += *word as u64;
        *word = carry as u32;
        carry >>= 32;
        if carry == 0 {
            break;
        }
    }
}

fn align_scales(a: &mut DecWide, b: &mut DecWide) -> Option<()> {
    if a.scale == b.scale {
        return Some(());
    }

    let (smaller, larger_scale) = if a.scale < b.scale {
        (&mut *a, b.scale)
    } else {
        (&mut *b, a.scale)
    };

    let diff = larger_scale - smaller.scale;
    let mut remaining = diff;
    while remaining > 0 {
        let step = remaining.min(MAX_I32_SCALE as u32);
        let power = POWERS_10[step as usize];

        let mut carry = 0u64;
        for i in 0..6 {
            carry += smaller.data[i] as u64 * power as u64;
            smaller.data[i] = carry as u32;
            carry >>= 32;
        }

        if carry > 0 {
            return None;
        }

        smaller.scale += step;
        remaining -= step;
    }

    Some(())
}

const fn cmp_data(a: &[u32; 6], b: &[u32; 6]) -> core::cmp::Ordering {
    let mut i = 5;
    loop {
        if a[i] > b[i] {
            return core::cmp::Ordering::Greater;
        }
        if a[i] < b[i] {
            return core::cmp::Ordering::Less;
        }
        if i == 0 {
            return core::cmp::Ordering::Equal;
        }
        i -= 1;
    }
}

impl Buf48 {
    const fn upper_word(&self) -> usize {
        let mut i = 11;
        while i > 0 {
            if self.data[i] > 0 {
                return i;
            }
            i -= 1;
        }
        0
    }
}

const fn upper_word_6(data: &[u32; 6]) -> usize {
    let mut i = 5;
    while i > 0 {
        if data[i] > 0 {
            return i;
        }
        i -= 1;
    }
    0
}

fn div_buf_by_power<const N: usize>(data: &mut [u32; N], upper: &mut usize, power: u32) {
    let mut remainder = 0u32;
    let u = *upper;

    for i in (0..=u).rev() {
        let num = (data[i] as u64) + ((remainder as u64) << 32);
        data[i] = (num / power as u64) as u32;
        remainder = (num as u32).wrapping_sub(data[i].wrapping_mul(power));
    }

    if data[u] == 0 && u > 0 {
        *upper = u - 1;
    }

    let power_half = power >> 1;
    if remainder > power_half || (remainder == power_half && (data[0] & 1) != 0) {
        add_one(data);
    }
}

fn rescale_buf<const N: usize, const TARGET: usize>(
    data: &mut [u32; N],
    upper: &mut usize,
    scale: &mut i32,
) -> Option<()> {
    if *upper <= TARGET && *scale <= Decimal::MAX_SCALE as i32 {
        return Some(());
    }

    let mut rescale_target = if *upper > TARGET {
        let bits = (*upper - TARGET) as i32 * 32 - (data[*upper].leading_zeros() as i32);
        (((bits.max(0)) * 77) >> 8) + 1
    } else {
        0i32
    };

    let max_scale = if TARGET <= 2 { Decimal::MAX_SCALE as i32 } else { 57 };
    if *scale - rescale_target > max_scale {
        rescale_target = *scale - max_scale;
    }

    if rescale_target <= 0 && *upper <= TARGET {
        return Some(());
    }
    if rescale_target > *scale {
        return None;
    }

    let mut sticky = 0u32;
    let mut remainder = 0u32;

    while rescale_target > 0 || *upper > TARGET {
        sticky |= remainder;
        let power_idx = rescale_target.clamp(1, MAX_I32_SCALE) as usize;
        let power = POWERS_10[power_idx];

        remainder = 0;
        for i in (0..=*upper).rev() {
            let num = (data[i] as u64) + ((remainder as u64) << 32);
            data[i] = (num / power as u64) as u32;
            remainder = (num as u32).wrapping_sub(data[i].wrapping_mul(power));
        }

        while *upper > 0 && data[*upper] == 0 {
            *upper -= 1;
        }

        *scale -= power_idx as i32;
        rescale_target -= power_idx as i32;

        if *upper > TARGET && rescale_target <= 0 {
            if *scale <= 0 {
                return None;
            }
            rescale_target = 1;
        }
    }

    let sticky_combined = sticky | remainder;
    if sticky_combined > 0 && remainder > 0 && ((data[0] & 1) != 0 || remainder > 1) {
        let mut carry = true;
        for word in data.iter_mut() {
            if carry {
                *word = word.wrapping_add(1);
                carry = *word == 0;
            } else {
                break;
            }
        }
        if carry || data.get(TARGET + 1).map_or(false, |&w| w > 0) {
            if *scale <= 0 {
                return None;
            }
            let power = POWERS_10[1];
            let mut rem2 = 0u32;
            for i in (0..N).rev() {
                if data[i] > 0 {
                    *upper = i;
                    break;
                }
            }
            for i in (0..=*upper).rev() {
                let num = (data[i] as u64) + ((rem2 as u64) << 32);
                data[i] = (num / power as u64) as u32;
                rem2 = (num as u32).wrapping_sub(data[i].wrapping_mul(power));
            }
            while *upper > 0 && data[*upper] == 0 {
                *upper -= 1;
            }
            *scale -= 1;
        }
    }

    if *scale < 0 || *upper > TARGET {
        return None;
    }

    Some(())
}

/// Exponentiation by squaring using adaptive precision.
///
/// For small exponents (fewer than 10 squarings, i.e. exp < 1024), uses
/// standard 96-bit Decimal arithmetic - fast and sufficient precision (~18+
/// correct digits). For large exponents, uses 192-bit DecWide intermediates
/// to prevent precision loss from compounding over many squarings.
pub(crate) fn powu_wide(base: &Decimal, exp: u64) -> Option<Decimal> {
    if exp == 0 {
        return Some(Decimal::ONE);
    }
    if base.is_zero() {
        return Some(Decimal::ZERO);
    }
    if *base == Decimal::ONE {
        return Some(Decimal::ONE);
    }

    match exp {
        1 => Some(*base),
        2 => base.checked_mul(*base),
        _ => {
            // Number of squarings = bit_length - 1.
            // Each squaring in 96-bit loses ~1 decimal digit.
            // With ≤10 squarings (exp < 1024), we keep 18+ correct digits.
            let squarings = 63 - exp.leading_zeros();
            if squarings < 10 {
                powu_narrow(base, exp)
            } else {
                powu_192(base, exp)
            }
        }
    }
}

/// Fast path: exponentiation by squaring using 96-bit Decimal.
fn powu_narrow(base: &Decimal, exp: u64) -> Option<Decimal> {
    let mut product = Decimal::ONE;
    let mut mask = exp;
    let mut power = *base;

    for n in 0..(64 - exp.leading_zeros()) {
        if n > 0 {
            power = power.checked_mul(power)?;
            mask >>= 1;
        }
        if mask & 0x01 > 0 {
            product = product.checked_mul(power)?;
        }
    }

    product.normalize_assign();
    Some(product)
}

/// Precise path: exponentiation by squaring using 192-bit DecWide.
fn powu_192(base: &Decimal, exp: u64) -> Option<Decimal> {
    let mut product = DecWide::from_decimal(&Decimal::ONE);
    let mut mask = exp;
    let mut power = DecWide::from_decimal(base);

    for n in 0..(64 - exp.leading_zeros()) {
        if n > 0 {
            power = power.checked_mul(&power)?;
            mask >>= 1;
        }
        if mask & 0x01 > 0 {
            product = product.checked_mul(&power)?;
        }
    }

    let mut result = product.to_decimal()?;
    result.normalize_assign();
    Some(result)
}

/// Compute exp(x) using 192-bit intermediate precision.
///
/// Uses argument reduction: exp(x) = exp(n) * exp(r) where n = floor(x), r = x - n.
/// - exp(n) = e^n via powu squaring in DecWide
/// - exp(r) via Taylor series entirely in DecWide
pub(crate) fn exp_wide(value: &Decimal) -> Option<Decimal> {
    if value.is_zero() {
        return Some(Decimal::ONE);
    }
    if value.is_sign_negative() {
        let mut pos = *value;
        pos.set_sign_positive(true);
        let exp = exp_wide(&pos)?;
        return Decimal::ONE.checked_div(exp);
    }

    let n = value.floor();
    let r = value.checked_sub(n)?;

    // Compute exp(r) via Taylor series in DecWide precision
    let r_wide = DecWide::from_decimal(&r);
    let exp_r = if r.is_zero() {
        DecWide::from_decimal(&Decimal::ONE)
    } else {
        let one_wide = DecWide::from_decimal(&Decimal::ONE);
        let mut result = one_wide.checked_add(&r_wide)?;
        let mut term = r_wide.clone();

        for i in 2..100u32 {
            term = r_wide.checked_mul(&term.checked_div_u32(i)?)?;
            result = result.checked_add(&term)?;

            if term.magnitude_le_28() {
                break;
            }
        }
        result
    };

    if n.is_zero() {
        return exp_r.to_decimal();
    }

    let m = n.mantissa_array3();
    if m[2] != 0 {
        return None;
    }
    let n_u64 = m[0] as u64 + ((m[1] as u64) << 32);

    // Compute e^n in DecWide via squaring.
    // We use the 28-digit Decimal::E (not the 57-digit WIDE_E) because when
    // squared, 28 digits → 56 digits which fits perfectly in DecWide's 192
    // bits (~57.8 digits) with minimal truncation. Starting with 57 digits
    // would overflow to 114 digits on first squaring, losing half immediately.
    let exp_n = {
        let mut product = DecWide::from_decimal(&Decimal::ONE);
        let mut mask = n_u64;
        let mut power = DecWide::from_decimal(&Decimal::E);

        for i in 0..(64 - n_u64.leading_zeros()) {
            if i > 0 {
                power = power.checked_mul(&power)?;
                mask >>= 1;
            }
            if mask & 0x01 > 0 {
                product = product.checked_mul(&power)?;
            }
        }
        product
    };

    let result_wide = exp_n.checked_mul(&exp_r)?;
    let mut result = result_wide.to_decimal()?;
    result.normalize_assign();
    Some(result)
}

// ln(10) = 2.302585092994045684017991454 (scale 27)
const LN10: Decimal = Decimal::from_parts_raw(267849502, 33690064, 124823388, 1769472);

// E_NEG_SIXTEENTHS[j] ~= e^(-j/16) at scale 28; LN_E_NEG_SIXTEENTHS[j] = -ln(E_NEG_SIXTEENTHS[j]) exactly rounded.
const E_NEG_SIXTEENTHS: [Decimal; 38] = [
    Decimal::from_parts(0x10000000, 0x3e250261, 0x204fce5e, false, 28),
    Decimal::from_parts(0x2fe85416, 0xc825189f, 0x1e5aa489, false, 28),
    Decimal::from_parts(0x7a51f958, 0x7f39a6fd, 0x1c83d7e1, false, 28),
    Decimal::from_parts(0x2fa73635, 0x7e1240e5, 0x1ac99171, false, 28),
    Decimal::from_parts(0xe76938e, 0x783f377e, 0x192a16ce, false, 28),
    Decimal::from_parts(0x46ecf1dd, 0x2a05a8ff, 0x17a3c85b, false, 28),
    Decimal::from_parts(0x91271e5f, 0x98782dec, 0x16351fa8, false, 28),
    Decimal::from_parts(0xb25b4fc1, 0x81f19671, 0x14dcadef, false, 28),
    Decimal::from_parts(0x4677db56, 0x7841a58e, 0x13991aa1, false, 28),
    Decimal::from_parts(0x1d3e4c42, 0x358762a1, 0x12692210, false, 28),
    Decimal::from_parts(0xca4f66b8, 0xd2f121c1, 0x114b9429, false, 28),
    Decimal::from_parts(0x7ed637a2, 0x9d7e069a, 0x103f5348, false, 28),
    Decimal::from_parts(0xd6c8a765, 0x587c697c, 0xf435315, false, 28),
    Decimal::from_parts(0xac93ba2c, 0xcfefcc80, 0xe56977a, false, 28),
    Decimal::from_parts(0xabdccb2f, 0xae5a676b, 0xd7833a9, false, 28),
    Decimal::from_parts(0xacd240e5, 0x99ab0fb5, 0xca7492b, false, 28),
    Decimal::from_parts(0x8e19db46, 0xaa58ac52, 0xbe30704, false, 28),
    Decimal::from_parts(0xb428261a, 0x5e0fc4ae, 0xb2aa8e2, false, 28),
    Decimal::from_parts(0xa055561f, 0x34d36c11, 0xa7d7657, false, 28),
    Decimal::from_parts(0xc1b60e87, 0x341e4c33, 0x9dac222, false, 28),
    Decimal::from_parts(0xa31964fa, 0x97778fd1, 0x941e981, false, 28),
    Decimal::from_parts(0xdc88f671, 0x11dd062, 0x8b25390, false, 28),
    Decimal::from_parts(0x9facf336, 0x87eb2027, 0x82b70ab, false, 28),
    Decimal::from_parts(0x7def0ab5, 0x9735552, 0x7acb9e6, false, 28),
    Decimal::from_parts(0x3fe6dc64, 0x30a2bb0b, 0x735b07e, false, 28),
    Decimal::from_parts(0xd5093e0, 0xa9d88729, 0x6c5dd60, false, 28),
    Decimal::from_parts(0xa7fbc615, 0x5a69dc1, 0x65cd0b1, false, 28),
    Decimal::from_parts(0xe39f45dc, 0xd30f74e1, 0x5fa2159, false, 28),
    Decimal::from_parts(0xfcf18deb, 0x815302dd, 0x59d6ca3, false, 28),
    Decimal::from_parts(0x1d697e3f, 0xa023c159, 0x54655d1, false, 28),
    Decimal::from_parts(0x996ceb6e, 0x1b7bbf1a, 0x4f485c6, false, 28),
    Decimal::from_parts(0x318eb678, 0x1645da7d, 0x4a7aaaa, false, 28),
    Decimal::from_parts(0x3c206b46, 0xcae8a58, 0x45f779c, false, 28),
    Decimal::from_parts(0x236191d1, 0xec37b378, 0x41ba462, false, 28),
    Decimal::from_parts(0xfbf8a744, 0xd4a1359b, 0x3dbed25, false, 28),
    Decimal::from_parts(0xdc10796f, 0x3961130f, 0x3a01228, false, 28),
    Decimal::from_parts(0xa1fdc8fc, 0x1fc702fa, 0x367d78a, false, 28),
    Decimal::from_parts(0x6ced981f, 0x3a04419c, 0x333050c, false, 28),
];
const LN_E_NEG_SIXTEENTHS: [Decimal; 38] = [
    Decimal::from_parts(0x0, 0x0, 0x0, false, 28),
    Decimal::from_parts(0x11000000, 0xe3e25026, 0x204fce5, false, 28),
    Decimal::from_parts(0x22000000, 0xc7c4a04c, 0x409f9cb, false, 28),
    Decimal::from_parts(0x33000001, 0xaba6f072, 0x60ef6b1, false, 28),
    Decimal::from_parts(0x44000000, 0x8f894098, 0x813f397, false, 28),
    Decimal::from_parts(0x55000000, 0x736b90be, 0xa18f07d, false, 28),
    Decimal::from_parts(0x66000001, 0x574de0e4, 0xc1ded63, false, 28),
    Decimal::from_parts(0x76ffffff, 0x3b30310a, 0xe22ea49, false, 28),
    Decimal::from_parts(0x88000000, 0x1f128130, 0x1027e72f, false, 28),
    Decimal::from_parts(0x99000001, 0x2f4d156, 0x122ce415, false, 28),
    Decimal::from_parts(0xaa000000, 0xe6d7217c, 0x1431e0fa, false, 28),
    Decimal::from_parts(0xbb000001, 0xcab971a2, 0x1636dde0, false, 28),
    Decimal::from_parts(0xcc000001, 0xae9bc1c8, 0x183bdac6, false, 28),
    Decimal::from_parts(0xdd000000, 0x927e11ee, 0x1a40d7ac, false, 28),
    Decimal::from_parts(0xee000001, 0x76606214, 0x1c45d492, false, 28),
    Decimal::from_parts(0xfeffffff, 0x5a42b23a, 0x1e4ad178, false, 28),
    Decimal::from_parts(0xfffffff, 0x3e250261, 0x204fce5e, false, 28),
    Decimal::from_parts(0x21000000, 0x22075287, 0x2254cb44, false, 28),
    Decimal::from_parts(0x31ffffff, 0x5e9a2ad, 0x2459c82a, false, 28),
    Decimal::from_parts(0x43000001, 0xe9cbf2d3, 0x265ec50f, false, 28),
    Decimal::from_parts(0x54000002, 0xcdae42f9, 0x2863c1f5, false, 28),
    Decimal::from_parts(0x65000000, 0xb190931f, 0x2a68bedb, false, 28),
    Decimal::from_parts(0x76000001, 0x9572e345, 0x2c6dbbc1, false, 28),
    Decimal::from_parts(0x87000001, 0x7955336b, 0x2e72b8a7, false, 28),
    Decimal::from_parts(0x97fffffe, 0x5d378391, 0x3077b58d, false, 28),
    Decimal::from_parts(0xa8ffffff, 0x4119d3b7, 0x327cb273, false, 28),
    Decimal::from_parts(0xba000000, 0x24fc23dd, 0x3481af59, false, 28),
    Decimal::from_parts(0xcb000001, 0x8de7403, 0x3686ac3f, false, 28),
    Decimal::from_parts(0xdbfffffe, 0xecc0c429, 0x388ba924, false, 28),
    Decimal::from_parts(0xecfffffe, 0xd0a3144f, 0x3a90a60a, false, 28),
    Decimal::from_parts(0xfe000001, 0xb4856475, 0x3c95a2f0, false, 28),
    Decimal::from_parts(0xf000002, 0x9867b49c, 0x3e9a9fd6, false, 28),
    Decimal::from_parts(0x1ffffffe, 0x7c4a04c2, 0x409f9cbc, false, 28),
    Decimal::from_parts(0x30fffffe, 0x602c54e8, 0x42a499a2, false, 28),
    Decimal::from_parts(0x42000001, 0x440ea50e, 0x44a99688, false, 28),
    Decimal::from_parts(0x53000004, 0x27f0f534, 0x46ae936e, false, 28),
    Decimal::from_parts(0x64000004, 0xbd3455a, 0x48b39054, false, 28),
    Decimal::from_parts(0x75000004, 0xefb59580, 0x4ab88d39, false, 28),
];

/// Compute ln(x) using 192-bit intermediate precision.
///
/// Reduces exactly by a power of ten, then by a tabulated e^(-j/16) so the atanh series
/// ln(z) = 2 * atanh((z-1)/(z+1)) only needs a handful of terms.
pub(crate) fn ln_wide(value: &Decimal) -> Option<Decimal> {
    if value.is_sign_negative() || value.is_zero() {
        return None;
    }
    if *value == Decimal::ONE {
        return Some(Decimal::ZERO);
    }

    // value = x * 10^k with x in [1, 10); exact, only the scale changes.
    let mantissa = value.mantissa().unsigned_abs();
    let digits = mantissa.ilog10();
    let k = digits as i32 - value.scale() as i32;
    let x = Decimal::from_i128_with_scale(mantissa as i128, digits);
    let k_ln10 = if k != 0 {
        Decimal::new(k as i64, 0).checked_mul(LN10)?
    } else {
        Decimal::ZERO
    };
    if x == Decimal::ONE {
        let mut out = k_ln10;
        out.normalize_assign();
        return Some(out);
    }

    // z = x * e^(-j/16) with j ~= 16 ln(x), so |z - 1| < 1/31.
    let j = ((x.as_f64().ln() * 16.0).round() as usize).min(E_NEG_SIXTEENTHS.len() - 1);
    let z = x.checked_mul(E_NEG_SIXTEENTHS[j])?;
    let ln_table = LN_E_NEG_SIXTEENTHS[j];

    let z_wide = DecWide::from_decimal(&z);
    let one_wide = DecWide::one();
    let z_minus_1 = z_wide.checked_sub_impl(&one_wide, false)?;
    let ln_z = if z_minus_1.is_zero() {
        Decimal::ZERO
    } else {
        let z_plus_1 = z_wide.checked_add(&one_wide)?;
        // There is no wide division, so the one division happens in Decimal.
        let y_dec = z_minus_1.to_decimal()?.checked_div(z_plus_1.to_decimal()?)?;
        let y = DecWide::from_decimal(&y_dec);
        let y2 = y.checked_mul(&y)?;

        // atanh(y) = y + y³/3 + y⁵/5 + ...
        let mut result = y.clone();
        let mut term = y;
        for n in 1..100u32 {
            term = term.checked_mul(&y2)?;
            let contribution = term.checked_div_u32(2 * n + 1)?;
            result = result.checked_add(&contribution)?;
            if contribution.magnitude_le_28() {
                break;
            }
        }
        DecWide::from_decimal(&Decimal::TWO)
            .checked_mul(&result)?
            .to_decimal()?
    };

    let mut out = k_ln10.checked_add(ln_table)?.checked_add(ln_z)?;
    out.normalize_assign();
    Some(out)
}

/// Compute sin(x) using 192-bit intermediate precision.
pub(crate) fn sin_wide(value: &Decimal) -> Option<Decimal> {
    if value.is_zero() {
        return Some(Decimal::ZERO);
    }
    if value.is_sign_negative() {
        return sin_wide(&(-*value)).map(|x| -x);
    }
    if *value >= Decimal::TWO_PI {
        let adjusted = value.checked_rem(Decimal::TWO_PI)?;
        return sin_wide(&adjusted);
    }
    if *value >= Decimal::PI {
        return sin_wide(&(*value - Decimal::PI)).map(|x| -x);
    }
    if *value > Decimal::QUARTER_PI {
        return cos_wide(&(Decimal::HALF_PI - *value));
    }

    let x_wide = DecWide::from_decimal(value);
    let x2 = x_wide.checked_mul(&x_wide)?;

    // sin(x) = x - x³/3! + x⁵/5! - ...
    // term_{n+1} = -term_n * x² / ((2n+2)(2n+3))
    let mut result = x_wide.clone();
    let mut term = x_wide;

    for n in 0..50u32 {
        let d = (2 * n + 2) * (2 * n + 3);
        term = term.checked_mul(&x2)?.checked_div_u32(d)?;
        term.negate();
        result = result.checked_add(&term)?;

        if term.magnitude_le_28() {
            break;
        }
    }

    let mut out = result.to_decimal()?;
    out.normalize_assign();
    Some(out)
}

/// Compute cos(x) using 192-bit intermediate precision.
pub(crate) fn cos_wide(value: &Decimal) -> Option<Decimal> {
    if value.is_zero() {
        return Some(Decimal::ONE);
    }
    if value.is_sign_negative() {
        return cos_wide(&(-*value));
    }
    if *value >= Decimal::TWO_PI {
        let adjusted = value.checked_rem(Decimal::TWO_PI)?;
        return cos_wide(&adjusted);
    }
    if *value >= Decimal::PI {
        return cos_wide(&(*value - Decimal::PI)).map(|x| -x);
    }
    if *value > Decimal::QUARTER_PI {
        return sin_wide(&(Decimal::HALF_PI - *value));
    }

    let x_wide = DecWide::from_decimal(value);
    let x2 = x_wide.checked_mul(&x_wide)?;

    // cos(x) = 1 - x²/2! + x⁴/4! - ...
    // term_{n+1} = -term_n * x² / ((2n+1)(2n+2))
    let mut result = DecWide::one();
    let mut term = DecWide::one();

    for n in 0..50u32 {
        let d = (2 * n + 1) * (2 * n + 2);
        term = term.checked_mul(&x2)?.checked_div_u32(d)?;
        term.negate();
        result = result.checked_add(&term)?;

        if term.magnitude_le_28() {
            break;
        }
    }

    let mut out = result.to_decimal()?;
    out.normalize_assign();
    Some(out)
}
