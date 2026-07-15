//! Shared codec helpers for Vanilla RGB color components.

use simdnbt::owned::NbtTag;
use steel_utils::nbt::{NbtNumeric as _, nbt_collection_values};

pub(super) fn decode_rgb_color(tag: &NbtTag) -> Option<i32> {
    if let Some(color) = tag.codec_i32() {
        return Some(color);
    }
    let channels = nbt_collection_values(tag)?;
    let [red, green, blue] = channels.as_slice() else {
        return None;
    };
    Some(rgb_from_floats(
        red.codec_f32()?,
        green.codec_f32()?,
        blue.codec_f32()?,
    ))
}

fn rgb_from_floats(red: f32, green: f32, blue: f32) -> i32 {
    let red = java_floor(red * 255.0) & 0xff;
    let green = java_floor(green * 255.0) & 0xff;
    let blue = java_floor(blue * 255.0) & 0xff;
    (0xff00_0000_u32 | ((red as u32) << 16) | ((green as u32) << 8) | blue as u32) as i32
}

fn java_floor(value: f32) -> i32 {
    let truncated = value as i32;
    if value < truncated as f32 {
        truncated.wrapping_sub(1)
    } else {
        truncated
    }
}
