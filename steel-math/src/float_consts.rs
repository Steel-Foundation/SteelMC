//! IEEE 754 binary32 layout constants

/// Total bit width of an `f32`
pub const SIZE: i32 = u32::BITS as i32;
/// Significand width including the implicit bit
pub const SIGNIFICAND_WIDTH: i32 = f32::MANTISSA_DIGITS as i32;
/// Bias applied to the stored exponent
pub const EXP_BIAS: i32 = f32::MAX_EXP - 1;
/// Mask for the stored exponent bits
pub const EXP_BIT_MASK: i32 = ((1 << (SIZE - SIGNIFICAND_WIDTH)) - 1) << (SIGNIFICAND_WIDTH - 1);
/// Mask for the fraction bits, excluding the implicit bit
pub const SIGNIF_BIT_MASK: i32 = (1 << (SIGNIFICAND_WIDTH - 1)) - 1;
