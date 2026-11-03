//! Bit-exact port of HotSpot's `x86-64` `Math.sin` / `Math.cos` intrinsics.
//!
//! On `x86-64` the JVM never calls the C library for `Math.sin`/`Math.cos`: both the
//! interpreter and the JIT run `StubRoutines::dsin`/`dcos`, generated from
//! `src/hotspot/cpu/x86/stubGenerator_x86_64_sin.cpp` and `_cos.cpp` (Intel LIBM, unchanged
//! from JDK 9 through JDK 25). Those stubs round differently from glibc in the last bit for
//! some inputs (for example `sin(3.664170064300503)`), which is enough to flip a block in a
//! world-generation feature such as the iceberg ellipse. This module follows the stubs
//! instruction by instruction: every IEEE double operation in the same order, every 32-bit
//! and 64-bit integer operation with the same width, so it reproduces them bit for bit on any
//! target. It is verified against 3.3 million values dumped from a JDK 25 JVM.
//!
//! Structure, as in the stubs:
//! * main path for `2^-252 <= |x| < 90112`: `x = N * pi/32 + r`, table lookup on `N mod 64`,
//!   polynomial in `r` ([`core`]);
//! * tiny arguments: `sin(x) = (2^55 x - x) 2^-55` (or `x (1 - 2^-53)` for denormals),
//!   `cos(x) = 1 - |x|`;
//! * large arguments: Payne-Hanek style reduction with the bits of `1/(2 pi)` ([`large`]),
//!   then [`core`] with the quadrant folded into the table index;
//! * NaN and infinities: `x * -0.0`, i.e. NaN.
//!
//! Use these wherever vanilla calls `Math.sin`/`Math.cos` on a `double` (ore blobs, icebergs,
//! fancy oak limbs, end spikes, stronghold rings, the spawn search and the `Mth` sine table).
//! `Mth.sin`/`Mth.cos` are the float lookup table in [`crate::trig`], not this.

mod tables;

use tables::{CTABLE, PI_INV_TABLE};

const SIGN: u64 = 0x8000_0000_0000_0000;
/// `32 / pi`.
const PI32INV: f64 = f64::from_bits(0x4024_5f30_6dc9_c883);
/// `pi / 32` split into a 32-bit, a 32-bit and a 53-bit part.
const P_1: f64 = f64::from_bits(0x3fb9_21fb_5440_0000);
const P_2: f64 = f64::from_bits(0x3d90_b461_1a60_0000);
const P_3: f64 = f64::from_bits(0x3b63_198a_2e03_7073);
/// Polynomial coefficients, `[sin part, cos part]` as the packed SSE lanes hold them.
const SC_1: [f64; 2] = [
    f64::from_bits(0xbfc5_5555_5555_5555),
    f64::from_bits(0xbfe0_0000_0000_0000),
];
const SC_2: [f64; 2] = [
    f64::from_bits(0x3f81_1111_1111_1111),
    f64::from_bits(0x3fa5_5555_5555_5555),
];
const SC_3: [f64; 2] = [
    f64::from_bits(0xbf2a_01a0_1a01_a01a),
    f64::from_bits(0xbf56_c16c_16c1_6c17),
];
const SC_4: [f64; 2] = [
    f64::from_bits(0x3ec7_1de3_a556_c734),
    f64::from_bits(0x3efa_01a0_1a01_a01a),
];
/// `pi / 4` as a high and a low part.
const PI_4_HI: f64 = f64::from_bits(0x3fe9_21fb_4000_0000);
const PI_4_LO: f64 = f64::from_bits(0x3e64_442d_1846_9899);
const TWO_POW_55: f64 = f64::from_bits(0x4360_0000_0000_0000);
const TWO_POW_M55: f64 = f64::from_bits(0x3c80_0000_0000_0000);
/// `1 - 2^-53`.
const ALL_ONES: f64 = f64::from_bits(0x3fef_ffff_ffff_ffff);

