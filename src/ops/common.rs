use crate::constants::{MAX_I32_SCALE, MAX_SCALE_I32, POWERS_10};
use crate::Decimal;

#[derive(Debug)]
pub struct Buf12 {
    pub data: [u32; 3],
}

impl Buf12 {
    #[inline(always)]
    pub(super) const fn from_dec64(value: &Dec64) -> Self {
        Buf12 {
            data: [value.low64 as u32, (value.low64 >> 32) as u32, value.hi],
        }
    }

    #[inline(always)]
    pub(super) const fn from_decimal(value: &Decimal) -> Self {
        Buf12 {
            data: value.mantissa_array3(),
        }
    }

    #[inline(always)]
    pub const fn lo(&self) -> u32 {
        self.data[0]
    }
    #[inline(always)]
    pub const fn mid(&self) -> u32 {
        self.data[1]
    }
    #[inline(always)]
    pub const fn hi(&self) -> u32 {
        self.data[2]
    }
    #[inline(always)]
    pub const fn set_lo(&mut self, value: u32) {
        self.data[0] = value;
    }
    #[inline(always)]
    pub const fn set_mid(&mut self, value: u32) {
        self.data[1] = value;
    }
    #[inline(always)]
    pub const fn set_hi(&mut self, value: u32) {
        self.data[2] = value;
    }

    #[inline(always)]
    pub const fn low64(&self) -> u64 {
        ((self.data[1] as u64) << 32) | (self.data[0] as u64)
    }

    #[inline(always)]
    pub const fn set_low64(&mut self, value: u64) {
        self.data[1] = (value >> 32) as u32;
        self.data[0] = value as u32;
    }

    #[inline(always)]
    pub const fn high64(&self) -> u64 {
        ((self.data[2] as u64) << 32) | (self.data[1] as u64)
    }

    #[inline(always)]
    pub const fn set_high64(&mut self, value: u64) {
        self.data[2] = (value >> 32) as u32;
        self.data[1] = value as u32;
    }

    /// Determine the maximum value of x such that scaling up by 10^x still fits in 96 bits.
    /// Returns `None` on overflow, `Some(x)` where `0 <= x <= 9`.
    pub const fn find_scale(&self, scale: i32) -> Option<usize> {
        const OVERFLOW_MAX_9_HI: u32 = 4;
        const OVERFLOW_MAX_8_HI: u32 = 42;
        const OVERFLOW_MAX_7_HI: u32 = 429;
        const OVERFLOW_MAX_6_HI: u32 = 4294;
        const OVERFLOW_MAX_5_HI: u32 = 42949;
        const OVERFLOW_MAX_4_HI: u32 = 429496;
        const OVERFLOW_MAX_3_HI: u32 = 4294967;
        const OVERFLOW_MAX_2_HI: u32 = 42949672;
        const OVERFLOW_MAX_1_HI: u32 = 429496729;
        const OVERFLOW_MAX_9_LOW64: u64 = 5441186219426131129;

        let hi = self.data[2];
        let low64 = self.low64();
        let mut x;

        // Quick exit: if hi exceeds the 1-digit threshold, we can only scale by 0.
        if hi > OVERFLOW_MAX_1_HI {
            if scale < 0 {
                return None;
            }
            return Some(0);
        }

        if scale > MAX_SCALE_I32 - 9 {
            // Can't use a full power-of-9; find the largest safe x.
            x = (MAX_SCALE_I32 - scale) as usize;
            if hi < POWER_OVERFLOW_VALUES[x - 1].data[2] {
                if (x as i32) + scale < 0 {
                    return None;
                }
                return Some(x);
            }
        } else if hi < OVERFLOW_MAX_9_HI || (hi == OVERFLOW_MAX_9_HI && low64 <= OVERFLOW_MAX_9_LOW64) {
            return Some(9);
        }

        // Binary search for the highest safe power (1..=8).
        x = if hi > OVERFLOW_MAX_5_HI {
            if hi > OVERFLOW_MAX_3_HI {
                if hi > OVERFLOW_MAX_2_HI {
                    1
                } else {
                    2
                }
            } else if hi > OVERFLOW_MAX_4_HI {
                3
            } else {
                4
            }
        } else if hi > OVERFLOW_MAX_7_HI {
            if hi > OVERFLOW_MAX_6_HI {
                5
            } else {
                6
            }
        } else if hi > OVERFLOW_MAX_8_HI {
            7
        } else {
            8
        };

        // Verify the boundary: if hi matches exactly, check low64.
        if hi == POWER_OVERFLOW_VALUES[x - 1].data[2] && low64 > POWER_OVERFLOW_VALUES[x - 1].low64() {
            x -= 1;
        }

        if (x as i32) + scale < 0 {
            None
        } else {
            Some(x)
        }
    }
}

