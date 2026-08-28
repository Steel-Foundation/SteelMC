//! Vanilla's text display implementation.

use crate::entity::damage::DamageSource;
use crate::entity::entities::objects::display_ui::Display;
use crate::entity::entities::objects::display_ui::display::PrivateDisplay;
use crate::entity::entities::objects::display_ui::display::Transformation;
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::world::World;
use bitflags::bitflags;
use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::TextDisplayEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};
use text_components::TextComponent;

/// The Vanilla text display entity.
///
/// In addition to having the common display entity properties, this entity
/// also stores text-related fields to control what text it renders and
/// how it does so.
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

    /// Gets a clone of the text currently displayed by this text display.
    pub fn text(&self) -> Box<TextComponent> {
        self.entity_data.lock().text.get().clone()
    }

    /// Sets the text displayed by this text display to `text`.
    pub fn set_text(&self, text: impl Into<TextComponent>) {
        self.entity_data.lock().text.set(Box::new(text.into()));
    }

    /// Gets the maximum width of a single line on this text display.
    pub fn line_width(&self) -> i32 {
        *self.entity_data.lock().line_width.get()
    }

    /// Sets the maximum width of a single line on this text display to `width`.
    pub fn set_line_width(&self, width: i32) {
        self.entity_data.lock().line_width.set(width);
    }

    /// Gets the text opacity of this text display.
    ///
    /// Values from `0` to `3`, inclusive, result in **fully opaque** text due to Minecraft's rendering.
    /// Values starting from `4` act as normal: a higher value means a higher opacity, where `255`
    /// represents full opacity.
    pub fn text_opacity(&self) -> u8 {
        *self.entity_data.lock().text_opacity.get() as u8
    }

    /// Sets the opacity of this text display to `opacity`.
    ///
    /// Values from `0` to `3`, inclusive, result in **fully opaque** text due to Minecraft's rendering.
    /// Values starting from `4` act as normal: a higher value means a higher opacity, where `255`
    /// represents full opacity.
    pub fn set_text_opacity(&self, opacity: u8) {
        self.entity_data.lock().text_opacity.set(opacity as i8);
    }

    /// Gets the background color of this text display.
    pub fn background_color(&self) -> i32 {
        *self.entity_data.lock().background_color.get()
    }

    /// Sets the background color of this text display to `color`.
    pub fn set_background_color(&self, color: i32) {
        self.entity_data.lock().line_width.set(color);
    }

    /// Gets whether a shadow is present for the text in this text display.
    pub fn shadow(&self) -> bool {
        self.flags().contains(TextDisplayFlags::SHADOW)
    }

    /// Sets whether a shadow is present for the text in this text display to `shadow`.
    pub fn set_shadow(&self, shadow: bool) {
        let mut flags = self.flags();
        flags.set(TextDisplayFlags::SHADOW, shadow);
        self.set_flags(flags);
    }

    /// Gets whether the text in this text display is see-through.
    pub fn see_through(&self) -> bool {
        self.flags().contains(TextDisplayFlags::SEE_THROUGH)
    }

    /// Sets whether the text in this text display is see-through to `state`.
    pub fn set_see_through(&self, state: bool) {
        let mut flags = self.flags();
        flags.set(TextDisplayFlags::SEE_THROUGH, state);
        self.set_flags(flags);
    }

    /// Gets whether the color of the text's background in this text display
    /// matches with that of the default text background (the same as that of chat).
    pub fn default_background(&self) -> bool {
        self.flags()
            .contains(TextDisplayFlags::USE_DEFAULT_BACKGROUND)
    }

    /// Sets whether the color of the text's background in this text display
    /// matches with that of the default text background (the same as that of chat)
    /// to `state`.
    pub fn set_default_background(&self, state: bool) {
        let mut flags = self.flags();
        flags.set(TextDisplayFlags::USE_DEFAULT_BACKGROUND, state);
        self.set_flags(flags);
    }

    /// Gets the [`Alignment`] of this text display.
    pub fn alignment(&self) -> Alignment {
        self.flags().into()
    }

    /// Sets the alignment of this text display to `alignment`.
    pub fn set_alignment(&self, alignment: Alignment) {
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
    fn flags(&self) -> TextDisplayFlags {
        TextDisplayFlags(*self.entity_data.lock().style_flags.get())
    }

    /// Sets the boolean flags of this text display to `flags`.
    fn set_flags(&self, flags: TextDisplayFlags) {
        self.entity_data.lock().style_flags.set(flags.0);
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
        self.save_display(nbt);

        nbt.insert("text", *self.text());
        nbt.insert("line_width", self.line_width());
        nbt.insert("background", self.background_color());
        nbt.insert("text_opacity", self.text_opacity());

        nbt.insert("shadow", self.flags().contains(TextDisplayFlags::SHADOW));
        nbt.insert(
            "see_through",
            self.flags().contains(TextDisplayFlags::SEE_THROUGH),
        );
        nbt.insert(
            "default_background",
            self.flags()
                .contains(TextDisplayFlags::USE_DEFAULT_BACKGROUND),
        );

        nbt.insert("alignment", self.text_opacity());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_display(nbt);

        self.set_line_width(nbt.int("line_width").unwrap_or(200));
        self.set_text_opacity(nbt.byte("text_opacity").unwrap_or(-1) as u8);
        self.set_background_color(nbt.int("background").unwrap_or(0x4000_0000));

        let mut flags = TextDisplayFlags::empty();
        if self.shadow() {
            flags.insert(TextDisplayFlags::SHADOW);
        }
        if self.see_through() {
            flags.insert(TextDisplayFlags::SEE_THROUGH);
        }
        if self.default_background() {
            flags.insert(TextDisplayFlags::USE_DEFAULT_BACKGROUND);
        }
        self.set_flags(flags);

        let alignment = nbt.get("alignment").and_then(Alignment::from_nbt_tag);
        if let Some(alignment) = alignment {
            self.set_alignment(alignment);
        }

        // TODO: resolve text component
        let text = nbt.get("text").and_then(TextComponent::from_nbt_tag);
        if let Some(text) = text {
            self.set_text(text);
        }
    }
}

display_impl!(TextDisplayEntity);

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
