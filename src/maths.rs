use crate::prelude::*;
use num_traits::pow::Pow;

// Tolerance for inaccuracies when calculating exp
const EXP_TOLERANCE: Decimal = Decimal::from_parts(2, 0, 0, false, 7);
// Approximation of 1/ln(10) = 0.4342944819032518276511289189
const LN10_INVERSE: Decimal = Decimal::from_parts_raw(1763037029, 1670682625, 235431510, 1835008);
// Total iterations of taylor series for Trig.
const TRIG_SERIES_UPPER_BOUND: usize = 6;
// PI / 8
const EIGHTH_PI: Decimal = Decimal::from_parts_raw(2822163429, 3244459792, 212882598, 1835008);

// Table representing {index}!
const FACTORIAL: [Decimal; 28] = [
    Decimal::from_parts(1, 0, 0, false, 0),
    Decimal::from_parts(1, 0, 0, false, 0),
    Decimal::from_parts(2, 0, 0, false, 0),
    Decimal::from_parts(6, 0, 0, false, 0),
    Decimal::from_parts(24, 0, 0, false, 0),
    // 5!
    Decimal::from_parts(120, 0, 0, false, 0),
    Decimal::from_parts(720, 0, 0, false, 0),
    Decimal::from_parts(5040, 0, 0, false, 0),
    Decimal::from_parts(40320, 0, 0, false, 0),
    Decimal::from_parts(362880, 0, 0, false, 0),
    // 10!
    Decimal::from_parts(3628800, 0, 0, false, 0),
    Decimal::from_parts(39916800, 0, 0, false, 0),
    Decimal::from_parts(479001600, 0, 0, false, 0),
    Decimal::from_parts(1932053504, 1, 0, false, 0),
    Decimal::from_parts(1278945280, 20, 0, false, 0),
    // 15!
    Decimal::from_parts(2004310016, 304, 0, false, 0),
    Decimal::from_parts(2004189184, 4871, 0, false, 0),
    Decimal::from_parts(4006445056, 82814, 0, false, 0),
    Decimal::from_parts(3396534272, 1490668, 0, false, 0),
    Decimal::from_parts(109641728, 28322707, 0, false, 0),
    // 20!
    Decimal::from_parts(2192834560, 566454140, 0, false, 0),
    Decimal::from_parts(3099852800, 3305602358, 2, false, 0),
    Decimal::from_parts(3772252160, 4003775155, 60, false, 0),
    Decimal::from_parts(862453760, 1892515369, 1401, false, 0),
    Decimal::from_parts(3519021056, 2470695900, 33634, false, 0),
    // 25!
    Decimal::from_parts(2076180480, 1637855376, 840864, false, 0),
    Decimal::from_parts(2441084928, 3929534124, 21862473, false, 0),
    Decimal::from_parts(1484783616, 3018206259, 590286795, false, 0),
];

// Pre-computed reciprocals of FACTORIAL entries used in the trig Taylor series.
// sin uses indices 1,3,5,7,9,11 → TRIG_SERIES_UPPER_BOUND=6 terms.
// cos uses indices 0,2,4,6,8,10.
// Storing the reciprocal avoids a division inside the inner loop.
//
// 1/n! values (same precision as the rest of the module):
const INV_FACTORIAL: [Decimal; 12] = [
    // 1/0! = 1
    Decimal::from_parts(1, 0, 0, false, 0),
    // 1/1! = 1
    Decimal::from_parts(1, 0, 0, false, 0),
    // 1/2! = 0.5
    Decimal::from_parts(5, 0, 0, false, 1),
    // 1/3! = 0.1666666666666666666666666667
    Decimal::from_parts_raw(2576980378, 2576980377, 452312848, 1835008),
    // 1/4! = 0.0416666666666666666666666667
    Decimal::from_parts_raw(2576980378, 643745094, 113078212, 1835008),
    // 1/5! = 0.0083333333333333333333333333
    Decimal::from_parts_raw(3355443133, 3355443200, 22615642, 1835008),
    // 1/6! = 0.0013888888888888888888888889
    Decimal::from_parts_raw(2938661075, 3149642985, 3769273, 1835008),
    // 1/7! = 0.0001984126984126984126984127
    Decimal::from_parts_raw(1382505448, 3184756790, 538467, 1835008),
    // 1/8! = 0.0000248015873015873015873016
    Decimal::from_parts_raw(3427044488, 1487877824, 67308, 1835008),
    // 1/9! = 0.0000027557319223985890652557
    Decimal::from_parts_raw(2424453529, 3900820826, 7478, 1835008),
    // 1/10! = 0.0000002755731922398589065256
    Decimal::from_parts_raw(3855180338, 680975768, 748, 1835008),
    // 1/11! = 0.0000000250521083854417187750
    Decimal::from_parts_raw(3680820889, 1907100994, 68, 1835008),
];

