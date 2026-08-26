//! Vanilla's abstract `Display` implementation.

use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::World;
use glam::Mat4;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_data::{Matrix4f, Quaternionf, Vector3f};

/// A private trait, only used by display entities, to get and set
/// some synced entity data.
trait PrivateDisplay {
    fn synced_billboard_constraints(&self) -> i8;
    fn set_synced_billboard_constraints(&self, constraints: i8);

    fn synced_brightness_override(&self) -> i32;
    fn set_synced_brightness_override(&self, brightness: i32);

    fn synced_glow_color_override(&self) -> i32;
    fn set_synced_glow_color_override(&self, glow_color: i32);
}

/// The abstract display trait used by all display entities.
///
/// Display entities have:
/// - A [`Transformation`] (containing how the display of the entity is transformed)
/// - A billboard value to control how a display entity looks at players.
/// - A brightness and glow color override.
/// - A maximum and minimum height and width (if set).
/// - Interpolation properties, like the duration of a transformation interpolation, its delay and the duration
///   of a teleport interpolation.
/// - A shadow radius and strength.
/// - A maximum view range.
#[expect(
    private_bounds,
    reason = "outside crates and plugins should not work with raw synced values"
)]
pub trait Display: Entity + PrivateDisplay {
    /// The base `tick()` method for display entities.
    fn tick_display(&self) {
        // TODO: Implement dismounting if the vehicle of the display entity is removed.
    }
    /// The base `hurtServer()` method for display entities.
    fn hurt_display(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        false
    }
    /// The base `pistonPushReaction()` method for display entities.
    fn piston_push_reaction_display(&self) -> PushReaction {
        PushReaction::Ignore
    }
    /// The base `isIgnoringBlockTriggers()` method for display entities.
    fn is_ignoring_block_triggers_display(&self) -> bool {
        true
    }

    /// Gets the [`Transformation`] of this display entity.
    fn transformation(&self) -> Transformation;
    /// Sets the [`Transformation`] of this display entity to `transformation`.
    fn set_transformation(&self, transformation: Transformation);

    /// Gets this display entity's *interpolation duration* (the time to interpolate to a new transformation), in ticks.
    fn transformation_interpolation_duration(&self) -> i32;
    /// Sets this display entity's *interpolation duration* (the time to interpolate to a new transformation), in ticks, to `duration`.
    fn set_transformation_interpolation_duration(&self, duration: i32);
    /// Gets this display entity's *teleport duration* (the time to interpolate to a new position due to a teleport), in ticks.
    ///
    /// Values are clamped to be between `0` and `59` ticks, inclusive.
    ///
    /// **Note:** This property is not saved to disk.
    fn transformation_interpolation_delay(&self) -> i32;
    /// Sets this display entity's *start interpolation delay* (the delay in starting an interpolation), in ticks, to `duration`.
    ///
    /// If this is set to `0`, interpolation starts immediately.
    ///
    /// **Note:** This property is not saved to disk.
    fn set_transformation_interpolation_delay(&self, duration: i32);
    /// Gets this display entity's *start interpolation delay* (the delay in starting an interpolation), in ticks.
    fn pos_rot_interpolation_duration(&self) -> i32;
    /// Sets this display entity's *teleport duration* (the time to interpolate to a new position due to a teleport), in ticks, to `duration`.
    fn set_pos_rot_interpolation_duration(&self, duration: i32);

    /// Gets the billboard constraints of this display entity.
    fn billboard_constraints(&self) -> BillboardConstraints {
        BillboardConstraints::try_from(self.synced_billboard_constraints())
            .unwrap_or(BillboardConstraints::Fixed)
    }
    /// Sets this display entity's billboard constraints to `constraints`.
    fn set_billboard_constraints(&self, constraints: BillboardConstraints) {
        self.set_synced_billboard_constraints(constraints as i8);
    }

    /// Gets this display entity's billboard constraints.
    fn brightness_override(&self) -> Option<Brightness> {
        let synced = self.synced_brightness_override();
        (synced != -1).then(|| Brightness::unpack(synced))
    }
    /// Sets this display entity's brightness override to `brightness`.
    fn set_brightness_override(&self, brightness: Option<Brightness>) {
        self.set_synced_brightness_override(brightness.map_or(-1, Brightness::pack));
    }

