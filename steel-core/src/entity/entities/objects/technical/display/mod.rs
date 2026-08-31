//! Vanilla's abstract `Display` implementation.
//!
use crate::entity::{Entity, EntityBase};
use crate::entity::damage::DamageSource;
pub use crate::entity::entities::objects::technical::display::transformation::Transformation;
use crate::world::World;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_data::Matrix4f;
use steel_registry::vanilla_entity_data::DisplayEntityData;

/// The default interpolation duration of a display entity.
pub const DEFAULT_TRANSFORMATION_INTERPOLATION_DURATION: i32 = 0;
/// The default delay in interpolation of a display entity.
pub const DEFAULT_TRANSFORMATION_INTERPOLATION_DELAY: i32 = 0;
/// The default teleport duration of a display entity.
pub const DEFAULT_POS_ROT_INTERPOLATION_DURATION: i32 = 0;
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
/// The default glow color override of a display entity.
///
/// `-1` corresponds to no override.
pub const DEFAULT_GLOW_COLOR_OVERRIDE: i32 = -1;

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
///
/// To **access** or **modify** the data of a display entity, you will need to use [`Display::with_view`].
/// This method takes a function with a [`DisplayView`] as a parameter, which can be used within the function.
pub trait Display: Entity {
    /// The type of [`DisplayView`] implementation associated with this entity.
    type View<'a>;

    /// The base `tick()` method for display entities.
    fn tick_display(&self) {
        if self.vehicle().is_some_and(|v| v.is_removed()) {
            self.stop_riding()
        }
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

    /// Provides a view to the synced data of this entity, accessible via the function `f`.
    ///
    /// This allows accessing and modifying any required data.
    ///
    /// **Warning:** Because this function locks the synced entity data associated with this display entity,
    /// if this method is called again in `f`, it will cause a deadlock to occur.
    fn with_view(&self, f: impl FnOnce(Self::View<'_>));

    /// Loads a display entity's fields common to all display entities from an NBT compound via a view.
    fn load_display<'a: 'b, 'b>(
        view: &'b mut impl DisplayView<'a>,
        nbt: BorrowedNbtCompoundView<'_, '_>,
    ) {
        view.set_transformation(
            nbt.get("transformation")
                .and_then(Transformation::from_nbt_tag)
                .unwrap_or(Transformation::IDENTITY),
        );

        view.set_transformation_interpolation_duration(
            nbt.int("interpolation_duration")
                .unwrap_or(DEFAULT_TRANSFORMATION_INTERPOLATION_DURATION),
        );
        view.set_transformation_interpolation_delay(
            nbt.int("start_interpolation")
                .unwrap_or(DEFAULT_TRANSFORMATION_INTERPOLATION_DELAY),
        );
        view.set_pos_rot_interpolation_duration(
            nbt.int("teleport_duration")
                .unwrap_or(DEFAULT_POS_ROT_INTERPOLATION_DURATION)
                .clamp(0, 59),
        );
        view.set_billboard_constraints(
            nbt.get("billboard")
                .and_then(BillboardConstraints::from_nbt_tag)
                .unwrap_or(DEFAULT_BILLBOARD_CONSTRAINTS),
        );
        view.set_view_range(nbt.float("view_range").unwrap_or(DEFAULT_VIEW_RANGE));
        view.set_shadow_radius(nbt.float("shadow_radius").unwrap_or(DEFAULT_SHADOW_RADIUS));
        view.set_shadow_strength(
            nbt.float("shadow_strength")
                .unwrap_or(DEFAULT_SHADOW_STRENGTH),
        );
        view.set_width(nbt.float("width").unwrap_or(DEFAULT_WIDTH));
        view.set_height(nbt.float("height").unwrap_or(DEFAULT_HEIGHT));
        view.set_synced_glow_color_override(
            nbt.int("glow_color_override")
                .unwrap_or(DEFAULT_GLOW_COLOR_OVERRIDE),
        );
        view.set_brightness_override(nbt.get("brightness").and_then(Brightness::from_nbt_tag));
    }

    /// Saves a display entity's fields common to all display entities to an NBT compound via a view.
    fn save_display<'a>(view: &'a impl DisplayView<'a>, nbt: &mut NbtCompound) {
        nbt.insert("transformation", view.transformation());

        nbt.insert("billboard", view.billboard_constraints());
        nbt.insert(
            "interpolation_duration",
            view.transformation_interpolation_duration(),
        );
        nbt.insert("teleport_duration", view.pos_rot_interpolation_duration());
        nbt.insert("view_range", view.view_range());
        nbt.insert("shadow_radius", view.shadow_radius());
        nbt.insert("shadow_strength", view.shadow_strength());
        nbt.insert("width", view.width());
        nbt.insert("height", view.height());
        nbt.insert(
            "glow_color_override",
            view.glow_color_override().unwrap_or(-1),
        );
        if let Some(brightness) = view.brightness_override() {
            nbt.insert("brightness", brightness);
        }
    }
}

/// A private trait, only used by display entities, to get and set
/// some synced entity data.
trait PrivateDisplayView<'a> {
    fn display_data(&self) -> &DisplayEntityData;
    fn display_data_mut(&mut self) -> &mut DisplayEntityData;

