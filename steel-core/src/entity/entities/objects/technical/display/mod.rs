//! Vanilla `Display`: what block, item and text displays share.
//!
//! The state lives in the synced `Display` data layer, reached through the
//! [`Display`] view; the [`DisplayBase`] component saves it. Entity types hold
//! the component and start their properties from [`entity_props`].

pub mod block_display;
pub mod item_display;
pub mod text_display;
pub mod transformation;

use glam::Mat4;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::vanilla_entity_data::DisplayEntityData;
use steel_utils::nbt::{NbtNumeric as _, NbtValueInput as _};
use steel_utils::{DowncastType, DowncastTypeKey};

pub use self::transformation::Transformation;
use self::transformation::TransformationNbt;
use crate::chunk::light::MAX_LIGHT_LEVEL;
use crate::entity::component::{Component, Components as _, Has};
use crate::entity::prelude::*;
use crate::entity::{Entity, EntityProps};

/// The default interpolation duration of a display entity.
pub const DEFAULT_TRANSFORMATION_INTERPOLATION_DURATION: i32 = 0;

/// The default delay in interpolation of a display entity.
pub const DEFAULT_TRANSFORMATION_INTERPOLATION_DELAY: i32 = 0;

/// The default teleport duration of a display entity.
pub const DEFAULT_POS_ROT_INTERPOLATION_DURATION: i32 = 0;
const MAX_POS_ROT_INTERPOLATION_DURATION: i32 = 59;

/// The default [`BillboardConstraints`] of a display entity.
pub const DEFAULT_BILLBOARD_CONSTRAINTS: BillboardConstraints = BillboardConstraints::Fixed;

/// The default view range of a display entity.
pub const DEFAULT_VIEW_RANGE: f32 = 1.0;

/// The default shadow radius of a display entity.
pub const DEFAULT_SHADOW_RADIUS: f32 = 0.0;

/// The default shadow strength of a display entity.
pub const DEFAULT_SHADOW_STRENGTH: f32 = 1.0;

/// The default width of a display entity.
pub const DEFAULT_WIDTH: f32 = 0.0;

/// The default height of a display entity.
pub const DEFAULT_HEIGHT: f32 = 0.0;

/// The default glow color override of a display entity. `-1` corresponds to no override.
pub const DEFAULT_GLOW_COLOR_OVERRIDE: i32 = -1;

/// The synced packed brightness of a display entity without an override.
const NO_BRIGHTNESS_OVERRIDE: i32 = -1;

/// Marks a vanilla `Display` type and saves the state its types share.
pub struct DisplayBase;

// SAFETY: This key is owned by Steel and uniquely identifies `DisplayBase`.
unsafe impl DowncastType for DisplayBase {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity_component/display");
}

impl Component for DisplayBase {
    fn save(&self, owner: &dyn Entity, nbt: &mut NbtCompound) {
        Display::new(owner).save(nbt);
    }

    fn load(&self, owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        Display::new(owner).load(nbt);
    }
}

/// A concrete display type.
pub trait Parts: Entity + Has<DisplayBase> {
    /// Returns this entity as a display.
    fn display(&self) -> Display<'_>;
}

impl<E: Entity + Has<DisplayBase>> Parts for E {
    fn display(&self) -> Display<'_> {
        Display::new(self)
    }
}

/// An entity seen as a vanilla `Display`.
///
/// Display entities have:
/// - A [`Transformation`] (containing how the display of the entity is transformed)
/// - A billboard value to control how a display entity looks at players.
/// - A brightness and glow color override.
/// - Width and height for the client culling box.
/// - Interpolation properties, like the duration of a transformation interpolation, its delay and the duration
///   of a teleport interpolation.
/// - A shadow radius and strength.
/// - A client view range multiplier.
///
/// Every accessor locks the entity's synced data for the length of the call.
#[derive(Clone, Copy)]
pub struct Display<'a> {
    entity: &'a dyn Entity,
}

impl<'a> Display<'a> {
    /// Views `entity`, which holds a [`DisplayBase`], as a display.
    const fn new(entity: &'a dyn Entity) -> Self {
        Self { entity }
    }

    /// Returns `entity` as a display, if it is one.
    #[must_use]
    pub fn of(entity: &'a dyn Entity) -> Option<Self> {
        entity.query::<DisplayBase>().map(|_| Self { entity })
    }