pub const trait MathematicalOps {
    /// The estimated exponential function, e<sup>x</sup>. Stops calculating when it is within
    /// tolerance of roughly `0.0000002`.
    fn exp(&self) -> Decimal;

    /// The estimated exponential function, e<sup>x</sup>. Stops calculating when it is within
    /// tolerance of roughly `0.0000002`. Returns `None` on overflow.
    fn checked_exp(&self) -> Option<Decimal>;

    /// The estimated exponential function, e<sup>x</sup> using the `tolerance` provided as a hint
    /// as to when to stop calculating. A larger tolerance will cause the number to stop calculating
    /// sooner at the potential cost of a slightly less accurate result.
    fn exp_with_tolerance(&self, tolerance: Decimal) -> Decimal;

    /// The estimated exponential function, e<sup>x</sup> using the `tolerance` provided as a hint
    /// as to when to stop calculating. A larger tolerance will cause the number to stop calculating
    /// sooner at the potential cost of a slightly less accurate result.
    /// Returns `None` on overflow.
    fn checked_exp_with_tolerance(&self, tolerance: Decimal) -> Option<Decimal>;

    /// Raise self to the given integer exponent: x<sup>y</sup>
    fn powi(&self, exp: i64) -> Decimal;

    /// Raise self to the given integer exponent x<sup>y</sup> returning `None` on overflow.
    fn checked_powi(&self, exp: i64) -> Option<Decimal>;

    /// Raise self to the given unsigned integer exponent: x<sup>y</sup>
    fn powu(&self, exp: u64) -> Decimal;

    /// Raise self to the given unsigned integer exponent x<sup>y</sup> returning `None` on overflow.
    fn checked_powu(&self, exp: u64) -> Option<Decimal>;

    /// Raise self to the given floating point exponent: x<sup>y</sup>
    fn powf(&self, exp: f64) -> Decimal;

    /// Raise self to the given floating point exponent x<sup>y</sup> returning `None` on overflow.
    fn checked_powf(&self, exp: f64) -> Option<Decimal>;

    /// Raise self to the given Decimal exponent: x<sup>y</sup>. If `exp` is not whole then the approximation
    /// e<sup>y*ln(x)</sup> is used.
    fn powd(&self, exp: Decimal) -> Decimal;

    /// Raise self to the given Decimal exponent x<sup>y</sup> returning `None` on overflow.
    /// If `exp` is not whole then the approximation e<sup>y*ln(x)</sup> is used.
    fn checked_powd(&self, exp: Decimal) -> Option<Decimal>;

    /// The square root of a Decimal. Uses a standard Babylonian method.
    fn sqrt(&self) -> Option<Decimal>;

    /// Calculates the natural logarithm for a Decimal calculated using Taylor's series.
    fn ln(&self) -> Decimal;

    /// Calculates the checked natural logarithm for a Decimal calculated using Taylor's series.
    /// Returns `None` for negative numbers or zero.
    fn checked_ln(&self) -> Option<Decimal>;

    /// Calculates the base 10 logarithm of a specified Decimal number.
    fn log10(&self) -> Decimal;

    /// Calculates the checked base 10 logarithm of a specified Decimal number.
    /// Returns `None` for negative numbers or zero.
    fn checked_log10(&self) -> Option<Decimal>;

    /// Abramowitz Approximation of Error Function from [wikipedia](https://en.wikipedia.org/wiki/Error_function#Numerical_approximations)
    fn erf(&self) -> Decimal;

    /// The Cumulative distribution function for a Normal distribution
    fn norm_cdf(&self) -> Decimal;

    /// The Probability density function for a Normal distribution.
    fn norm_pdf(&self) -> Decimal;

    /// The Probability density function for a Normal distribution returning `None` on overflow.
    fn checked_norm_pdf(&self) -> Option<Decimal>;

    /// Computes the sine of a number (in radians).
    /// Panics upon overflow.
    fn sin(&self) -> Decimal;

    /// Computes the checked sine of a number (in radians).
    fn checked_sin(&self) -> Option<Decimal>;

    /// Computes the cosine of a number (in radians).
    /// Panics upon overflow.
    fn cos(&self) -> Decimal;

    /// Computes the checked cosine of a number (in radians).
    fn checked_cos(&self) -> Option<Decimal>;

    /// Computes the tangent of a number (in radians).
    /// Panics upon overflow or upon approaching a limit.
    fn tan(&self) -> Decimal;

    /// Computes the checked tangent of a number (in radians).
    /// Returns None on limit.
    fn checked_tan(&self) -> Option<Decimal>;
}