    /// Gets this display entity's maximum view range.
    fn view_range(&self) -> f32;
    /// Sets this display entity's maximum view range to `range`.
    fn set_view_range(&self, range: f32);
    /// Gets this display entity's shadow radius.
    ///
    /// **Note:** This property is interpolated.
    fn shadow_radius(&self) -> f32;
    /// Sets this display entity's shadow radius to `size`.
    ///
    /// **Note:** This property is interpolated.
    fn set_shadow_radius(&self, size: f32);
    /// Sets this display entity's shadow strength (which affects the opacity of the display entity's shadow depending on its distance to the block below).
    ///
    /// **Note:** This property is interpolated.
    fn shadow_strength(&self) -> f32;
    /// Sets this display entity's shadow strength (which affects the opacity of the display entity's shadow depending on its distance to the block below) to `strength`.
    ///
    /// **Note:** This property is interpolated.
    fn set_shadow_strength(&self, strength: f32);
    /// Gets this display entity's maximum width.
    fn width(&self) -> f32;
    /// Sets this display entity's maximum width to `width`.
    ///
    /// Setting this to `0` indicates no culling on the horizontal axis.
    fn set_width(&self, width: f32);
    /// Gets this display entity's maximum height.
    fn height(&self) -> f32;
    /// Sets this display entity's maximum height to `height`.
    ///
    /// Setting this to `0` indicates no culling on the vertical axis.
    fn set_height(&self, height: f32);
    /// Gets this display entity's glow color override. If this is `None`, the entity glows according to its team's color.
    ///
    /// **Note:** This has no effect on *text displays*.
    fn glow_color_override(&self) -> Option<i32> {
        let color = self.synced_glow_color_override();
        (color != -1).then_some(color)
    }
    /// Sets this display entity's glow color override to `value`. If this is `None`, the entity glows according to its team's color.
    ///
    /// **Note:** This has no effect on *text displays*.
    fn set_glow_color_override(&self, value: Option<i32>) {
        self.set_synced_glow_color_override(value.unwrap_or(-1));
    }

    /// Loads this display entity's fields common to all display entities from an NBT compound.
    fn load_display(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_transformation(
            nbt.get("transformation")
                .and_then(Transformation::from_nbt_tag)
                .unwrap_or(Transformation::IDENTITY),
        );

        self.set_transformation_interpolation_duration(
            nbt.int("interpolation_duration").unwrap_or(0),
        );
        self.set_transformation_interpolation_delay(nbt.int("start_interpolation").unwrap_or(0));
        self.set_pos_rot_interpolation_duration(
            nbt.int("teleport_duration").unwrap_or(0).clamp(0, 59),
        );
        self.set_billboard_constraints(
            nbt.get("billboard")
                .and_then(BillboardConstraints::from_nbt_tag)
                .unwrap_or(BillboardConstraints::Fixed),
        );
        self.set_view_range(nbt.float("view_range").unwrap_or(1.0));
        self.set_shadow_radius(nbt.float("shadow_radius").unwrap_or(0.0));
        self.set_shadow_strength(nbt.float("shadow_strength").unwrap_or(1.0));
        self.set_width(nbt.float("width").unwrap_or(0.0));
        self.set_height(nbt.float("height").unwrap_or(0.0));
        self.set_synced_glow_color_override(nbt.int("glow_color_override").unwrap_or(-1));
        self.set_brightness_override(nbt.get("brightness").and_then(Brightness::from_nbt_tag));
    }

    /// Saves this display entity's fields common to all display entities to an NBT compound.
    fn save_display(&self, nbt: &mut NbtCompound) {
        nbt.insert("transformation", self.transformation());

        nbt.insert("billboard", self.billboard_constraints());
        nbt.insert(
            "interpolation_duration",
            self.transformation_interpolation_duration(),
        );
        nbt.insert("teleport_duration", self.pos_rot_interpolation_duration());
        nbt.insert("view_range", self.view_range());
        nbt.insert("shadow_radius", self.shadow_radius());
        nbt.insert("shadow_strength", self.shadow_strength());
        nbt.insert("width", self.width());
        nbt.insert("height", self.height());
        nbt.insert(
            "glow_color_override",
            self.glow_color_override().unwrap_or(-1),
        );
        if let Some(brightness) = self.brightness_override() {
            nbt.insert("brightness", brightness);
        }
    }

    // TODO: Add `getTeamColor()` when team foundations exist.
}