/// Largest values that will not overflow when multiplied by 10^(index+1).
/// Index 0 → ×10^1, index 7 → ×10^8.
const POWER_OVERFLOW_VALUES: [Buf12; 8] = [
    Buf12 {
        data: [2576980377, 2576980377, 429496729],
    },
    Buf12 {
        data: [687194767, 4123168604, 42949672],
    },
    Buf12 {
        data: [2645699854, 1271310319, 4294967],
    },
    Buf12 {
        data: [694066715, 3133608139, 429496],
    },
    Buf12 {
        data: [2216890319, 2890341191, 42949],
    },
    Buf12 {
        data: [2369172679, 4154504685, 4294],
    },
    Buf12 {
        data: [4102387834, 2133437386, 429],
    },
    Buf12 {
        data: [410238783, 4078814305, 42],
    },
];

pub(super) struct Dec64 {
    pub negative: bool,
    pub scale: u32,
    pub hi: u32,
    pub low64: u64,
}

impl Dec64 {
    #[inline(always)]
    pub(super) const fn new(d: &Decimal) -> Dec64 {
        let m = d.mantissa_array3();
        Dec64 {
            negative: d.is_sign_negative(),
            scale: d.scale(),
            hi: m[2],
            // Combine lo+mid into a single u64 regardless of whether mid is zero —
            // the branch was noise; this is always correct and equally fast.
            low64: ((m[1] as u64) << 32) | (m[0] as u64),
        }
    }

    #[inline(always)]
    pub(super) const fn lo(&self) -> u32 {
        self.low64 as u32
    }
    #[inline(always)]
    pub(super) const fn mid(&self) -> u32 {
        (self.low64 >> 32) as u32
    }

    #[inline(always)]
    pub(super) const fn high64(&self) -> u64 {
        (self.low64 >> 32) | ((self.hi as u64) << 32)
    }

    #[inline(always)]
    pub(super) const fn to_decimal(&self) -> Decimal {
        Decimal::from_parts(
            self.low64 as u32,
            (self.low64 >> 32) as u32,
            self.hi,
            self.negative,
            self.scale,
        )
    }
}

pub struct Buf16 {
    pub data: [u32; 4],
}

impl Buf16 {
    #[inline(always)]
    pub const fn zero() -> Self {
        Buf16 { data: [0, 0, 0, 0] }
    }

    #[inline(always)]
    pub const fn low64(&self) -> u64 {
        ((self.data[1] as u64) << 32) | (self.data[0] as u64)
    }

    #[inline(always)]
    pub const fn set_low64(&mut self, value: u64) {
        self.data[1] = (value >> 32) as u32;
        self.data[0] = value as u32;
    }

    #[inline(always)]
    pub const fn mid64(&self) -> u64 {
        ((self.data[2] as u64) << 32) | (self.data[1] as u64)
    }

    #[inline(always)]
    pub const fn set_mid64(&mut self, value: u64) {
        self.data[2] = (value >> 32) as u32;
        self.data[1] = value as u32;
    }

    #[inline(always)]
    pub const fn high64(&self) -> u64 {
        ((self.data[3] as u64) << 32) | (self.data[2] as u64)
    }

    #[inline(always)]
    pub const fn set_high64(&mut self, value: u64) {
        self.data[3] = (value >> 32) as u32;
        self.data[2] = value as u32;
    }
}

#[derive(Debug)]
pub struct Buf24 {
    pub data: [u32; 6],
}

impl Buf24 {
    #[inline(always)]
    pub const fn zero() -> Self {
        Buf24 {
            data: [0, 0, 0, 0, 0, 0],
        }
    }

    #[inline(always)]
    pub const fn low64(&self) -> u64 {
        ((self.data[1] as u64) << 32) | (self.data[0] as u64)
    }