impl MathematicalOps for Decimal {
    #[inline]
    fn exp(&self) -> Decimal {
        self.exp_with_tolerance(EXP_TOLERANCE)
    }

    #[inline]
    fn checked_exp(&self) -> Option<Decimal> {
        self.checked_exp_with_tolerance(EXP_TOLERANCE)
    }

    #[inline]
    fn exp_with_tolerance(&self, tolerance: Decimal) -> Decimal {
        match self.checked_exp_with_tolerance(tolerance) {
            Some(d) => d,
            None => {
                if self.is_sign_negative() {
                    panic!("Exp underflowed")
                } else {
                    panic!("Exp overflowed")
                }
            }
        }
    }

    fn checked_exp_with_tolerance(&self, tolerance: Decimal) -> Option<Decimal> {
        if self.is_zero() {
            return Some(Decimal::ONE);
        }
        if self.is_sign_negative() {
            let mut flipped = *self;
            flipped.set_sign_positive(true);
            let exp = flipped.checked_exp_with_tolerance(tolerance)?;
            return Decimal::ONE.checked_div(exp);
        }

        // exp(x) = Σ x^i / i!  where q_i = q_{i-1} * x / i
        // Avoids computing large intermediate powers: q_i = x*(x/2)*...*(x/i)

        // First two terms: result = 1 + x, q_1 = x
        let mut result = self.checked_add(Decimal::ONE)?;
        let mut term = *self;

        // Accumulate the divisor as a Decimal to avoid repeated `from_u32` conversions.
        // We start at i=2 so the initial divisor is 2.
        let mut i_dec = Decimal::TWO;
        let one = Decimal::ONE;

        const ITERATION_COUNT: u32 = 200;
        for _ in 2..ITERATION_COUNT {
            term = self.checked_mul(term.checked_div(i_dec)?)?;
            result = result.checked_add(term)?;
            if term <= tolerance {
                break;
            }
            // Increment the divisor for the next iteration — a single addition is cheaper
            // than calling `from_u32` + `unwrap` on every pass.
            i_dec = i_dec.checked_add(one)?;
        }

        Some(result)
    }

    #[inline]
    fn powi(&self, exp: i64) -> Decimal {
        match self.checked_powi(exp) {
            Some(result) => result,
            None => panic!("Pow overflowed"),
        }
    }

    #[inline]
    fn checked_powi(&self, exp: i64) -> Option<Decimal> {
        if exp >= 0 {
            return self.checked_powu(exp as u64);
        }
        let exp = exp.unsigned_abs();
        let pow = self.checked_powu(exp)?;
        Decimal::ONE.checked_div(pow)
    }

