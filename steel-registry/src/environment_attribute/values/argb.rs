//! Vector color helpers of vanilla's `util.ARGB` used by color attributes.
//!
//! RGB colors are [`Vec3`] (`x` = red, `y` = green, `z` = blue) and ARGB colors
//! are [`Vec4`] with alpha in `w`, matching vanilla's JOML vectors. Packed
//! colors are `0xAARRGGBB`.

use glam::{Vec3, Vec4};
use steel_math::keyframe_track::lerp_float;

/// Vanilla `Mth.floor(float)`.
const fn floor(value: f32) -> i32 {
    let truncated = value as i32;
    if value < truncated as f32 {
        truncated - 1
    } else {
        truncated
    }
}

/// Vanilla `ARGB.as8BitChannel`.
#[must_use]
pub const fn as_8bit_channel(value: f32) -> i32 {
    floor(value * 255.0)
}

/// Vanilla `ARGB.color(int, int, int, int)`.
#[must_use]
pub const fn color(alpha: i32, red: i32, green: i32, blue: i32) -> u32 {
    ((alpha as u32 & 0xFF) << 24)
        | ((red as u32 & 0xFF) << 16)
        | ((green as u32 & 0xFF) << 8)
        | (blue as u32 & 0xFF)
}

/// Vanilla `ARGB.color(float, int)`: replaces the alpha of a packed color.
#[must_use]
pub const fn color_with_alpha(alpha: f32, rgb: u32) -> u32 {
    ((as_8bit_channel(alpha) as u32) << 24) | (rgb & 0x00FF_FFFF)
}

/// Vanilla `ARGB.colorFromFloat`.
#[must_use]
pub const fn color_from_float(alpha: f32, red: f32, green: f32, blue: f32) -> u32 {
    color(
        as_8bit_channel(alpha),
        as_8bit_channel(red),
        as_8bit_channel(green),
        as_8bit_channel(blue),
    )
}

const fn channel_float(color: u32, shift: u32) -> f32 {
    ((color >> shift) & 0xFF) as f32 / 255.0
}

/// Vanilla `ARGB.vector3fFromRGB24`.
#[must_use]
pub const fn vector3f_from_rgb24(color: u32) -> Vec3 {
    Vec3::new(
        channel_float(color, 16),
        channel_float(color, 8),
        channel_float(color, 0),
    )
}

/// Vanilla `ARGB.vector4fFromARGB32`.
#[must_use]
pub const fn vector4f_from_argb32(color: u32) -> Vec4 {
    Vec4::new(
        channel_float(color, 16),
        channel_float(color, 8),
        channel_float(color, 0),
        channel_float(color, 24),
    )
}

/// Vanilla `ARGB.srgbLerp` for RGB vectors.
#[must_use]
pub fn lerp_rgb(alpha: f32, from: &Vec3, to: &Vec3) -> Vec3 {
    Vec3::new(
        lerp_float(alpha, &from.x, &to.x),
        lerp_float(alpha, &from.y, &to.y),
        lerp_float(alpha, &from.z, &to.z),
    )
}

/// Vanilla `ARGB.srgbLerp` for ARGB vectors.
#[must_use]
pub fn lerp_argb(alpha: f32, from: &Vec4, to: &Vec4) -> Vec4 {
    Vec4::new(
        lerp_float(alpha, &from.x, &to.x),
        lerp_float(alpha, &from.y, &to.y),
        lerp_float(alpha, &from.z, &to.z),
        lerp_float(alpha, &from.w, &to.w),
    )
}

/// Vanilla `ARGB.multiply(Vector3fc, Vector3fc)`.
#[must_use]
pub fn multiply_rgb(lhs: Vec3, rhs: Vec3) -> Vec3 {
    if lhs == Vec3::ONE {
        return rhs;
    }
    if rhs == Vec3::ONE {
        return lhs;
    }
    lhs * rhs
}

/// Vanilla `ARGB.multiply(Vector4fc, Vector4fc)`.
#[must_use]
pub fn multiply_argb(lhs: Vec4, rhs: Vec4) -> Vec4 {
    if lhs == Vec4::ONE {
        return rhs;
    }
    if rhs == Vec4::ONE {
        return lhs;
    }
    lhs * rhs
}