/// Width of the exponent class that takes the main path (`cmpl rax, 281346048` in the stub).
const MAIN_SPAN: u32 = 0x10c5_0000;
/// Added to `N` before the table lookup: `1865216 = 0 (mod 64)` for sin, `1865232 = 16 (mod 64)`
/// for cos, a quarter turn through the 64-entry table.
const SIN_SHIFT: u32 = 1_865_216;
const COS_SHIFT: u32 = 1_865_232;

/// `Math.sin(x)` as the `HotSpot` `x86-64` intrinsic computes it.
#[must_use]
pub fn sin(x: f64) -> f64 {
    let hi = (x.to_bits() >> 32) as u32;
    let e = (hi & 0x7fff_0000).wrapping_sub(0x3030_0000);
    if e <= MAIN_SPAN {
        return core(x, 0.0, 0, false);
    }
    if (e as i32) > MAIN_SPAN as i32 {
        return large(x, false);
    }
    if e >> 20 == 3325 {
        // Zero or denormal.
        return x * ALL_ONES;
    }
    let t = TWO_POW_55 * x;
    (t - x) * TWO_POW_M55
}

/// `Math.cos(x)` as the `HotSpot` `x86-64` intrinsic computes it.
#[must_use]
pub fn cos(x: f64) -> f64 {
    let hi = (x.to_bits() >> 32) as u32;
    let e = (hi & 0x7fff_0000).wrapping_sub(0x3030_0000);
    if e <= MAIN_SPAN {
        return core(x, 0.0, COS_SHIFT, false);
    }
    if (e as i32) > MAIN_SPAN as i32 {
        return large(x, true);
    }
    1.0 - f64::from_bits(x.to_bits() & !SIGN)
}

/// Table lookup and polynomial on `x` (plus its low part `x_lo`, zero on the main path).
/// `shift` is added to `N` before `mod 64`; `wide` selects the 64-bit integer conversion the
/// cos stub uses after a large-argument reduction (no observable difference, kept literal).
#[inline]
#[expect(
    clippy::many_single_char_names,
    reason = "names follow the HotSpot stub's algorithm notes"
)]
fn core(x: f64, x_lo: f64, shift: u32, wide: bool) -> f64 {
    let y = PI32INV * x;
    let half = f64::from_bits(0x3fe0_0000_0000_0000 | (x.to_bits() & SIGN));
    // cvttsd2si: truncation toward zero of y +- 0.5, i.e. round half away from zero.
    let n: i64 = if wide {
        (y + half) as i64
    } else {
        i64::from((y + half) as i32)
    };
    let nd = n as f64;
    let idx = ((n as u32).wrapping_add(shift) & 63) as usize * 4;
    let c_hl = CTABLE[idx];
    let s_hi = CTABLE[idx + 1];
    let s_lo = CTABLE[idx + 2];
    let sigma = CTABLE[idx + 3];

    let m1 = P_1 * nd;
    let m2 = P_2 * nd;
    let m3 = nd * P_3;
    let r1 = x - m1;
    let r = r1 - m2;
    let psc4 = [SC_4[0] * r1, SC_4[1] * r1];
    let s_hi_r = s_hi * r;
    let c1 = r1 - r;
    let msc4 = [psc4[0] * r, psc4[1] * r];
    let r2 = r * r;
    let c2 = c1 - m2;
    let neg_c = m3 - c2;
    let chs = c_hl + sigma;
    let neg_d = s_hi_r - chs;
    let neg_c = neg_c - x_lo;
    let chs_r = chs * r;
    let msc2 = [SC_2[0] * r2, SC_2[1] * r2];
    let rs = sigma * r;
    let p = [chs_r * r2, s_hi * r2];
    let r4 = r2 * r2;
    let psc3 = [msc4[0] + SC_3[0], msc4[1] + SC_3[1]];
    let med = r * c_hl;
    let psc1 = [msc2[0] + SC_1[0], msc2[1] + SC_1[1]];
    let msc3 = [psc3[0] * r4, psc3[1] * r4];
    let res_int = rs + s_hi;
    let cd = neg_c * neg_d;
    let res_hi = med + res_int;
    let sincospols = [psc1[0] + msc3[0], psc1[1] + msc3[1]];
    let k0 = s_hi - res_int;
    let k1 = res_int - res_hi;
    let corr = cd + s_lo;
    let pols = [sincospols[0] * p[0], sincospols[1] * p[1]];
    let k2 = k0 + rs;
    let k3 = k1 + med;
    let lo = corr + k2;
    let lo = lo + k3;
    let lo = lo + pols[0];
    let lo = lo + pols[1];
    res_hi + lo
}

