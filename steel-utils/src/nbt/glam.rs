use glam::{Mat4, Quat, Vec3};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};

use crate::nbt::NbtNumeric;
use simdnbt::borrow::NbtTag as BorrowedNbtTag;

fn numeric_f32_list(tag: BorrowedNbtTag, len: usize) -> Option<Vec<f32>> {
    if let Some(list) = tag.list() {
        return list
            .to_owned()
            .as_nbt_tags()
            .iter()
            .map(NbtNumeric::codec_f32)
            .take(len)
            .collect::<Option<Vec<_>>>()
            .filter(|v| v.len() >= len);
    }

    tag.byte_array()
        .map(|v| v.iter().map(|&v| f32::from(v as i8)).collect::<Vec<_>>())
        .or_else(|| {
            tag.int_array()
                .map(|v| v.into_iter().map(|v| v as f32).collect::<Vec<_>>())
        })
        .or_else(|| {
            tag.long_array()
                .map(|v| v.into_iter().map(|v| v as f32).collect::<Vec<_>>())
        })
        .filter(|v| v.len() >= len)
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
#[derive(Debug, Clone, Copy, PartialEq)]
struct AxisAngle4f {
    pub angle: f32,
    pub axis: Vec3,
}

impl ToNbtTag for AxisAngle4f {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("angle", self.angle);
        compound.insert("axis", vec3_to_nbt_tag(self.axis));
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for AxisAngle4f {
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            angle: f32::from_nbt_tag(compound.get("angle")?)?,
            axis: vec3_from_nbt_tag(compound.get("axis")?)?,
        })
    }
}

impl From<AxisAngle4f> for Quat {
    fn from(value: AxisAngle4f) -> Self {
        let half_angle = value.angle / 2.0;
        let sin = half_angle.sin();
        let cos = half_angle.cos();
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
        return Some(Quat::from_xyzw(l[0], l[1], l[2], l[3]).normalize());
    }
    Some(AxisAngle4f::from_nbt_tag(tag)?.into())
}

/// Tries to convert a [`Mat4`] from its NBT representation (recreates `ExtraCodecs.MATRIX4F`).
#[must_use]
pub fn mat4_to_nbt_tag(mat: Mat4) -> NbtTag {
    let elements = mat.transpose().to_cols_array();
    NbtList::Float(elements.to_vec()).into()
}

/// Tries to convert a [`Mat4`] from its NBT representation (recreates `ExtraCodecs.MATRIX4F`).
#[must_use]
pub fn mat4_from_nbt_tag(tag: BorrowedNbtTag) -> Option<Mat4> {
    let list = numeric_f32_list(tag, 16)?;
    let floats = list.as_array()?;
    Some(Mat4::from_cols_array(floats).transpose())
}