    /// Returns the entity this view belongs to.
    #[must_use]
    pub const fn entity(&self) -> &'a dyn Entity {
        self.entity
    }

    fn with_synced<R>(&self, f: impl FnOnce(&mut DisplayEntityData) -> R) -> Option<R> {
        let synced = self.entity.synced_data()?;
        let mut data = synced.lock_data();
        data.display_layer_mut().map(f)
    }

    /// Gets the [`Transformation`] of the display entity.
    ///
    /// **Note:** This property is interpolated.
    #[must_use]
    pub fn transformation(&self) -> Transformation {
        self.with_synced(|data| Transformation {
            translation: *data.translation.get(),
            left_rotation: *data.left_rotation.get(),
            scale: *data.scale.get(),
            right_rotation: *data.right_rotation.get(),
        })
        .unwrap_or(Transformation::IDENTITY)
    }

    /// Sets the [`Transformation`] of the display entity to `transformation`.
    ///
    /// **Note:** This property is interpolated.
    pub fn set_transformation(&self, transformation: Transformation) {
        self.with_synced(|data| {
            data.translation.set(transformation.translation);
            data.left_rotation.set(transformation.left_rotation);
            data.scale.set(transformation.scale);
            data.right_rotation.set(transformation.right_rotation);
        });
    }

    /// Sets the transformation matrix of the display entity to `mat`.
    pub fn set_transformation_matrix(&self, mat: impl Into<Mat4>) {
        self.set_transformation(Transformation::decompose(mat.into()));
    }

    /// Gets the display entity's *interpolation duration* (the time to interpolate to a new transformation), in ticks.
    #[must_use]
    pub fn transformation_interpolation_duration(&self) -> i32 {
        self.with_synced(|data| *data.transformation_interpolation_duration.get())
            .unwrap_or(DEFAULT_TRANSFORMATION_INTERPOLATION_DURATION)
    }

    /// Sets the display entity's *interpolation duration* (the time to interpolate to a new transformation), in ticks, to `duration`.
    pub fn set_transformation_interpolation_duration(&self, duration: i32) {
        self.with_synced(|data| data.transformation_interpolation_duration.set(duration));
    }

    /// Gets the delay before starting a transformation interpolation, in ticks.
    ///
    /// This value is not saved to disk.
    #[must_use]
    pub fn transformation_interpolation_delay(&self) -> i32 {
        self.with_synced(|data| *data.transformation_interpolation_start_delta_ticks.get())
            .unwrap_or(DEFAULT_TRANSFORMATION_INTERPOLATION_DELAY)
    }

    /// Sets the delay before starting a transformation interpolation, in ticks.
    ///
    /// Sending the same delay still restarts the animation. This value is not saved to disk.
    pub fn set_transformation_interpolation_delay(&self, duration: i32) {
        self.with_synced(|data| {
            data.transformation_interpolation_start_delta_ticks
                .set_and_force_dirty(duration, true);
        });
    }

    /// Gets the position and rotation interpolation duration, in ticks.
    #[must_use]
    pub fn pos_rot_interpolation_duration(&self) -> i32 {
        self.with_synced(|data| *data.pos_rot_interpolation_duration.get())
            .unwrap_or(DEFAULT_POS_ROT_INTERPOLATION_DURATION)
    }

    /// Sets the position and rotation interpolation duration, in ticks.
    ///
    /// Display NBT clamps this value to `0..=59` when loading.
    pub fn set_pos_rot_interpolation_duration(&self, duration: i32) {
        self.with_synced(|data| data.pos_rot_interpolation_duration.set(duration));
    }

    /// Gets the billboard constraints of the display entity.
    #[must_use]
    pub fn billboard_constraints(&self) -> BillboardConstraints {
        self.with_synced(|data| *data.billboard_render_constraints.get())
            .and_then(|id| BillboardConstraints::try_from(id).ok())
            .unwrap_or(DEFAULT_BILLBOARD_CONSTRAINTS)
    }

    /// Sets the display entity's billboard constraints to `constraints`.
    pub fn set_billboard_constraints(&self, constraints: BillboardConstraints) {
        self.with_synced(|data| data.billboard_render_constraints.set(constraints as i8));
    }

    /// Gets the packed-light override as block and sky light levels.
    #[must_use]
    pub fn brightness_override(&self) -> Option<Brightness> {
        self.with_synced(|data| *data.brightness_override.get())
            .filter(|&packed| packed != NO_BRIGHTNESS_OVERRIDE)
            .map(Brightness::unpack)
    }

    /// Sets the display entity's brightness override to `brightness`.
    pub fn set_brightness_override(&self, brightness: Option<Brightness>) {
        let packed = brightness.map_or(NO_BRIGHTNESS_OVERRIDE, Brightness::pack);
        self.with_synced(|data| data.brightness_override.set(packed));
    }

    /// Gets the display entity's client view range multiplier.
    #[must_use]
    pub fn view_range(&self) -> f32 {
        self.with_synced(|data| *data.view_range.get())
            .unwrap_or(DEFAULT_VIEW_RANGE)
    }

    /// Sets the display entity's client view range multiplier to `range`.
    pub fn set_view_range(&self, range: f32) {
        self.with_synced(|data| data.view_range.set(range));
    }

    /// Gets the display entity's shadow radius.
    ///
    /// **Note:** This property is interpolated.
    #[must_use]
    pub fn shadow_radius(&self) -> f32 {
        self.with_synced(|data| *data.shadow_radius.get())
            .unwrap_or(DEFAULT_SHADOW_RADIUS)
    }

    /// Sets the display entity's shadow radius to `size`.
    ///
    /// **Note:** This property is interpolated.
    pub fn set_shadow_radius(&self, size: f32) {
        self.with_synced(|data| data.shadow_radius.set(size));
    }

    /// Gets the display entity's shadow strength.
    ///
    /// **Note:** This property is interpolated.
    #[must_use]
    pub fn shadow_strength(&self) -> f32 {
        self.with_synced(|data| *data.shadow_strength.get())
            .unwrap_or(DEFAULT_SHADOW_STRENGTH)
    }

    /// Sets the display entity's shadow strength to `strength`.
    ///
    /// **Note:** This property is interpolated.
    pub fn set_shadow_strength(&self, strength: f32) {
        self.with_synced(|data| data.shadow_strength.set(strength));
    }

    /// Gets the width of the client culling box.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.with_synced(|data| *data.width.get())
            .unwrap_or(DEFAULT_WIDTH)
    }

    /// Sets the width of the client culling box.
    ///
    /// A zero width or height disables frustum culling.
    pub fn set_width(&self, width: f32) {
        self.with_synced(|data| data.width.set(width));
    }

    /// Gets the height of the client culling box.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.with_synced(|data| *data.height.get())
            .unwrap_or(DEFAULT_HEIGHT)
    }

    /// Sets the height of the client culling box.
    ///
    /// A zero width or height disables frustum culling.
    pub fn set_height(&self, height: f32) {
        self.with_synced(|data| data.height.set(height));
    }

    /// Gets the display entity's glow color override. If this is `None`, the entity glows according to its team's color.
    ///
    /// **Note:** This has no effect on *text displays*.
    #[must_use]
    pub fn glow_color_override(&self) -> Option<i32> {
        let color = self.synced_glow_color_override();
        (color != DEFAULT_GLOW_COLOR_OVERRIDE).then_some(color)
    }

    /// Sets the display entity's glow color override to `value`. If this is `None`, the entity glows according to its team's color.
    ///
    /// **Note:** This has no effect on *text displays*.
    pub fn set_glow_color_override(&self, value: Option<i32>) {
        self.set_synced_glow_color_override(value.unwrap_or(DEFAULT_GLOW_COLOR_OVERRIDE));
    }

    fn synced_glow_color_override(&self) -> i32 {
        self.with_synced(|data| *data.glow_color_override.get())
            .unwrap_or(DEFAULT_GLOW_COLOR_OVERRIDE)
    }

    fn set_synced_glow_color_override(&self, value: i32) {
        self.with_synced(|data| data.glow_color_override.set(value));
    }

    fn save(&self, nbt: &mut NbtCompound) {
        nbt.insert("transformation", TransformationNbt(self.transformation()));

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
        nbt.insert("glow_color_override", self.synced_glow_color_override());
        if let Some(brightness) = self.brightness_override() {
            nbt.insert("brightness", brightness);
        }
    }

    fn load(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_transformation(
            nbt.get("transformation")
                .and_then(TransformationNbt::from_nbt_tag)
                .map_or(Transformation::IDENTITY, |tag| tag.0),
        );

        self.set_transformation_interpolation_duration(
            nbt.int_value("interpolation_duration")
                .unwrap_or(DEFAULT_TRANSFORMATION_INTERPOLATION_DURATION),
        );
        self.set_transformation_interpolation_delay(
            nbt.int_value("start_interpolation")
                .unwrap_or(DEFAULT_TRANSFORMATION_INTERPOLATION_DELAY),
        );
        self.set_pos_rot_interpolation_duration(
            nbt.int_value("teleport_duration")
                .unwrap_or(DEFAULT_POS_ROT_INTERPOLATION_DURATION)
                .clamp(0, MAX_POS_ROT_INTERPOLATION_DURATION),
        );
        self.set_billboard_constraints(
            nbt.get("billboard")
                .and_then(BillboardConstraints::from_nbt_tag)
                .unwrap_or(DEFAULT_BILLBOARD_CONSTRAINTS),
        );
        self.set_view_range(nbt.float_value("view_range").unwrap_or(DEFAULT_VIEW_RANGE));
        self.set_shadow_radius(
            nbt.float_value("shadow_radius")
                .unwrap_or(DEFAULT_SHADOW_RADIUS),
        );
        self.set_shadow_strength(
            nbt.float_value("shadow_strength")
                .unwrap_or(DEFAULT_SHADOW_STRENGTH),
        );
        self.set_width(nbt.float_value("width").unwrap_or(DEFAULT_WIDTH));
        self.set_height(nbt.float_value("height").unwrap_or(DEFAULT_HEIGHT));
        self.set_synced_glow_color_override(
            nbt.int_value("glow_color_override")
                .unwrap_or(DEFAULT_GLOW_COLOR_OVERRIDE),
        );
        self.set_brightness_override(nbt.get("brightness").and_then(Brightness::from_nbt_tag));
    }
}

