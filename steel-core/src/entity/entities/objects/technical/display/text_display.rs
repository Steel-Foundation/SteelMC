//! Vanilla's text display implementation.

use crate::entity::damage::DamageSource;
use crate::entity::entities::objects::technical::display::{
    Display, DisplayView, PrivateDisplayView,
};
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::world::World;
use bitflags::bitflags;
use glam::DVec3;
use parking_lot::MutexGuard;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::{DisplayEntityData, TextDisplayEntityData};
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};
use text_components::TextComponent;

/// The default line width of text shown by a text display.
pub const DEFAULT_LINE_WIDTH: i32 = 200;
/// The default text opacity of text shown by a text display.
///
/// `-1` corresponds to full opacity.
pub const DEFAULT_TEXT_OPACITY: i8 = -1;
/// The default background color of text shown by a text display.
pub const DEFAULT_BACKGROUND_COLOR: i32 = 0x4000_0000;

/// The Vanilla text display entity.
///
/// In addition to having the common display entity properties, this entity
/// also stores text-related fields to control what text it renders and
/// how it does so.
///
/// Like any display entity, to **access** or **modify** the data of a text display,
/// you will need to use [`Display::with_view`]. This method takes a function with a
/// [`TextDisplayView`] as a parameter, which can be used within the function.
#[entity_behavior(class = "TextDisplay")]
pub struct TextDisplayEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<TextDisplayEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `TextDisplayEntity`.
unsafe impl DowncastType for TextDisplayEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/text_display");
}

impl TextDisplayEntity {
    /// Creates a new text display entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates a text display entity from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    #[must_use]
    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: SyncMutex::new(TextDisplayEntityData::new()),
        }
    }
}

impl Entity for TextDisplayEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn tick(&self) {
        self.tick_display();
    }
    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.hurt_display(world, source, amount)
    }
    fn piston_push_reaction(&self) -> PushReaction {
        self.piston_push_reaction_display()
    }
    fn is_ignoring_block_triggers(&self) -> bool {
        self.is_ignoring_block_triggers_display()
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.with_view(|view| {
            <Self as Display>::save_display(&view, nbt);

            nbt.insert("text", *view.text());
            nbt.insert("line_width", view.line_width());
            nbt.insert("background", view.background_color());
            nbt.insert("text_opacity", view.text_opacity());

            nbt.insert("shadow", view.flags().contains(TextDisplayFlags::SHADOW));
            nbt.insert(
                "see_through",
                view.flags().contains(TextDisplayFlags::SEE_THROUGH),
            );
            nbt.insert(
                "default_background",
                view.flags()
                    .contains(TextDisplayFlags::USE_DEFAULT_BACKGROUND),
            );

            nbt.insert("alignment", view.alignment());
        });
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.with_view(|mut view| {
            <Self as Display>::load_display(&mut view, nbt);

            view.set_line_width(nbt.int("line_width").unwrap_or(DEFAULT_LINE_WIDTH));
            view.set_text_opacity(nbt.byte("text_opacity").unwrap_or(DEFAULT_TEXT_OPACITY) as u8);
            view.set_background_color(nbt.int("background").unwrap_or(DEFAULT_BACKGROUND_COLOR));

            let mut flags = TextDisplayFlags::empty();
            if view.shadow() {
                flags.insert(TextDisplayFlags::SHADOW);
            }
            if view.see_through() {
                flags.insert(TextDisplayFlags::SEE_THROUGH);
            }
            if view.default_background() {
                flags.insert(TextDisplayFlags::USE_DEFAULT_BACKGROUND);
            }
            view.set_flags(flags);

            let alignment = nbt.get("alignment").and_then(Alignment::from_nbt_tag);
            if let Some(alignment) = alignment {
                view.set_alignment(alignment);
            }

            // TODO: Resolve the text component with this display entity's world.
            let text = nbt.get("text").and_then(TextComponent::from_nbt_tag);
            if let Some(text) = text {
                view.set_text(text);
            }
        });
    }
}

impl Display for TextDisplayEntity {
    type View<'a> = TextDisplayView<'a>;

    fn with_view(&self, f: impl FnOnce(Self::View<'_>)) {
        f(TextDisplayView(self.entity_data.lock()));
    }
}

/// A view to the data of a text display.
///
/// Along with having the methods in [`DisplayView`], this view also has additional methods
/// to access and manipulate the text and the way it is displayed of the text display.
pub struct TextDisplayView<'a>(MutexGuard<'a, TextDisplayEntityData>);

impl<'a> PrivateDisplayView<'a> for TextDisplayView<'a> {
    fn display_data(&self) -> &DisplayEntityData {
        self.0.display()
    }

    fn display_data_mut(&mut self) -> &mut DisplayEntityData {
        self.0.display_mut()
    }
}

impl<'a> DisplayView<'a> for TextDisplayView<'a> {}

