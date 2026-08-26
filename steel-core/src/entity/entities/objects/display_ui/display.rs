//! Vanilla's abstract `Display` implementation.

use glam::Mat4;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::borrow::{NbtCompound as BorrowedNbtCompoundView};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_data::{Matrix4f, Quaternionf, Vector3f};
use steel_registry::vanilla_entity_data::DisplayEntityData;
use crate::entity::Entity;

#[repr(i8)]
#[derive(Debug, Clone, Copy)]
pub enum BillboardConstraints {
    Fixed = 0,
    Vertical = 1,
    Horizontal = 2,
    Center = 3
}

impl TryFrom<i8> for BillboardConstraints {
    type Error = ();

    fn try_from(value: i8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BillboardConstraints::Fixed),
            1 => Ok(BillboardConstraints::Vertical),
            2 => Ok(BillboardConstraints::Horizontal),
            3 => Ok(BillboardConstraints::Center),
            _ => Err(())
        }
    }
}

impl ToNbtTag for BillboardConstraints {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::String(
            match self {
                Self::Fixed => "fixed",
                Self::Vertical => "vertical",
                Self::Horizontal => "horizontal",
                Self::Center => "center"
            }.into()
        )
    }
}

impl FromNbtTag for BillboardConstraints {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        match tag.string()?.to_string().as_str() {
            "fixed" => Some(Self::Fixed),
            "vertical" => Some(Self::Vertical),
            "horizontal" => Some(Self::Horizontal),
            "center" => Some(Self::Center),
            _ => None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Brightness {
    block: i32,
    sky: i32
}

impl Brightness {
    pub fn pack(self) -> i32 {
        self.block << 4 | self.sky << 20
    }

    pub fn unpack(bits: i32) -> Brightness {
        Self {
            block: (bits >> 4) & 0b1111,
            sky: (bits >> 20) & 0b1111
        }
    }
}

impl ToNbtTag for Brightness {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("block", self.block);
        compound.insert("sky", self.sky);
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for Brightness {
    fn from_nbt_tag(tag: NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let block = compound.get("block")?.int()?;
        let range = 0..=15;
        if !range.contains(&block) { return None }
        let sky = compound.get("sky")?.int()?;
        if !range.contains(&sky) { return None }
        Some(Self { block, sky })
    }
}

/// A structure describing an affine transformation in 3D space.
///
/// Transformations are applied in the following order:
/// `translation` -> `left_rotation` -> `scale` -> `right_rotation`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transformation {
    /// The translation (displacement) applied by this transformation.
    pub translation: Vector3f,
    /// The left rotation applied by this transformation.
    pub left_rotation: Quaternionf,
    /// The scale applied by this transformation.
    pub scale: Vector3f,
    /// The right rotation applied by this transformation.
    pub right_rotation: Quaternionf
}

impl Transformation {
    /// The identity [`Transformation`].
    pub const IDENTITY: Self = Transformation {
        translation: Vector3f::ZERO,
        left_rotation: Quaternionf::IDENTITY,
        scale: Vector3f::ONE,
        right_rotation: Quaternionf::IDENTITY,
    };

    pub fn compose(self) -> Mat4 {
        Mat4::from_translation(self.translation.into())
            * Mat4::from_quat(self.left_rotation.into())
            * Mat4::from_scale(self.scale.into())
            * Mat4::from_quat(self.right_rotation.into())
    }
}

impl From<Mat4> for Transformation {
    /// Composes a [`Transformation`] with the provided matrix.
    fn from(_mat: Mat4) -> Self {
        // TODO: Implement svdDecompose()
        Transformation::IDENTITY
    }
}

impl From<Transformation> for Mat4 {
    /// Decomposes a [`Transformation`] to form a matrix.
    fn from(t: Transformation) -> Self {
        Mat4::from_translation(t.translation.into())
            * Mat4::from_quat(t.left_rotation.into())
            * Mat4::from_scale(t.scale.into())
            * Mat4::from_quat(t.right_rotation.into())
    }
}

struct NormalTransformation(Transformation);
impl From<Transformation> for NormalTransformation { fn from(t: Transformation) -> Self { Self(t)} }
impl From<NormalTransformation> for Transformation { fn from(t: NormalTransformation) -> Self { t.0 } }

// Recreates Vanilla's `Transformation.CODEC`.
impl ToNbtTag for NormalTransformation {
    fn to_nbt_tag(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("translation", self.0.translation.to_nbt_tag());
        compound.insert("left_rotation", self.0.left_rotation.to_nbt_tag());
        compound.insert("scale", self.0.scale.to_nbt_tag());
        compound.insert("right_rotation", self.0.right_rotation.to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for NormalTransformation {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self (Transformation {
            translation: Vector3f::from_nbt_tag(compound.get("transformation")?)?,
            left_rotation: Quaternionf::from_nbt_tag(compound.get("left_rotation")?)?,
            scale: Vector3f::from_nbt_tag(compound.get("scale")?)?,
            right_rotation: Quaternionf::from_nbt_tag(compound.get("right_rotation")?)?,
        }))
    }
}

// Recreates Vanilla's `Transformation.EXTENDED_CODEC`.
// This codec prefers using the ordinary codec created above, but it also accepts a matrix.
impl FromNbtTag for Transformation {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(NormalTransformation(transformation)) = NormalTransformation::from_nbt_tag(tag) {
            return Some(transformation);
        }
        Some(Matrix4f::from_nbt_tag(tag)?.into())
    }
}

/// The abstract display trait, used by block, item and display entities.
pub trait Display: Entity {
    fn tick_display(&self) {
        /// TODO: Implement dismounting if the vehicle of the display entity is removed.
    }