    #[inline(always)]
    pub const fn set_low64(&mut self, value: u64) {
        self.data[1] = (value >> 32) as u32;
        self.data[0] = value as u32;
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub const fn mid64(&self) -> u64 {
        ((self.data[3] as u64) << 32) | (self.data[2] as u64)
    }

    #[inline(always)]
    pub const fn set_mid64(&mut self, value: u64) {
        self.data[3] = (value >> 32) as u32;
        self.data[2] = value as u32;
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub const fn high64(&self) -> u64 {
        ((self.data[5] as u64) << 32) | (self.data[4] as u64)
    }

    #[inline(always)]
    pub const fn set_high64(&mut self, value: u64) {
        self.data[5] = (value >> 32) as u32;
        self.data[4] = value as u32;
    }

    #[inline(always)]
    pub const fn upper_word(&self) -> usize {
        // Scan from the top down — branchless cascade.
        if self.data[5] > 0 {
            return 5;
        }
        if self.data[4] > 0 {
            return 4;
        }
        if self.data[3] > 0 {
            return 3;
        }
        if self.data[2] > 0 {
            return 2;
        }
        if self.data[1] > 0 {
            return 1;
        }
        0
    }

    /// Attempt to rescale the 192-bit buffer into 96 bits.
    ///
    /// * `upper` – index of the highest non-zero word.
    /// * `scale` – current scale factor.
    ///
    /// Returns the adjusted scale wrapped in `Some`, or `None` on overflow.
    pub const fn rescale(&mut self, upper: usize, scale: u32) -> Option<u32> {
        let mut scale = scale as i32;
        let mut upper = upper;

        // Determine the initial rescale target.
        let mut rescale_target = 0i32;
        if upper > 2 {
            rescale_target = upper as i32 * 32 - 64 - 1;
            rescale_target -= self.data[upper].leading_zeros() as i32;
            rescale_target = ((rescale_target * 77) >> 8) + 1;
            if rescale_target > scale {
                return None;
            }
        }

        // Ensure we scale enough to reach a valid range.
        if rescale_target < scale - MAX_SCALE_I32 {
            rescale_target = scale - MAX_SCALE_I32;
        }

        if rescale_target > 0 {
            scale -= rescale_target;
            let mut sticky = 0u32;
            let mut remainder = 0u32;

            loop {
                sticky |= remainder;

                let power = if rescale_target > 8 {
                    POWERS_10[9]
                } else {
                    POWERS_10[rescale_target as usize]
                };
                let power64 = power as u64;

                // Divide the highest word first; its remainder cascades downward.
                let high = self.data[upper];
                let high_quotient = high / power;
                remainder = high % power;

                // Divide remaining words from (upper-1) down to 0, feeding the remainder.
                let mut i = upper;
                while i > 0 {
                    i -= 1;
                    let num = (self.data[i] as u64) + ((remainder as u64) << 32);
                    let q = (num / power64) as u32;
                    remainder = (num % power64) as u32;
                    self.data[i] = q;
                }

                self.data[upper] = high_quotient;

                // Shrink the upper bound if the high word became zero.
                if high_quotient == 0 && upper > 0 {
                    upper -= 1;
                }

                if rescale_target > MAX_I32_SCALE {
                    rescale_target -= MAX_I32_SCALE;
                    continue;
                }

                // If still > 96 bits, reduce by one more power of 10.
                if upper > 2 {
                    if scale == 0 {
                        return None;
                    }
                    rescale_target = 1;
                    scale -= 1;
                    continue;
                }

                // Round the final result (round-half-up, tie-to-odd).
                let half_power = power >> 1;
                let carried =
                    if remainder > half_power || (remainder == half_power && ((self.data[0] & 1) | sticky) != 0) {
                        self.data[0] = self.data[0].wrapping_add(1);
                        self.data[0] == 0
                    } else {
                        false
                    };

                // Propagate carry through words 1..5.
                if carried {
                    let mut pos = 0usize;
                    let mut idx = 1usize;
                    while idx < 6 {
                        pos = idx;
                        self.data[idx] = self.data[idx].wrapping_add(1);
                        if self.data[idx] != 0 {
                            break;
                        }
                        idx += 1;
                    }

                    // If carry pushed us back above 96 bits, rescale once more.
                    if pos > 2 {
                        if scale == 0 {
                            return None;
                        }
                        upper = pos;
                        sticky = 0;
                        remainder = 0;
                        rescale_target = 1;
                        scale -= 1;
                        continue;
                    }
                }

                break;
            }
        }

        Some(scale as u32)
    }
}