impl TextDisplayView<'_> {
    /// Gets a clone of the text component currently displayed by this text display.
    #[must_use]
    pub fn text(&self) -> Box<TextComponent> {
        self.0.text.get().clone()
    }

    /// Gets a reference to the text component currently displayed by this text display.
    #[must_use]
    pub fn text_ref(&self) -> &TextComponent {
        self.0.text.get()
    }

    /// Sets the text displayed by this text display to `text`.
    pub fn set_text(&mut self, text: impl Into<TextComponent>) {
        self.0.text.set(Box::new(text.into()));
    }

    /// Gets the maximum width of a single line on this text display.
    #[must_use]
    pub fn line_width(&self) -> i32 {
        *self.0.line_width.get()
    }

    /// Sets the maximum width of a single line on this text display to `width`.
    pub fn set_line_width(&mut self, width: i32) {
        self.0.line_width.set(width);
    }

    /// Gets the text opacity of this text display.
    ///
    /// Values from `0` to `3`, inclusive, result in **fully opaque** text due to Minecraft's rendering.
    /// Values starting from `4` act as normal: a higher value means a higher opacity, where `255`
    /// represents full opacity.
    ///
    /// **Note:** This property is interpolated.
    #[must_use]
    pub fn text_opacity(&self) -> u8 {
        *self.0.text_opacity.get() as u8
    }

    /// Sets the opacity of this text display to `opacity`.
    ///
    /// Values from `0` to `3`, inclusive, result in **fully opaque** text due to Minecraft's rendering.
    /// Values starting from `4` act as normal: a higher value means a higher opacity, where `255`
    /// represents full opacity.
    ///
    /// **Note:** This property is interpolated.
    pub fn set_text_opacity(&mut self, opacity: u8) {
        self.0.text_opacity.set(opacity as i8);
    }

    /// Gets the background color of this text display.
    ///
    /// **Note:** This property is interpolated.
    #[must_use]
    pub fn background_color(&self) -> i32 {
        *self.0.background_color.get()
    }

    /// Sets the background color of this text display to `color`.
    ///
    /// **Note:** This property is interpolated.
    pub fn set_background_color(&mut self, color: i32) {
        self.0.background_color.set(color);
    }

    /// Gets whether a shadow is present for the text in this text display.
    #[must_use]
    pub fn shadow(&self) -> bool {
        self.flags().contains(TextDisplayFlags::SHADOW)
    }

    /// Sets whether a shadow is present for the text in this text display to `shadow`.
    pub fn set_shadow(&mut self, shadow: bool) {
        let mut flags = self.flags();
        flags.set(TextDisplayFlags::SHADOW, shadow);
        self.set_flags(flags);
    }

    /// Gets whether the text in this text display is see-through.
    #[must_use]
    pub fn see_through(&self) -> bool {
        self.flags().contains(TextDisplayFlags::SEE_THROUGH)
    }

    /// Sets whether the text in this text display is see-through to `state`.
    pub fn set_see_through(&mut self, state: bool) {
        let mut flags = self.flags();
        flags.set(TextDisplayFlags::SEE_THROUGH, state);
        self.set_flags(flags);
    }

    /// Gets whether the color of the text's background in this text display
    /// matches with that of the default text background (the same as that of chat).
    #[must_use]
    pub fn default_background(&self) -> bool {
        self.flags()
            .contains(TextDisplayFlags::USE_DEFAULT_BACKGROUND)
    }

    /// Sets whether the color of the text's background in this text display
    /// matches with that of the default text background (the same as that of chat)
    /// to `state`.
    pub fn set_default_background(&mut self, state: bool) {
        let mut flags = self.flags();
        flags.set(TextDisplayFlags::USE_DEFAULT_BACKGROUND, state);
        self.set_flags(flags);
    }

    /// Gets the [`Alignment`] of this text display.
    #[must_use]
    pub fn alignment(&self) -> Alignment {
        self.flags().into()
    }

    /// Sets the alignment of this text display to `alignment`.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        let mut flags = self.flags();
        flags.set(
            TextDisplayFlags::ALIGN_LEFT,
            matches!(alignment, Alignment::Left),
        );
        flags.set(
            TextDisplayFlags::ALIGN_RIGHT,
            matches!(alignment, Alignment::Right),
        );
        self.set_flags(flags);
    }

    /// Gets the boolean flags of this text display.
    #[must_use]
    fn flags(&self) -> TextDisplayFlags {
        TextDisplayFlags(*self.0.style_flags.get())
    }

    /// Sets the boolean flags of this text display to `flags`.
    fn set_flags(&mut self, flags: TextDisplayFlags) {
        self.0.style_flags.set(flags.0);
    }
}

/// Flags that control some boolean properties of text displays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextDisplayFlags(i8);

bitflags! {
    impl TextDisplayFlags: i8 {
        const SHADOW = 1;
        const SEE_THROUGH = 1 << 1;
        const USE_DEFAULT_BACKGROUND = 1 << 2;
        const ALIGN_LEFT = 1 << 3;
        const ALIGN_RIGHT = 1 << 4;
    }
}

/// The text alignment used by a text display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// Center alignment.
    Center,
    /// Left alignment.
    Left,
    /// Right alignment.
    Right,
}

impl From<TextDisplayFlags> for Alignment {
    fn from(flags: TextDisplayFlags) -> Self {
        if flags.contains(TextDisplayFlags::ALIGN_LEFT) {
            Alignment::Left
        } else if flags.contains(TextDisplayFlags::ALIGN_RIGHT) {
            Alignment::Right
        } else {
            Alignment::Center
        }
    }
}

impl ToNbtTag for Alignment {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::String(
            match self {
                Self::Center => "center",
                Self::Left => "left",
                Self::Right => "right",
            }
            .into(),
        )
    }
}

impl FromNbtTag for Alignment {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        match tag.string()?.to_string().as_str() {
            "center" => Some(Self::Center),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}