    #[inline]
    fn powu(&self, exp: u64) -> Decimal {
        match self.checked_powu(exp) {
            Some(result) => result,
            None => panic!("Pow overflowed"),
        }
    }

    fn checked_powu(&self, exp: u64) -> Option<Decimal> {
        match exp {
            0 => Some(Decimal::ONE),
            1 => Some(*self),
            2 => self.checked_mul(*self),
            _ => {
                if self.is_zero() {
                    return Some(Decimal::ZERO);
                }
                if self.is_one() {
                    return Some(Decimal::ONE);
                }

                // Binary (fast) exponentiation:
                // iterate over each bit of `exp` from LSB to MSB, squaring `power`
                // each step and accumulating into `product` when the bit is set.
                let mut product = Decimal::ONE;
                let mut mask = exp;
                let mut power = *self;
                let bit_count = 64 - exp.leading_zeros();

                for n in 0..bit_count {
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
        }
    }

    #[inline]
    fn powf(&self, exp: f64) -> Decimal {
        match self.checked_powf(exp) {
            Some(result) => result,
            None => panic!("Pow overflowed"),
        }
    }

    #[inline]
    fn checked_powf(&self, exp: f64) -> Option<Decimal> {
        let exp = Decimal::from_f64(exp)?;
        self.checked_powd(exp)
    }

    #[inline]
    fn powd(&self, exp: Decimal) -> Decimal {
        match self.checked_powd(exp) {
            Some(result) => result,
            None => panic!("Pow overflowed"),
        }
    }

    fn checked_powd(&self, exp: Decimal) -> Option<Decimal> {
        if exp.is_zero() {
            return Some(Decimal::ONE);
        }
        if self.is_zero() {
            return Some(Decimal::ZERO);
        }
        if self.is_one() {
            return Some(Decimal::ONE);
        }
        if exp.is_one() {
            return Some(*self);
        }

        let exp = exp.normalize();
        if exp.scale() == 0 {
            if exp.mid() != 0 || exp.hi() != 0 {
                return None; // Exponent too large
            }
            return if exp.is_sign_negative() {
                self.checked_powi(-(exp.lo() as i64))
            } else {
                self.checked_powu(exp.lo() as u64)
            };
        }

        // For fractional exponent: a^b = exp(b * ln(a))
        let negative = self.is_sign_negative();
        let e = self.abs().ln().checked_mul(exp)?;
        let mut result = e.checked_exp()?;
        result.set_sign_negative(negative);
        Some(result)
    }

    fn sqrt(&self) -> Option<Decimal> {
        if self.is_sign_negative() {
            return None;
        }
        if self.is_zero() {
            return Some(Decimal::ZERO);
        }

        // Babylonian / Newton–Raphson: x_{n+1} = (x_n + S/x_n) / 2
        let mut result = self / Decimal::TWO;
        if result.is_zero() {
            result = *self;
        }
        let mut last = result + Decimal::ONE;

        let mut circuit_breaker = 0;
        while last != result {
            circuit_breaker += 1;
            assert!(circuit_breaker < 1000, "sqrt circuit breaker");
            last = result;
            result = (result + self / result) / Decimal::TWO;
        }
        Some(result)
    }

    #[cfg(feature = "maths-nopanic")]
    fn ln(&self) -> Decimal {
        match self.checked_ln() {
            Some(result) => result,
            None => Decimal::ZERO,
        }
    }

    #[cfg(not(feature = "maths-nopanic"))]
    fn ln(&self) -> Decimal {
        match self.checked_ln() {
            Some(result) => result,
            None => {
                if self.is_sign_negative() {
                    panic!("Unable to calculate ln for negative numbers")
                } else if self.is_zero() {
                    panic!("Unable to calculate ln for zero")
                } else {
                    panic!("Calculation of ln failed for unknown reasons")
                }
            }
        }
    }

    fn checked_ln(&self) -> Option<Decimal> {
        if self.is_sign_negative() || self.is_zero() {
            return None;
        }
        if self.is_one() {
            return Some(Decimal::ZERO);
        }

        // Range-reduce into (e^-1, 1) then apply Taylor series for ln(1+x).
        let mut x = *self;
        let mut count = 0i64;
        while x >= Decimal::ONE {
            x *= Decimal::E_INVERSE;
            count += 1;
        }
        while x <= Decimal::E_INVERSE {
            x *= Decimal::E;
            count -= 1;
        }
        x -= Decimal::ONE;
        if x.is_zero() {
            return Some(Decimal::new(count, 0));
        }

        // ln(1+x) = x - x²/2 + x³/3 - …  (Mercator series, x shifted above)
        let mut result = Decimal::ZERO;
        let mut iteration = 0i64;
        let mut y = Decimal::ONE;
        let mut last = Decimal::ONE;
        while last != result && iteration < 100 {
            iteration += 1;
            last = result;
            y *= -x;
            result += y / Decimal::new(iteration, 0);
        }
        Some(Decimal::new(count, 0) - result)
    }

    #[cfg(feature = "maths-nopanic")]
    fn log10(&self) -> Decimal {
        match self.checked_log10() {
            Some(result) => result,
            None => Decimal::ZERO,
        }
    }

    #[cfg(not(feature = "maths-nopanic"))]
    fn log10(&self) -> Decimal {
        match self.checked_log10() {
            Some(result) => result,
            None => {
                if self.is_sign_negative() {
                    panic!("Unable to calculate log10 for negative numbers")
                } else if self.is_zero() {
                    panic!("Unable to calculate log10 for zero")
                } else {
                    panic!("Calculation of log10 failed for unknown reasons")
                }
            }
        }
    }

    fn checked_log10(&self) -> Option<Decimal> {
        use crate::ops::array::{div_by_u32, is_all_zero};

        if self.is_sign_negative() || self.is_zero() {
            return None;
        }
        if self.is_one() {
            return Some(Decimal::ZERO);
        }

        // log10(n) = ln(n) * (1/ln(10))  — use pre-computed constant
        let scale = self.scale();
        let mut working = self.mantissa_array3();

        // Fast exit for exact powers of 10^-scale (e.g. 0.001 = 10^-3).
        if scale > 0 && working[2] == 0 && working[1] == 0 && working[0] == 1 {
            return Some(Decimal::from_parts(scale, 0, 0, true, 0));
        }

        // Detect exact integer powers of 10 by repeated division.
        let mut result = 0i32;
        let mut base10 = true;
        while !is_all_zero(&working) {
            let remainder = div_by_u32(&mut working, 10u32);
            if remainder != 0 {
                base10 = false;
                break;
            }
            result += 1;
            if working[2] == 0 && working[1] == 0 && working[0] == 1 {
                break;
            }
        }
        if base10 {
            return Some((result - scale as i32).into());
        }

        self.checked_ln().map(|result| LN10_INVERSE * result)
    }

    fn erf(&self) -> Decimal {
        if self.is_sign_positive() {
            // Abramowitz & Stegun approximation (maximum error ≈ 1.5×10⁻⁷):
            //   erf(x) ≈ 1 − (a₁t + a₂t² + a₃t³ + a₄t⁴ + a₅t⁵ + a₆t⁶)
            // where the original form had t = 1/(1 + p*x).
            //
            // Here we use the equivalent Horner-form evaluation of the denominator
            // polynomial to avoid redundant `powi` calls and accumulate the sum of
            // x^k * coefficient terms directly via Horner's method:
            //   sum = 1 + x*(a1 + x*(a2 + x*(a3 + x*(a4 + x*(a5 + x*a6)))))
            //
            // Constants (same values as original, reordered for Horner evaluation):
            const A1: Decimal = Decimal::from_parts(705230784, 0, 0, false, 10);
            const A2: Decimal = Decimal::from_parts(422820123, 0, 0, false, 10);
            const A3: Decimal = Decimal::from_parts(92705272, 0, 0, false, 10);
            const A4: Decimal = Decimal::from_parts(1520143, 0, 0, false, 10);
            const A5: Decimal = Decimal::from_parts(2765672, 0, 0, false, 10);
            const A6: Decimal = Decimal::from_parts(430638, 0, 0, false, 10);

            // Horner evaluation of (1 + x*(a1 + x*(a2 + x*(a3 + x*(a4 + x*(a5 + x*a6))))))
            // Starting from the innermost coefficient and working outward:
            let x = self;
            let sum = Decimal::ONE + *x * (A1 + *x * (A2 + *x * (A3 + *x * (A4 + *x * (A5 + *x * A6)))));

            // erf ≈ 1 - 1/sum^16
            Decimal::ONE - (Decimal::ONE / sum.powi(16))
        } else {
            -self.abs().erf()
        }
    }

    #[inline]
    fn norm_cdf(&self) -> Decimal {
        (Decimal::ONE + (self / Decimal::from_parts(2318911239, 3292722, 0, false, 16)).erf()) / Decimal::TWO
    }

    #[inline]
    fn norm_pdf(&self) -> Decimal {
        match self.checked_norm_pdf() {
            Some(d) => d,
            None => panic!("Norm Pdf overflowed"),
        }
    }

    #[inline]
    fn checked_norm_pdf(&self) -> Option<Decimal> {
        let sqrt2pi = Decimal::from_parts_raw(2133383024, 2079885984, 1358845910, 1835008);
        let factor = -self.checked_powi(2)?;
        let factor = factor.checked_div(Decimal::TWO)?;
        factor.checked_exp()?.checked_div(sqrt2pi)
    }

    #[inline]
    fn sin(&self) -> Decimal {
        match self.checked_sin() {
            Some(x) => x,
            None => panic!("Sin overflowed"),
        }
    }

    fn checked_sin(&self) -> Option<Decimal> {
        if self.is_zero() {
            return Some(Decimal::ZERO);
        }
        if self.is_sign_negative() {
            return (-self).checked_sin().map(|x| -x);
        }
        if self >= &Decimal::TWO_PI {
            let adjusted = self.checked_rem(Decimal::TWO_PI)?;
            return adjusted.checked_sin();
        }
        if self >= &Decimal::PI {
            return (self - Decimal::PI).checked_sin().map(|x| -x);
        }
        if self > &Decimal::QUARTER_PI {
            return (Decimal::HALF_PI - self).checked_cos();
        }

        // Taylor series for sin(x), unrolled for TRIG_SERIES_UPPER_BOUND = 6 terms:
        // x^1/1! - x^3/3! + x^5/5! - x^7/7! + x^9/9! - x^11/11!
        //
        // Using pre-computed inverse factorials (INV_FACTORIAL) avoids a division per term.
        // Using Horner / Estrin is tricky for alternating series; direct evaluation is clear
        // and the loop is known-small (6 iterations, always unrolled by the optimizer).
        let x2 = self.checked_mul(*self)?; // x^2, reused across all terms
        let mut result = Decimal::ZERO;
        let mut x_pow = *self; // starts at x^1
        for n in 0..TRIG_SERIES_UPPER_BOUND {
            let idx = 2 * n + 1;
            let element = x_pow.checked_mul(INV_FACTORIAL[idx])?;
            if n & 0x1 == 0 {
                result += element;
            } else {
                result -= element;
            }
            // Advance x^(2n+1) → x^(2n+3) by multiplying by x^2
            if n + 1 < TRIG_SERIES_UPPER_BOUND {
                x_pow = x_pow.checked_mul(x2)?;
            }
        }
        Some(result)
    }

    #[inline]
    fn cos(&self) -> Decimal {
        match self.checked_cos() {
            Some(x) => x,
            None => panic!("Cos overflowed"),
        }
    }

    fn checked_cos(&self) -> Option<Decimal> {
        if self.is_zero() {
            return Some(Decimal::ONE);
        }
        if self.is_sign_negative() {
            return (-self).checked_cos();
        }
        if self >= &Decimal::TWO_PI {
            let adjusted = self.checked_rem(Decimal::TWO_PI)?;
            return adjusted.checked_cos();
        }
        if self >= &Decimal::PI {
            return (self - Decimal::PI).checked_cos().map(|x| -x);
        }
        if self > &Decimal::QUARTER_PI {
            return (Decimal::HALF_PI - self).checked_sin();
        }

        // Taylor series for cos(x), unrolled for TRIG_SERIES_UPPER_BOUND = 6 terms:
        // x^0/0! - x^2/2! + x^4/4! - x^6/6! + x^8/8! - x^10/10!
        let x2 = self.checked_mul(*self)?;
        let mut result = Decimal::ZERO;
        let mut x_pow = Decimal::ONE; // starts at x^0 = 1
        for n in 0..TRIG_SERIES_UPPER_BOUND {
            let idx = 2 * n;
            let element = x_pow.checked_mul(INV_FACTORIAL[idx])?;
            if n & 0x1 == 0 {
                result += element;
            } else {
                result -= element;
            }
            if n + 1 < TRIG_SERIES_UPPER_BOUND {
                x_pow = x_pow.checked_mul(x2)?;
            }
        }
        Some(result)
    }

    #[inline]
    fn tan(&self) -> Decimal {
        match self.checked_tan() {
            Some(x) => x,
            None => panic!("Tan overflowed"),
        }
    }

    fn checked_tan(&self) -> Option<Decimal> {
        if self.is_zero() {
            return Some(Decimal::ZERO);
        }
        if self.is_sign_negative() {
            return (-self).checked_tan().map(|x| -x);
        }
        if self >= &Decimal::TWO_PI {
            let adjusted = self.checked_rem(Decimal::TWO_PI)?;
            return adjusted.checked_tan();
        }
        if self >= &Decimal::PI {
            return (self - Decimal::PI).checked_tan();
        }
        if self > &Decimal::HALF_PI {
            return ((Decimal::HALF_PI - self) + Decimal::HALF_PI).checked_tan().map(|x| -x);
        }
        if self > &Decimal::QUARTER_PI {
            return match (Decimal::HALF_PI - self).checked_tan() {
                Some(x) => Decimal::ONE.checked_div(x),
                None => None,
            };
        }

        // Halve-angle identity for x > PI/8 to improve accuracy:
        //   tan(x) = 2*tan(x/2) / (1 - tan²(x/2))
        if self > &EIGHTH_PI {
            let tan_half = (self / Decimal::TWO).checked_tan()?;
            let dividend = Decimal::TWO.checked_mul(tan_half)?;
            let squared = tan_half.checked_mul(tan_half)?;
            let divisor = Decimal::ONE - squared;
            if divisor.is_zero() {
                return None;
            }
            return dividend.checked_div(divisor);
        }

        // Maclaurin polynomial for 0 ≤ x ≤ PI/8 (accurate to ~10⁻⁸):
        //   x + (1/3)x³ + (2/15)x⁵ + (17/315)x⁷ + (62/2835)x⁹ + (1382/155925)x¹¹
        //
        // Re-expressed via Horner for fewer multiplications:
        //   x * (1 + x²*(1/3 + x²*(2/15 + x²*(17/315 + x²*(62/2835 + x²*(1382/155925))))))
        //
        // Note: the original structure iterated `self.powu(pow)` separately for each term,
        // costing O(log pow) multiplications each time. Using x² as a running accumulator
        // reduces this to 5 multiplications of x² plus the Horner accumulation.
        const C1: Decimal = Decimal::from_parts_raw(89478485, 347537611, 180700362, 1835008); // 1/3
        const C2: Decimal = Decimal::from_parts_raw(894784853, 3574988881, 72280144, 1835008); // 2/15
        const C3: Decimal = Decimal::from_parts_raw(905437054, 3907911371, 2925624, 1769472); // 17/315
        const C4: Decimal = Decimal::from_parts_raw(3191872741, 2108928381, 11855473, 1835008); // 62/2835
        const C5: Decimal = Decimal::from_parts_raw(3482645539, 2612995122, 4804769, 1835008); // 1382/155925
        const C6: Decimal = Decimal::from_parts_raw(4189029078, 2192791200, 1947296, 1835008); // 21844/6081075

        let x2 = self.checked_mul(*self)?;
        // Horner from innermost coefficient outward:
        let series = C1 + x2 * (C2 + x2 * (C3 + x2 * (C4 + x2 * (C5 + x2 * C6))));
        Some(*self * (Decimal::ONE + x2 * series))
    }
}

impl Pow<Decimal> for Decimal {
    type Output = Decimal;
    #[inline]
    fn pow(self, rhs: Decimal) -> Self::Output {
        MathematicalOps::powd(&self, rhs)
    }
}

impl Pow<u64> for Decimal {
    type Output = Decimal;
    #[inline]
    fn pow(self, rhs: u64) -> Self::Output {
        MathematicalOps::powu(&self, rhs)
    }
}

impl Pow<i64> for Decimal {
    type Output = Decimal;
    #[inline]
    fn pow(self, rhs: i64) -> Self::Output {
        MathematicalOps::powi(&self, rhs)
    }
}

impl Pow<f64> for Decimal {
    type Output = Decimal;
    #[inline]
    fn pow(self, rhs: f64) -> Self::Output {
        MathematicalOps::powf(&self, rhs)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;

    #[test]
    fn test_factorials() {
        assert_eq!("1", FACTORIAL[0].to_string(), "0!");
        assert_eq!("1", FACTORIAL[1].to_string(), "1!");
        assert_eq!("2", FACTORIAL[2].to_string(), "2!");
        assert_eq!("6", FACTORIAL[3].to_string(), "3!");
        assert_eq!("24", FACTORIAL[4].to_string(), "4!");
        assert_eq!("120", FACTORIAL[5].to_string(), "5!");
        assert_eq!("720", FACTORIAL[6].to_string(), "6!");
        assert_eq!("5040", FACTORIAL[7].to_string(), "7!");
        assert_eq!("40320", FACTORIAL[8].to_string(), "8!");
        assert_eq!("362880", FACTORIAL[9].to_string(), "9!");
        assert_eq!("3628800", FACTORIAL[10].to_string(), "10!");
        assert_eq!("39916800", FACTORIAL[11].to_string(), "11!");
        assert_eq!("479001600", FACTORIAL[12].to_string(), "12!");
        assert_eq!("6227020800", FACTORIAL[13].to_string(), "13!");
        assert_eq!("87178291200", FACTORIAL[14].to_string(), "14!");
        assert_eq!("1307674368000", FACTORIAL[15].to_string(), "15!");
        assert_eq!("20922789888000", FACTORIAL[16].to_string(), "16!");
        assert_eq!("355687428096000", FACTORIAL[17].to_string(), "17!");
        assert_eq!("6402373705728000", FACTORIAL[18].to_string(), "18!");
        assert_eq!("121645100408832000", FACTORIAL[19].to_string(), "19!");
        assert_eq!("2432902008176640000", FACTORIAL[20].to_string(), "20!");
        assert_eq!("51090942171709440000", FACTORIAL[21].to_string(), "21!");
        assert_eq!("1124000727777607680000", FACTORIAL[22].to_string(), "22!");
        assert_eq!("25852016738884976640000", FACTORIAL[23].to_string(), "23!");
        assert_eq!("620448401733239439360000", FACTORIAL[24].to_string(), "24!");
        assert_eq!("15511210043330985984000000", FACTORIAL[25].to_string(), "25!");
        assert_eq!("403291461126605635584000000", FACTORIAL[26].to_string(), "26!");
        assert_eq!("10888869450418352160768000000", FACTORIAL[27].to_string(), "27!");
    }
}