/// `Display`'s [`EntityProps::tick`]; it never calls `super`.
///
/// The rest of `Display.tick` is client interpolation.
pub fn tick<E: Parts>(display: &E) {
    if display
        .vehicle()
        .is_some_and(|vehicle| vehicle.is_removed())
    {
        display.stop_riding();
    }
}

/// `Display`'s [`EntityProps::piston_push_reaction`].
#[must_use]
pub const fn piston_push_reaction<E: Parts>(_: &E) -> PushReaction {
    PushReaction::IgnoreEntity
}

/// `Display`'s [`EntityProps::is_ignoring_block_triggers`].
#[must_use]
pub const fn is_ignoring_block_triggers<E: Parts>(_: &E) -> bool {
    true
}

/// `Display`'s overrides of the entity properties.
///
/// `Display.hurtServer` never takes damage, which is already [`EntityProps::hurt`]'s
/// default.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        tick: tick::<E>,
        piston_push_reaction: piston_push_reaction::<E>,
        is_ignoring_block_triggers: is_ignoring_block_triggers::<E>,
        ..EntityProps::new()
    }
}

/// Controls how a display entity looks at a player (from their client).
///
/// Each value controls whether a display entity follows the player along
/// the horizontal axis and the vertical axis.
#[repr(i8)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
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
        match tag.string()?.to_str().as_ref() {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brightness {
    /// The block light level.
    pub block: i32,
    /// The skylight level.
    pub sky: i32,
}

impl Brightness {
    const BLOCK_LIGHT_SHIFT: u32 = 4;
    const SKY_LIGHT_SHIFT: u32 = 20;

    /// Pack using `LightCoordsUtil` bit positions
    #[must_use]
    pub const fn pack(self) -> i32 {
        self.block << Self::BLOCK_LIGHT_SHIFT | self.sky << Self::SKY_LIGHT_SHIFT
    }

    /// Unpacks a [`Brightness`] from a single `i32`.
    #[must_use]
    pub const fn unpack(bits: i32) -> Brightness {
        Self {
            block: (bits >> Self::BLOCK_LIGHT_SHIFT) & MAX_LIGHT_LEVEL as i32,
            sky: (bits >> Self::SKY_LIGHT_SHIFT) & MAX_LIGHT_LEVEL as i32,
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
        let block = compound.get("block")?.codec_i32()?;
        let range = 0..=i32::from(MAX_LIGHT_LEVEL);
        if !range.contains(&block) {
            return None;
        }

        let sky = compound.get("sky")?.codec_i32()?;
        if !range.contains(&sky) {
            return None;
        }

        Some(Self { block, sky })
    }
}

#[cfg(test)]
mod tests;
