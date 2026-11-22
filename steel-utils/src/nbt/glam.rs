use std::f32::consts::{FRAC_PI_2, PI, TAU};

use glam::{Mat4, Quat, Vec3};
use simdnbt::FromNbtTag;
use simdnbt::owned::{NbtList, NbtTag};

use crate::nbt::{NbtNumeric, nbt_collection_values};
use simdnbt::borrow::NbtTag as BorrowedNbtTag;

fn numeric_f32_list(tag: BorrowedNbtTag, len: usize) -> Option<Vec<f32>> {
    let values = nbt_collection_values(&tag.to_owned())?;
    let values: Vec<_> = values
        .iter()
        .filter_map(NbtNumeric::codec_f32)
        .take(len)
        .collect();

    (values.len() == len).then_some(values)
}

/// Converts a [`Vec3`] to its NBT representation (recreates `ExtraCodecs.VECTOR3F`).
#[must_use]
pub fn vec3_to_nbt_tag(vec3: Vec3) -> NbtTag {
    NbtList::Float(vec![vec3.x, vec3.y, vec3.z]).into()
}

/// Tries to convert a [`Vec3`] from its NBT representation (recreates `ExtraCodecs.VECTOR3F`).
#[must_use]
pub fn vec3_from_nbt_tag(tag: BorrowedNbtTag) -> Option<Vec3> {
    numeric_f32_list(tag, 3).map(|floats| Vec3::new(floats[0], floats[1], floats[2]))
}

/// A rotation storing an angle and axis (in 3 components).
///
/// This is only used to provide a more accurate codec implementation
/// for quaternions (`Quat`s).
struct AxisAngle4f {
    angle: f32,
    axis: Vec3,
}

impl FromNbtTag for AxisAngle4f {
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            angle: compound.get("angle")?.codec_f32()?,
            axis: vec3_from_nbt_tag(compound.get("axis")?)?,
        })
    }
}

impl From<AxisAngle4f> for Quat {
    fn from(value: AxisAngle4f) -> Self {
        let angle = if value.angle < 0.0 {
            (TAU + value.angle % TAU) % TAU
        } else {
            value.angle % TAU
        };

        let half_angle = angle * 0.5;
        let sin = f64::from(half_angle).sin() as f32;

        let mut cos = f64::from(1.0 - sin * sin).sqrt() as f32;
        let shift = half_angle + FRAC_PI_2;
        let mut quadrant = shift - (shift / TAU) as i32 as f32 * TAU;

        if quadrant < 0.0 {
            quadrant += TAU;
        }

        if quadrant >= PI {
            cos = -cos;
        }

        Self::from_xyzw(
            value.axis.x * sin,
            value.axis.y * sin,
            value.axis.z * sin,
            cos,
        )
    }
}

/// Convert a [`Quat`] to its NBT representation (recreates `ExtraCodecs.QUATERNIONF`).
#[must_use]
pub fn quat_to_nbt_tag(quat: Quat) -> NbtTag {
    NbtList::Float(vec![quat.x, quat.y, quat.z, quat.w]).into()
}

/// Tries to convert a [`Quat`] from its NBT representation (recreates `ExtraCodecs.QUATERNIONF`).
#[must_use]
pub fn quat_from_nbt_tag(tag: BorrowedNbtTag) -> Option<Quat> {
    // One of the two: 4 floats or AxisAngle4f
    if let Some(l) = numeric_f32_list(tag, 4) {
        let (x, y, z, w) = (l[0], l[1], l[2], l[3]);
        let length_squared = x * x + (y * y + (z * z + w * w));
        let inv_sqrt = 1.0 / (f64::from(length_squared).sqrt() as f32);

        return Some(Quat::from_xyzw(
            x * inv_sqrt,
            y * inv_sqrt,
            z * inv_sqrt,
            w * inv_sqrt,
        ));
    }
    Some(AxisAngle4f::from_nbt_tag(tag)?.into())
}

/// Tries to convert a [`Mat4`] from its NBT representation (recreates `ExtraCodecs.MATRIX4F`).
#[must_use]
pub fn mat4_from_nbt_tag(tag: BorrowedNbtTag) -> Option<Mat4> {
    let list = numeric_f32_list(tag, 16)?;
    let floats = list.as_array()?;
    Some(Mat4::from_cols_array(floats).transpose())
}