/// Vanilla `ARGB.addRgb(Vector3fc, Vector3fc)`.
#[must_use]
pub fn add_rgb(lhs: Vec3, rhs: Vec3) -> Vec3 {
    Vec3::new(
        (lhs.x + rhs.x).min(1.0),
        (lhs.y + rhs.y).min(1.0),
        (lhs.z + rhs.z).min(1.0),
    )
}

/// Vanilla `ARGB.addRgb(Vector4fc, Vector3fc)`: keeps the alpha.
#[must_use]
pub fn add_rgb_keep_alpha(lhs: Vec4, rhs: Vec3) -> Vec4 {
    add_rgb(lhs.truncate(), rhs).extend(lhs.w)
}

/// Vanilla `ARGB.subtractRgb(Vector3fc, Vector3fc)`.
#[must_use]
pub fn subtract_rgb(lhs: Vec3, rhs: Vec3) -> Vec3 {
    Vec3::new(
        (lhs.x - rhs.x).max(0.0),
        (lhs.y - rhs.y).max(0.0),
        (lhs.z - rhs.z).max(0.0),
    )
}

/// Vanilla `ARGB.subtractRgb(Vector4fc, Vector3fc)`: keeps the alpha.
#[must_use]
pub fn subtract_rgb_keep_alpha(lhs: Vec4, rhs: Vec3) -> Vec4 {
    subtract_rgb(lhs.truncate(), rhs).extend(lhs.w)
}

/// Vanilla `ARGB.greyscale(Vector3fc)`.
#[must_use]
pub fn greyscale(color: Vec3) -> Vec3 {
    Vec3::splat(color.x * 0.3 + color.y * 0.59 + color.z * 0.11)
}

/// Vanilla `ARGB.scaleRGB(Vector3fc, float)`: clamps each channel to `[0, 1]`.
#[must_use]
pub fn scale_rgb(color: Vec3, scale: f32) -> Vec3 {
    Vec3::new(
        (color.x * scale).clamp(0.0, 1.0),
        (color.y * scale).clamp(0.0, 1.0),
        (color.z * scale).clamp(0.0, 1.0),
    )
}

/// Vanilla `ARGB.alphaBlend(Vector3fc, Vector4fc)`.
#[must_use]
pub fn alpha_blend_rgb(destination: Vec3, source: Vec4) -> Vec3 {
    let source_alpha = source.w;
    if source_alpha == 0.0 {
        return destination;
    }
    lerp_rgb(source_alpha, &destination, &source.truncate())
}

/// Vanilla `ARGB.alphaBlend(Vector4fc, Vector4fc)`.
#[must_use]
#[expect(
    clippy::float_cmp,
    reason = "vanilla short-circuits exactly opaque and transparent sources"
)]
pub fn alpha_blend_argb(destination: Vec4, source: Vec4) -> Vec4 {
    let destination_alpha = destination.w;
    let source_alpha = source.w;
    if source_alpha == 1.0 {
        return source;
    }
    if source_alpha == 0.0 {
        return destination;
    }

    let alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
    let channel = |destination: f32, source: f32| {
        (source * source_alpha + destination * (alpha - source_alpha)) / alpha
    };
    Vec4::new(
        channel(destination.x, source.x),
        channel(destination.y, source.y),
        channel(destination.z, source.z),
        alpha,
    )
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::{as_8bit_channel, color_from_float, color_with_alpha, vector3f_from_rgb24};

    #[test]
    fn packing_floors_channels_like_vanilla() {
        assert_eq!(as_8bit_channel(0.5), 127);
        assert_eq!(as_8bit_channel(0.48), 122);
        assert_eq!(color_from_float(1.0, 0.48, 0.48, 1.0), 0xFF7A_7AFF);
        assert_eq!(color_with_alpha(0.3125, 0xFF7A_7AFF), 0x4F7A_7AFF);
        assert_eq!(
            vector3f_from_rgb24(0xFF_FF7F),
            Vec3::new(1.0, 1.0, 127.0 / 255.0)
        );
    }
}