    fn set_synced_brightness_override(&mut self, brightness: i32) {
        self.display_data_mut().brightness_override.set(brightness);
    }

    fn set_synced_glow_color_override(&mut self, value: i32) {
        self.display_data_mut().glow_color_override.set(value);
    }
}

/// A view to the synced data of a [`Display`].
///
/// This trait has common methods, for all displays, to access and modify
/// data specific to display entities. Use [`Display::with_view`] to access a view
/// for an entity.
#[expect(
    private_bounds,
    reason = "outside crates and plugins should not work with raw synced values"
)]
pub trait DisplayView<'a>: PrivateDisplayView<'a> {
    /// Gets the [`Transformation`] of the display entity.
    ///
    /// **Note:** This property is interpolated.
    fn transformation(&self) -> Transformation {
        let data = self.display_data().display();
        Transformation {
            translation: *data.translation.get(),
            left_rotation: *data.left_rotation.get(),
            scale: *data.scale.get(),
            right_rotation: *data.right_rotation.get(),
        }
    }
    /// Sets the [`Transformation`] of the display entity to `transformation`.
    ///
    /// **Note:** This property is interpolated.
    fn set_transformation(&mut self, transformation: Transformation) {
        let data = self.display_data_mut();
        data.translation.set(transformation.translation);
        data.left_rotation.set(transformation.left_rotation);
        data.scale.set(transformation.scale);
        data.right_rotation.set(transformation.right_rotation);
    }

    /// Sets the transformation matrix of the display entity to `mat`.
    ///
    /// **Note:** Since `Mat4` implements `Into<Matrix4f>`, it can also act as
    /// a valid matrix for this function.
    fn set_transformation_matrix(&mut self, mat: impl Into<Matrix4f>) {
        self.set_transformation(Transformation::decompose(mat.into()));
    }

    /// Gets the display entity's *interpolation duration* (the time to interpolate to a new transformation), in ticks.
    fn transformation_interpolation_duration(&self) -> i32 {
        *self
            .display_data()
            .transformation_interpolation_duration
            .get()
    }
    /// Sets the display entity's *interpolation duration* (the time to interpolate to a new transformation), in ticks, to `duration`.
    fn set_transformation_interpolation_duration(&mut self, duration: i32) {
        self.display_data_mut()
            .transformation_interpolation_duration
            .set(duration);
    }
    /// Gets the display entity's *teleport duration* (the time to interpolate to a new position due to a teleport), in ticks.
    ///
    /// Values are clamped to be between `0` and `59` ticks, inclusive.
    ///
    /// **Note:** This property is not saved to disk.
    fn transformation_interpolation_delay(&self) -> i32 {
        *self
            .display_data()
            .transformation_interpolation_start_delta_ticks
            .get()
    }
    /// Sets the display entity's *start interpolation delay* (the delay in starting an interpolation), in ticks, to `duration`,
    /// and restarts the transformation animation (regardless of what `duration` is).
    ///
    /// If this is set to `0`, interpolation starts immediately.
    ///
    /// **Note:** This property is not saved to disk.
    fn set_transformation_interpolation_delay(&mut self, duration: i32) {
        self.display_data_mut()
            .transformation_interpolation_start_delta_ticks
            .set_and_force_dirty(duration, true);
    }
    /// Gets the display entity's *start interpolation delay* (the delay in starting an interpolation), in ticks.
    fn pos_rot_interpolation_duration(&self) -> i32 {
        *self.display_data().pos_rot_interpolation_duration.get()
    }
    /// Sets the display entity's *teleport duration* (the time to interpolate to a new position due to a teleport), in ticks, to `duration`.
    fn set_pos_rot_interpolation_duration(&mut self, duration: i32) {
        self.display_data_mut()
            .pos_rot_interpolation_duration
            .set(duration);
    }