    fn display_entity_data(&self) -> &DisplayEntityData;

    fn display_entity_data_mut(&self) -> &mut DisplayEntityData;

    fn set_transformation(&self, transformation: Transformation) {
        let data = self.display_entity_data_mut();

        data.translation.set(transformation.translation);
        data.left_rotation.set(transformation.left_rotation);
        data.scale.set(transformation.scale);
        data.right_rotation.set(transformation.right_rotation);
    }

    fn transformation(&self) -> Transformation {
        let data = self.display_entity_data();
        Transformation {
            translation: *data.translation.get(),
            left_rotation: *data.left_rotation.get(),
            scale: *data.scale.get(),
            right_rotation: *data.right_rotation.get()
        }
    }

    fn load_additional_display(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_transformation(Transformation::from_nbt_tag(nbt.list("transformation").into()).unwrap_or_else(|| Transformation::IDENTITY));

        let mut data = self.display_entity_data_mut();
        data.transformation_interpolation_duration.set(nbt.int("interpolation_duration").unwrap_or(0));
        data.transformation_interpolation_start_delta_ticks.set(nbt.int("start_interpolation").unwrap_or(0));
        let teleport_duration = nbt.int("teleport_duration").unwrap_or(0);
        data.transformation_interpolation_duration.set(teleport_duration.clamp(0, 59));
        data.billboard_render_constraints.set(nbt.get("billboard").and_then(BillboardConstraints::from_nbt_tag).unwrap_or(BillboardConstraints::Fixed) as i8);
        data.view_range.set(nbt.float("view_range").unwrap_or(1.0));
        data.shadow_radius.set(nbt.float("shadow_radius").unwrap_or(0.0));
        data.shadow_strength.set(nbt.float("shadow_strength").unwrap_or(1.0));
        data.width.set(nbt.float("width").unwrap_or(0.0));
        data.height.set(nbt.float("height").unwrap_or(0.0));
        data.glow_color_override.set(nbt.int("glow_color_override").unwrap_or(-1));
        data.brightness_override.set(nbt.get("brightness").and_then(Brightness::from_nbt_tag).map(Brightness::pack).unwrap_or(-1));
    }

    fn save_additional_display(&self, nbt: &mut NbtCompound) {
        nbt.insert("transformation", &self.transformation());

        let data = self.display_entity_data();
        nbt.insert("billboard", BillboardConstraints::try_from(data.transformation_interpolation_duration.get()).unwrap_or(BillboardConstraints::Fixed));
        nbt.insert("interpolation_duration", data.transformation_interpolation_duration.get());
        nbt.insert("teleport_duration", data.pos_rot_interpolation_duration.get());
        nbt.insert("view_range", data.view_range.get());
        nbt.insert("shadow_radius", data.shadow_radius.get());
        nbt.insert("shadow_strength", data.shadow_strength.get());
        nbt.insert("width", data.width.get());
        nbt.insert("height", data.height.get());
        nbt.insert("glow_color_override", data.glow_color_override.get());
        nbt.insert("brightness", Brightness::unpack(*data.brightness_override.get()));
    }

    fn hurt_display(&self) -> bool { false }
    fn piston_push_reaction(&self) -> PushReaction { PushReaction::Ignore }
    fn is_ignoring_block_triggers(&self) -> bool { true }

    /// TODO: Add `getTeamColor()` when team foundations exist.
}