macro_rules! display_impl {
    ($entity:ident) => {
        impl PrivateDisplay for $entity {
            fn synced_billboard_constraints(&self) -> i8 {
                *self
                    .entity_data
                    .lock()
                    .display
                    .billboard_render_constraints
                    .get()
            }

            fn set_synced_billboard_constraints(&self, constraints: i8) {
                self.entity_data
                    .lock()
                    .display
                    .billboard_render_constraints
                    .set(constraints)
            }

            fn synced_brightness_override(&self) -> i32 {
                *self.entity_data.lock().display.brightness_override.get()
            }

            fn set_synced_brightness_override(&self, brightness: i32) {
                self.entity_data
                    .lock()
                    .display
                    .brightness_override
                    .set(brightness)
            }

            fn synced_glow_color_override(&self) -> i32 {
                *self.entity_data.lock().display.brightness_override.get()
            }

            fn set_synced_glow_color_override(&self, brightness: i32) {
                self.entity_data
                    .lock()
                    .display
                    .glow_color_override
                    .set(brightness)
            }
        }

        impl Display for $entity {
            fn set_transformation(&self, transformation: Transformation) {
                let mut data = self.entity_data.lock();
                data.display.translation.set(transformation.translation);
                data.display.left_rotation.set(transformation.left_rotation);
                data.display.scale.set(transformation.scale);
                data.display
                    .right_rotation
                    .set(transformation.right_rotation);
            }

            fn transformation(&self) -> Transformation {
                let data = self.entity_data.lock();
                Transformation {
                    translation: *data.display.translation.get(),
                    left_rotation: *data.display.left_rotation.get(),
                    scale: *data.display.scale.get(),
                    right_rotation: *data.display.right_rotation.get(),
                }
            }

            fn transformation_interpolation_duration(&self) -> i32 {
                *self
                    .entity_data
                    .lock()
                    .display
                    .transformation_interpolation_duration
                    .get()
            }

            fn set_transformation_interpolation_duration(&self, duration: i32) {
                self.entity_data
                    .lock()
                    .display
                    .transformation_interpolation_duration
                    .set(duration)
            }

            fn transformation_interpolation_delay(&self) -> i32 {
                *self
                    .entity_data
                    .lock()
                    .display
                    .transformation_interpolation_start_delta_ticks
                    .get()
            }

            fn set_transformation_interpolation_delay(&self, ticks: i32) {
                self.entity_data
                    .lock()
                    .display
                    .transformation_interpolation_start_delta_ticks
                    .set(ticks)
            }

            fn pos_rot_interpolation_duration(&self) -> i32 {
                *self
                    .entity_data
                    .lock()
                    .display
                    .pos_rot_interpolation_duration
                    .get()
            }

            fn set_pos_rot_interpolation_duration(&self, duration: i32) {
                self.entity_data
                    .lock()
                    .display
                    .pos_rot_interpolation_duration
                    .set(duration)
            }

            fn view_range(&self) -> f32 {
                *self.entity_data.lock().display.view_range.get()
            }

            fn set_view_range(&self, range: f32) {
                self.entity_data.lock().display.view_range.set(range)
            }

            fn shadow_radius(&self) -> f32 {
                *self.entity_data.lock().display.shadow_radius.get()
            }

            fn set_shadow_radius(&self, size: f32) {
                self.entity_data.lock().display.shadow_radius.set(size)
            }

            fn shadow_strength(&self) -> f32 {
                *self.entity_data.lock().display.shadow_strength.get()
            }

            fn set_shadow_strength(&self, strength: f32) {
                self.entity_data
                    .lock()
                    .display
                    .shadow_strength
                    .set(strength)
            }

            fn width(&self) -> f32 {
                *self.entity_data.lock().display.width.get()
            }

            fn set_width(&self, width: f32) {
                self.entity_data.lock().display.width.set(width)
            }

            fn height(&self) -> f32 {
                *self.entity_data.lock().display.height.get()
            }

            fn set_height(&self, height: f32) {
                self.entity_data.lock().display.height.set(height)
            }
        }
    };
}

/// Controls how a display entity looks at a player (from their client).
///
/// Each value controls whether a display entity follows the player along
/// the horizontal axis and the vertical axis.
#[repr(i8)]
#[derive(Default, Debug, Clone, Copy)]
pub enum BillboardConstraints {
    #[default]
    /// Both the horizontal and vertical axes are fixed.
    Fixed = 0,
    /// Only the vertical axis is fixed.
    Vertical = 1,
    /// Only the horizontal axis is fixed.
    Horizontal = 2,
    /// Neither the horizontal nor the vertical axis is fixed.
    Center = 3,
}