    /// Gets the billboard constraints of the display entity.
    fn billboard_constraints(&self) -> BillboardConstraints {
        BillboardConstraints::try_from(*self.display_data().billboard_render_constraints.get())
            .unwrap_or(DEFAULT_BILLBOARD_CONSTRAINTS)
    }
    /// Sets the display entity's billboard constraints to `constraints`.
    fn set_billboard_constraints(&mut self, constraints: BillboardConstraints) {
        self.display_data_mut()
            .billboard_render_constraints
            .set(constraints as i8);
    }

    /// Gets the display entity's billboard constraints.
    fn brightness_override(&self) -> Option<Brightness> {
        let synced = *self.display_data().brightness_override.get();
        (synced != -1).then(|| Brightness::unpack(synced))
    }
    /// Sets the display entity's brightness override to `brightness`.
    fn set_brightness_override(&mut self, brightness: Option<Brightness>) {
        self.set_synced_brightness_override(brightness.map_or(-1, Brightness::pack));
    }

    /// Gets the display entity's maximum view range.
    fn view_range(&self) -> f32 {
        *self.display_data().view_range.get()
    }
    /// Sets the display entity's maximum view range to `range`.
    fn set_view_range(&mut self, range: f32) {
        self.display_data_mut().view_range.set(range);
    }
    /// Gets the display entity's shadow radius.
    ///
    /// **Note:** This property is interpolated.
    fn shadow_radius(&self) -> f32 {
        *self.display_data().shadow_radius.get()
    }
    /// Sets the display entity's shadow radius to `size`.
    ///
    /// **Note:** This property is interpolated.
    fn set_shadow_radius(&mut self, size: f32) {
        self.display_data_mut().shadow_radius.set(size);
    }
    /// Sets the display entity's shadow strength (which affects the opacity of the display entity's shadow depending on its distance to the block below).
    ///
    /// **Note:** This property is interpolated.
    fn shadow_strength(&self) -> f32 {
        *self.display_data().shadow_strength.get()
    }
    /// Sets the display entity's shadow strength (which affects the opacity of the display entity's shadow depending on its distance to the block below) to `strength`.
    ///
    /// **Note:** This property is interpolated.
    fn set_shadow_strength(&mut self, strength: f32) {
        self.display_data_mut().shadow_strength.set(strength);
    }
    /// Gets the display entity's maximum width.
    fn width(&self) -> f32 {
        *self.display_data().width.get()
    }
    /// Sets the display entity's maximum width to `width`.
    ///
    /// Setting this to `0` indicates no culling on the horizontal axis.
    fn set_width(&mut self, width: f32) {
        self.display_data_mut().width.set(width);
    }
    /// Gets the display entity's maximum height.
    fn height(&self) -> f32 {
        *self.display_data().height.get()
    }
    /// Sets the display entity's maximum height to `height`.
    ///
    /// Setting this to `0` indicates no culling on the vertical axis.
    fn set_height(&mut self, height: f32) {
        self.display_data_mut().height.set(height);
    }
    /// Gets the display entity's glow color override. If this is `None`, the entity glows according to its team's color.
    ///
    /// **Note:** This has no effect on *text displays*.
    fn glow_color_override(&self) -> Option<i32> {
        let color = *self.display_data().glow_color_override.get();
        (color != -1).then_some(color)
    }
    /// Sets the display entity's glow color override to `value`. If this is `None`, the entity glows according to its team's color.
    ///
    /// **Note:** This has no effect on *text displays*.
    fn set_glow_color_override(&mut self, value: Option<i32>) {
        self.set_synced_glow_color_override(value.unwrap_or(-1));
    }
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

fn modify_display_entity_base(base: EntityBase) -> EntityBase {
    base.set_no_physics(true);
    base
}

pub mod block_display;
pub mod item_display;
pub mod text_display;
pub mod transformation;
