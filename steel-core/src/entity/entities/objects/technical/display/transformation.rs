//! display transform NBT

use simdnbt::borrow::NbtTag as BorrowedNbtTag;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
pub use steel_math::transformation::Transformation;
use steel_utils::nbt::{
    mat4_from_nbt_tag, quat_from_nbt_tag, quat_to_nbt_tag, vec3_from_nbt_tag, vec3_to_nbt_tag,
};

pub(super) struct TransformationNbt(pub(super) Transformation);

impl TransformationNbt {
    fn from_compound(tag: BorrowedNbtTag) -> Option<Transformation> {
        let compound = tag.compound()?;
        Some(Transformation {
            translation: vec3_from_nbt_tag(compound.get("translation")?)?,
            left_rotation: quat_from_nbt_tag(compound.get("left_rotation")?)?,
            scale: vec3_from_nbt_tag(compound.get("scale")?)?,
            right_rotation: quat_from_nbt_tag(compound.get("right_rotation")?)?,
        })
    }
}

impl FromNbtTag for TransformationNbt {
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        let transformation = Self::from_compound(tag)
            .or_else(|| mat4_from_nbt_tag(tag).map(Transformation::from))?;
        Some(Self(transformation))
    }
}

impl ToNbtTag for TransformationNbt {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("translation", vec3_to_nbt_tag(self.0.translation));
        compound.insert("left_rotation", quat_to_nbt_tag(self.0.left_rotation));
        compound.insert("scale", vec3_to_nbt_tag(self.0.scale));
        compound.insert("right_rotation", quat_to_nbt_tag(self.0.right_rotation));
        NbtTag::Compound(compound)
    }
}