impl TryFrom<i8> for BillboardConstraints {
    type Error = ();

    fn try_from(value: i8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Fixed),
            1 => Ok(Self::Vertical),
            2 => Ok(Self::Horizontal),
            3 => Ok(Self::Center),
            _ => Err(()),
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
                Self::Center => "center",
            }
            .into(),
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
            _ => None,
        }
    }
}

/// A set of brightness (light) levels to override how bright a display entity looks.
///
/// It contains a block light level and skylight level.
#[derive(Debug, Clone, Copy)]
pub struct Brightness {
    /// The block light level.
    pub block: i32,
    /// The skylight level.
    pub sky: i32,
}

impl Brightness {
    /// Packs this [`Brightness`] into a single `i32`.
    #[must_use]
    pub const fn pack(self) -> i32 {
        self.block << 4 | self.sky << 20
    }

    /// Unpacks a [`Brightness`] from a single `i32`.
    #[must_use]
    pub const fn unpack(bits: i32) -> Brightness {
        Self {
            block: (bits >> 4) & 0b1111,
            sky: (bits >> 20) & 0b1111,
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
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let block = compound.get("block")?.int()?;
        let range = 0..=15;
        if !range.contains(&block) {
            return None;
        }
        let sky = compound.get("sky")?.int()?;
        if !range.contains(&sky) {
            return None;
        }
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
    pub right_rotation: Quaternionf,
}

impl Transformation {
    /// The identity [`Transformation`].
    pub const IDENTITY: Self = Transformation {
        translation: Vector3f::ZERO,
        left_rotation: Quaternionf::IDENTITY,
        scale: Vector3f::ONE,
        right_rotation: Quaternionf::IDENTITY,
    };

    /// Composes a [`Matrix4f`] from this transformation.
    #[must_use]
    pub fn compose(self) -> Matrix4f {
        Matrix4f(
            Mat4::from_translation(self.translation.into())
                * Mat4::from_quat(self.left_rotation.into())
                * Mat4::from_scale(self.scale.into())
                * Mat4::from_quat(self.right_rotation.into()),
        )
    }
}

impl From<Matrix4f> for Transformation {
    /// Decomposes a [`Matrix4f`] to form a [`Transformation`].
    fn from(_mat: Matrix4f) -> Self {
        // TODO: Implement svdDecompose()
        Transformation::IDENTITY
    }
}

impl From<Transformation> for Matrix4f {
    /// Composes a [`Transformation`] to form a matrix.
    fn from(t: Transformation) -> Self {
        Transformation::compose(t)
    }
}

struct NormalTransformation(Transformation);
impl From<Transformation> for NormalTransformation {
    fn from(t: Transformation) -> Self {
        Self(t)
    }
}
impl From<NormalTransformation> for Transformation {
    fn from(t: NormalTransformation) -> Self {
        t.0
    }
}

// Recreates Vanilla's `Transformation.CODEC`.
impl ToNbtTag for NormalTransformation {
    fn to_nbt_tag(self) -> NbtTag {
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
        Some(Self(Transformation {
            translation: Vector3f::from_nbt_tag(compound.get("transformation")?)?,
            left_rotation: Quaternionf::from_nbt_tag(compound.get("left_rotation")?)?,
            scale: Vector3f::from_nbt_tag(compound.get("scale")?)?,
            right_rotation: Quaternionf::from_nbt_tag(compound.get("right_rotation")?)?,
        }))
    }
}

// Recreates Vanilla's `Transformation.EXTENDED_CODEC`.
// This codec prefers using the ordinary codec created above, but it does also accept a matrix.
impl FromNbtTag for Transformation {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(NormalTransformation(transformation)) = NormalTransformation::from_nbt_tag(tag)
        {
            return Some(transformation);
        }
        Some(Matrix4f::from_nbt_tag(tag)?.into())
    }
}

impl ToNbtTag for Transformation {
    fn to_nbt_tag(self) -> NbtTag {
        NormalTransformation(self).to_nbt_tag()
    }
}

pub mod block_display;
pub mod item_display;
pub mod text_display;