/// Low 32 bits, zero-extended: the effect of a 32-bit x86 instruction writing a register.
#[inline]
const fn w32(v: u64) -> u64 {
    (v as u32) as u64
}

/// `[hi:mid:lo] = [hi:0:0] - [r9:r10:r8]`, the stub's `sub`/`sbb`/`sbb` sequence.
#[inline]
const fn negate3(r8: u64, r10: u64, r9: u64, hi: u64) -> (u64, u64, u64) {
    let (n8, b1) = 0u64.overflowing_sub(r8);
    let (t, b2a) = 0u64.overflowing_sub(r10);
    let (n10, b2b) = t.overflowing_sub(b1 as u64);
    let n9 = hi.wrapping_sub(r9).wrapping_sub((b2a | b2b) as u64);
    (n8, n10, n9)
}

/// Arguments with `|x| >= 90112`, NaN and infinities. Variable names follow the stub's
/// registers so the port can be checked against it line by line.
#[expect(
    clippy::too_many_lines,
    reason = "one straight-line transcription of the stub"
)]
fn large(x: f64, is_cos: bool) -> f64 {
    let xb = x.to_bits();
    let hi16 = ((xb >> 48) & 0xffff) as u32;
    if hi16 & 0x7ff0 == 0x7ff0 {
        // NaN or infinity: multiply by -0.0, which yields NaN either way.
        return x * f64::from_bits(SIGN);
    }
    let shift = if is_cos { COS_SHIFT } else { SIN_SHIFT };

    // Byte offset into the 1/(2 pi) table, chosen by the exponent.
    let mut rcx = u64::from((((hi16 & 0x7ff0).wrapping_sub(0x3f60)) >> 7) & 0xfffc);
    let base = (rcx >> 2) as usize;
    let t = |i: usize| u64::from(PI_INV_TABLE[base + i]);

    let mut rax = xb;
    let mut r10 = t(5);
    let mut r8 = t(6);
    let mut rdx = w32(rax);
    rax >>= 21;
    rax = w32(rax) | 0x8000_0000;
    rax = u64::from((rax as u32) >> 11);
    let mut r9 = w32(r10);
    r10 = r10.wrapping_mul(rdx);
    r9 = r9.wrapping_mul(rax);
    r8 = r8.wrapping_mul(rax);
    let mut rsi = t(4);
    let mut rdi = t(3);
    let mut r11 = w32(r10);
    r10 >>= 32;
    r9 = r9.wrapping_add(r10);
    r11 = r11.wrapping_add(r8);
    r8 = w32(r11);
    r11 >>= 32;
    r9 = r9.wrapping_add(r11);
    r10 = w32(rsi);
    rsi = rsi.wrapping_mul(rdx);
    r10 = r10.wrapping_mul(rax);
    r11 = w32(rdi);
    rdi = rdi.wrapping_mul(rdx);
    let mut rbx = w32(rsi);
    rsi >>= 32;
    r9 = r9.wrapping_add(rbx);
    rbx = w32(r9);
    r9 >>= 32;
    r10 = r10.wrapping_add(rsi);
    r10 = r10.wrapping_add(r9);
    rbx <<= 32;
    r8 |= rbx;
    r11 = r11.wrapping_mul(rax);
    r9 = t(2);
    rsi = t(1);
    rbx = w32(rdi);
    rdi >>= 32;
    r10 = r10.wrapping_add(rbx);
    rbx = w32(r10);
    r10 >>= 32;
    r11 = r11.wrapping_add(rdi);
    r11 = r11.wrapping_add(r10);
    rdi = r9;
    r9 = r9.wrapping_mul(rdx);
    rdi = rdi.wrapping_mul(rax);
    r10 = w32(r9);
    r9 >>= 32;
    r11 = r11.wrapping_add(r10);
    r10 = w32(r11);
    r11 >>= 32;
    rdi = rdi.wrapping_add(r9);
    rdi = rdi.wrapping_add(r11);
    r9 = rsi;
    rsi = rsi.wrapping_mul(rdx);
    r9 = r9.wrapping_mul(rax);
    r10 <<= 32;
    r10 |= rbx;
    rax = t(0);
    r11 = w32(rsi);
    rsi >>= 32;
    rdi = rdi.wrapping_add(r11);
    r11 = w32(rdi);
    rdi >>= 32;
    r9 = r9.wrapping_add(rsi);
    r9 = r9.wrapping_add(rdi);
    rdx = rdx.wrapping_mul(rax);

    // Exponent bookkeeping: how many bits of the product lie below the binary point.
    rcx = w32(u64::from((rcx as u32).wrapping_mul(8).wrapping_add(19)));
    rsi = u64::from(hi16 & 0x8000);
    rbx = w32(u64::from(((hi16 >> 4) & 0x7ff).wrapping_sub(1023)));
    rcx = w32(u64::from((rcx as u32).wrapping_sub(rbx as u32)));
    r9 = r9.wrapping_add(rdx);
    rdx = w32(u64::from((rcx as u32).wrapping_add(32)));

    if (rcx as u32 as i32) < 1 {
        rcx = w32(u64::from((rcx as u32).wrapping_neg()));
        r9 = (r9 << 32) | r11;
        r9 <<= rcx & 63;
        rdi = r9;
        if (r9 as u32) & 0x8000_0000 != 0 {
            r9 = w32(u64::from((r9 as u32) >> (rcx & 31)));
            rbx = 0x1_0000_0000u64 >> (rcx & 63);
            (r8, r10, r9) = negate3(r8, r10, r9, rbx);
            rbx = 0x8000;
            rdi >>= 3;
            rdi = w32(u64::from((rdi as u32).wrapping_add(536_870_912)));
        } else {
            r9 = w32(u64::from((r9 as u32) >> (rcx & 31)));
            rbx = 0;
            rdi >>= 3;
        }
    } else {
        rcx = w32(u64::from((rcx as u32).wrapping_neg().wrapping_add(29)));
        r9 = w32(u64::from((r9 as u32) << (rcx & 31)));
        rdi = w32(r9);
        r9 &= 0x1fff_ffff;
        if r9 & 0x1000_0000 != 0 {
            r9 = w32(u64::from((r9 as u32) >> (rcx & 31)));
            rbx = u64::from(536_870_912u32 >> (rcx & 31));
            r9 = (r9 << 32) | r11;
            rbx <<= 32;
            rdi = w32(u64::from((rdi as u32).wrapping_add(536_870_912)));
            (r8, r10, r9) = negate3(r8, r10, r9, rbx);
            rbx = 0x8000;
        } else {
            r9 = w32(u64::from((r9 as u32) >> (rcx & 31)));
            rbx = 0;
            r9 = (r9 << 32) | r11;
        }
    }

    if r9 == 0 {
        rdx = w32(u64::from((rdx as u32).wrapping_add(64)));
        r9 = r10;
        r10 = r8;
        r8 = 0;
        if r9 == 0 {
            rdx = w32(u64::from((rdx as u32).wrapping_add(64)));
            r9 = r10;
            r10 = r8;
            if r9 == 0 {
                // The stub jumps to the polynomial with a zero argument and whatever `rax`
                // held (the first table word); kept literal, unreachable for finite inputs.
                return core(0.0, 0.0, shift.wrapping_add((rax as u32) << 3), is_cos);
            }
        }
    }

    // Normalise so that the leading bit of r9 sits at bit 29.
    r11 = 63 - u64::from(r9.leading_zeros());
    rcx = w32(u64::from(29u32.wrapping_sub(r11 as u32)));
    if (rcx as u32 as i32) <= 0 {
        if rcx as u32 != 0 {
            rcx = w32(u64::from((rcx as u32).wrapping_neg()));
            r10 >>= rcx & 63;
            rax = r9;
            r9 >>= rcx & 63;
            rdx = w32(u64::from((rdx as u32).wrapping_sub(rcx as u32)));
            rcx = w32(u64::from((rcx as u32).wrapping_neg().wrapping_add(64)));
            rax <<= rcx & 63;
            r10 |= rax;
        }
    } else {
        r9 <<= rcx & 63;
        rax = r10;
        r10 <<= rcx & 63;
        rdx = w32(u64::from((rdx as u32).wrapping_add(rcx as u32)));
        rcx = w32(u64::from((rcx as u32).wrapping_neg().wrapping_add(64)));
        rax >>= rcx & 63;
        r8 >>= rcx & 63;
        r9 |= rax;
        r10 |= r8;
    }

    // Back to floating point: the reduced argument in units of pi/4, high and low word.
    let f0 = (r9 as i64) as f64;
    r10 >>= 1;
    let f3 = (r10 as i64) as f64;
    let e = (((rdx as u32) << 4).wrapping_neg().wrapping_add(0x3ff0) | rsi as u32) ^ rbx as u32;
    let f4 = f64::from_bits(u64::from(e & 0xffff) << 48);
    let f5 = f64::from_bits(u64::from(e.wrapping_sub(1008) & 0xffff) << 48);
    let f0 = f0 * f4;
    let sign_mask = ((((rsi as u32) << 16) as i32) >> 31) as u32;
    let f3 = f3 * f5;
    let f1 = f0;
    let f0 = f0 * PI_4_HI;
    let quadrant = (rdi as u32) >> 29;
    let f1 = f1 + f3;
    let f3 = f3 * PI_4_HI;
    let quadrant = quadrant.wrapping_add(sign_mask) ^ sign_mask;
    let f6 = PI_4_LO * f1;
    let f6 = f6 + f3;
    let f2 = f0;
    let f0 = f0 + f6;
    let f2 = f2 - f0;
    let f6 = f6 + f2;
    core(f0, f6, shift.wrapping_add(quadrant << 3), is_cos)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use super::{cos, sin};

    /// `(x, Math.sin(x), Math.cos(x))` raw bits dumped from a JDK 25 x86-64 JVM: the special
    /// values and a sample of every exponent range, from denormals to `1e300`.
    const JVM_CASES: &[(u64, u64, u64)] = &[
        (
            0x0000_0000_0000_0000,
            0x0000_0000_0000_0000,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x8000_0000_0000_0000,
            0x8000_0000_0000_0000,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x0000_0000_0000_0001,
            0x0000_0000_0000_0001,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x8000_0000_0000_0001,
            0x8000_0000_0000_0001,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x0010_0000_0000_0000,
            0x0010_0000_0000_0000,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x8010_0000_0000_0000,
            0x8010_0000_0000_0000,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x7fef_ffff_ffff_ffff,
            0x3f74_52fc_98b3_4e97,
            0xbfef_ffe6_2ecf_ab75,
        ),
        (
            0xffef_ffff_ffff_ffff,
            0xbf74_52fc_98b3_4e97,
            0xbfef_ffe6_2ecf_ab75,
        ),
        (
            0x7ff0_0000_0000_0000,
            0xfff8_0000_0000_0000,
            0xfff8_0000_0000_0000,
        ),
        (
            0xfff0_0000_0000_0000,
            0xfff8_0000_0000_0000,
            0xfff8_0000_0000_0000,
        ),
        (
            0x7ff8_0000_0000_0000,
            0x7ff8_0000_0000_0000,
            0x7ff8_0000_0000_0000,
        ),
        (
            0x400d_5038_6509_3918,
            0xbfdf_f180_ed7e_829d,
            0xbfeb_baa8_f055_c95c,
        ),
        (
            0x4009_21fb_5444_2d18,
            0x3ca1_a626_3314_5c07,
            0xbff0_0000_0000_0000,
        ),
        (
            0xc009_21fb_5444_2d18,
            0xbca1_a626_3314_5c07,
            0xbff0_0000_0000_0000,
        ),
        (
            0x3ff9_21fb_5444_2d18,
            0x3ff0_0000_0000_0000,
            0x3c91_a626_3314_5c07,
        ),
        (
            0x3fe9_21fb_5444_2d18,
            0x3fe6_a09e_667f_3bcc,
            0x3fe6_a09e_667f_3bcd,
        ),
        (
            0x4019_21fb_5444_2d18,
            0xbcb1_a626_3314_5c07,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x40f6_0000_0000_0000,
            0xbfef_bde6_256f_07e5,
            0x3fc0_3a48_7f06_b7f8,
        ),
        (
            0x40f5_ffff_fff5_83a5,
            0xbfef_bde8_ce0b_8a0d,
            0x3fc0_39f5_4972_df8c,
        ),
        (
            0x40f7_0000_0000_0000,
            0xbfeb_eeb9_6dd5_26ca,
            0xbfdf_39ff_c707_97c8,
        ),
        (
            0x40f6_fffe_6666_6666,
            0xbfea_3bf7_642c_8ee6,
            0xbfe2_52e9_96d4_e160,
        ),
        (
            0x3030_0000_0000_0000,
            0x3030_0000_0000_0000,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x3020_0000_0000_0000,
            0x3020_0000_0000_0000,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x3040_0000_0000_0000,
            0x3040_0000_0000_0000,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x40f0_0000_0000_0000,
            0x3fe6_2566_735b_8513,
            0xbfe7_1945_3109_288b,
        ),
        (
            0x4100_0000_0000_0000,
            0xbfef_f8bd_7b10_d6b0,
            0x3fa5_8ced_65ec_8b50,
        ),
        (
            0x40f8_6a00_0000_0000,
            0x3fa2_4daa_9c52_7e96,
            0xbfef_fac3_841b_3da7,
        ),
        (
            0x412e_8480_0000_0000,
            0xbfd6_664b_2568_d867,
            0x3fed_f9df_9906_d32c,
        ),
        (
            0x4202_a05f_2000_0000,
            0xbfdf_334c_7896_a4e3,
            0x3feb_f098_901c_931a,
        ),
        (
            0x4480_f0cf_064d_d592,
            0xbfeb_453a_b76b_f397,
            0x3fe0_be2c_ef01_c8f4,
        ),
        (
            0x7e37_e43c_8800_759c,
            0xbfea_2c16_b010_e385,
            0xbfe2_6990_22ad_c4c1,
        ),
        (
            0x0008_0000_0000_0000,
            0x0008_0000_0000_0000,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x3ff0_0000_0000_0000,
            0x3fea_ed54_8f09_0cee,
            0x3fe1_4a28_0fb5_068c,
        ),
        (
            0xbff0_0000_0000_0000,
            0xbfea_ed54_8f09_0cee,
            0x3fe1_4a28_0fb5_068c,
        ),
        (
            0x3fe0_0000_0000_0000,
            0x3fde_aee8_744b_05f0,
            0x3fec_1528_065b_7d50,
        ),
        (
            0x4059_0000_0000_0000,
            0xbfe0_3425_b78c_4db8,
            0x3feb_981d_bf66_5fdf,
        ),
        (
            0x408f_4000_0000_0000,
            0x3fea_75cc_150a_206b,
            0x3fe1_ff02_6793_f1bb,
        ),
        (
            0x4012_d97c_7f33_21d2,
            0xbff0_0000_0000_0000,
            0xbcaa_7939_4c9e_8a0a,
        ),
        (
            0x430c_6bf5_2634_0000,
            0x3feb_76f8_8136_ceba,
            0xbfe0_6c15_4609_d33f,
        ),
        (
            0x4341_c379_37e0_8000,
            0x3fe8_f334_432e_bba5,
            0xbfe4_0991_e398_dbfc,
        ),
        (
            0x4376_3457_85d8_a000,
            0xbfdd_badc_7a11_9fc8,
            0xbfec_567c_5278_afcb,
        ),
        (
            0x43f1_58e4_6091_3d00,
            0x3fe6_3e6f_8ad6_3d54,
            0xbfe7_012a_45e8_a24f,
        ),
        (
            0x4415_af1d_78b5_8c40,
            0xbfe4_a5e6_05fd_6450,
            0x3fe8_7272_0fc6_0d3d,
        ),
        (
            0x4499_6936_8974_c05b,
            0xbfb4_ba90_4147_c7f8,
            0xbfef_e519_c9b5_9755,
        ),
        (
            0x6979_cded_e69d_bb56,
            0xbfec_bdd5_3ad7_c7ce,
            0x3fdc_2319_542c_60af,
        ),
        (
            0x7e79_21fb_5444_2d18,
            0x3feb_921e_b761_288b,
            0x3fe0_3e57_7e2d_1264,
        ),
        (
            0x43b0_0000_0000_0000,
            0xbfea_94ad_ab06_665c,
            0xbfe1_d146_047d_6947,
        ),
        (
            0x4630_0000_0000_0000,
            0xbfeb_e8ed_97ac_1f59,
            0x3fdf_4eb3_ff66_e36d,
        ),
        (
            0x5f30_0000_0000_0000,
            0x3fdb_78f3_ff6b_537b,
            0x3fec_e6de_5cd1_db81,
        ),
        (
            0x5c9f_20d5_8361_b331,
            0xbfe7_7316_ecf4_fbeb,
            0x3fe5_c634_e73f_5734,
        ),
        (
            0xeed8_a921_eac8_0778,
            0x3fed_42c2_0535_d715,
            0x3fd9_e877_fb85_eb18,
        ),
        (
            0x45f4_d58e_248a_b66e,
            0xbfef_fff2_0207_23a4,
            0xbf6d_ecbb_bc30_f03e,
        ),
        (
            0xd1b5_6758_7fdf_9231,
            0x3fcb_30ae_3b66_c120,
            0xbfef_450a_57da_9773,
        ),
        (
            0xc8a7_cda5_ddde_6128,
            0xbfe3_86b3_c466_aab6,
            0x3fe9_5a22_51f9_3c81,
        ),
        (
            0x92c6_db45_6fb4_cba2,
            0x92c6_db45_6fb4_cba2,
            0x3ff0_0000_0000_0000,
        ),
        (
            0xc614_f148_13eb_26bb,
            0xbfe9_f118_d381_7748,
            0xbfe2_bc52_2bb5_64a3,
        ),
        (
            0xb590_9710_7043_5983,
            0xb590_9710_7043_5983,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x3944_b1d1_5144_8e07,
            0x3944_b1d1_5144_8e07,
            0x3ff0_0000_0000_0000,
        ),
        (
            0xc002_ede1_e92c_647f,
            0xbfe6_66a4_b775_039a,
            0xbfe6_da05_0191_24b4,
        ),
        (
            0xc015_6b7c_0d8d_6715,
            0x3fe9_9e24_6556_f243,
            0x3fe3_2d23_4fae_470c,
        ),
        (
            0x4002_2dcc_fb85_da63,
            0x3fe8_714e_e486_0e5d,
            0xbfe4_a73e_b062_1fd7,
        ),
        (
            0xc017_2026_d721_73fd,
            0x3fde_c897_70fa_e826,
            0x3fec_0e20_3e24_2a24,
        ),
        (
            0xc006_3d3d_58ea_0db4,
            0xbfd6_a593_020b_2e3a,
            0xbfed_edfa_34db_fb7e,
        ),
        (
            0xbfe2_d38d_9884_b1c2,
            0xbfe1_c24b_ffee_58b1,
            0x3fea_9eb1_8f27_84c5,
        ),
        (
            0x4004_3ba7_a69d_9f96,
            0x3fe2_6576_6c49_7ca9,
            0xbfea_2ef8_b03d_9f77,
        ),
        (
            0x4000_e6f0_c453_c526,
            0x3feb_6a0c_806e_2de1,
            0xbfe0_819e_6c16_937d,
        ),
        (
            0x7871_fdde_dd88_07a1,
            0xbfef_f988_086c_14f5,
            0xbfa4_57bf_d489_b4bd,
        ),
        (
            0x6f30_c3b7_7be0_113f,
            0x3fee_3cb8_ca9c_9e27,
            0x3fd4_f2de_4f82_2965,
        ),
        (
            0xf384_ed39_e7ed_3a7f,
            0xbfef_e8e2_cc76_c5e9,
            0xbfb3_379e_cc32_d071,
        ),
        (
            0x56e7_ada5_1158_e373,
            0xbfdf_7306_8078_51e4,
            0xbfeb_deb6_341f_ecac,
        ),
        (
            0xd0d8_4a11_940b_66e1,
            0x3fec_607a_6ab8_c7ef,
            0x3fdd_94a3_f07f_b66f,
        ),
        (
            0xf17c_6bb1_4e08_5ac5,
            0x3fd9_f8fe_1aaa_37d4,
            0xbfed_3f18_440c_e757,
        ),
        (
            0xf63f_d434_09f0_8ecc,
            0x3fee_b8e0_9f3f_8c1c,
            0xbfd1_e7b2_261b_6cd9,
        ),
        (
            0xc933_1a76_0cc0_9a26,
            0xbfd4_6191_dbbb_3eb9,
            0xbfee_5581_e61e_1a73,
        ),
        (
            0x868e_fb80_4f5c_a261,
            0x868e_fb80_4f5c_a261,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x4118_6eda_3521_c260,
            0x3fe2_4cf2_d389_a1c4,
            0xbfea_4020_c8b2_0f3b,
        ),
        (
            0x8018_357d_f914_5414,
            0x8018_357d_f914_5414,
            0x3ff0_0000_0000_0000,
        ),
        (
            0x40ef_15b7_3a0e_faa8,
            0x3fde_4008_9eae_a450,
            0x3fec_332a_5a28_d242,
        ),
        (
            0xa4da_657d_990d_1f3e,
            0xa4da_657d_990d_1f3e,
            0x3ff0_0000_0000_0000,
        ),
        (
            0xc107_034b_d99a_a9ba,
            0xbfe6_b726_64b7_5ec0,
            0x3fe6_89ff_e255_9e21,
        ),
    ];

    #[test]
    fn matches_jvm_samples() {
        for &(x, s, c) in JVM_CASES {
            let x = f64::from_bits(x);
            assert_eq!(sin(x).to_bits(), s, "sin({x:e})");
            assert_eq!(cos(x).to_bits(), c, "cos({x:e})");
        }
    }

    /// Full check against a vector file produced by the extractor's `TrigVectors.java`
    /// (24-byte big-endian records of `x`, `Math.sin(x)`, `Math.cos(x)` bits); skipped when
    /// `JVM_TRIG_VECTORS` is unset.
    #[test]
    fn matches_jvm_vector_file() {
        let Ok(path) = env::var("JVM_TRIG_VECTORS") else {
            return;
        };
        let data = fs::read(&path).expect("readable vector file");
        let mut mismatches = 0u64;
        let (records, _) = data.as_chunks::<24>();
        for rec in records {
            let x = f64::from_bits(u64::from_be_bytes(rec[0..8].try_into().expect("8 bytes")));
            let s = u64::from_be_bytes(rec[8..16].try_into().expect("8 bytes"));
            let c = u64::from_be_bytes(rec[16..24].try_into().expect("8 bytes"));
            if sin(x).to_bits() != s || cos(x).to_bits() != c {
                mismatches += 1;
            }
        }
        assert_eq!(mismatches, 0, "records differing from the JVM");
    }
}
