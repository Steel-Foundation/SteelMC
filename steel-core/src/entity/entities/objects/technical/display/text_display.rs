//! Vanilla `Display.TextDisplay`: renders a text component.

use bitflags::bitflags;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::TextDisplayEntityData;
use steel_utils::locks::CheckedMutex;
use steel_utils::nbt::NbtValueInput as _;
use text_components::TextComponent;

use super::Parts as _;
use crate::entity::{Entity, EntityBase, EntityProps, EntityTable};

/// The default line width of text shown by a text display.
pub const DEFAULT_LINE_WIDTH: i32 = 200;

/// The default text opacity of text shown by a text display.
///
/// `-1` corresponds to full opacity.
pub const DEFAULT_TEXT_OPACITY: i8 = -1;

/// The default background color of text shown by a text display.
pub const DEFAULT_BACKGROUND_COLOR: i32 = 0x4000_0000;

/// A display entity that renders text with configurable text properties.
///
/// The state shared by all displays is reached through [`Parts::display`](super::Parts::display).
#[entity_behavior(class = "TextDisplay")]
#[derive(Entity)]
#[entity(save = Self::save_text_display, load = Self::load_text_display)]
#[entity(key = "steel:entity/text_display", constructors)]
pub struct TextDisplayEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<TextDisplayEntityData>,
}

impl EntityTable for TextDisplayEntity {
    const ENTITY: EntityProps<Self> = super::entity_props();
}

impl super::Parts for TextDisplayEntity {}

impl TextDisplayEntity {
    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        base.set_no_physics(true);
        Self {
            base,
            entity_type,
            entity_data: CheckedMutex::new(TextDisplayEntityData::new()),
        }
    }

    /// Gets a copy of the text component currently displayed by this text display.
    #[must_use]
    pub fn text(&self) -> TextComponent {
        TextComponent::clone(self.entity_data.lock().text.get())
    }

    /// Sets the text displayed by this text display to `text`.
    pub fn set_text(&self, text: impl Into<TextComponent>) {
        self.entity_data.lock().text.set(Box::new(text.into()));
    }

    /// Gets the maximum width of a single line on this text display.
    #[must_use]
    pub fn line_width(&self) -> i32 {
        *self.entity_data.lock().line_width.get()
    }

    /// Sets the maximum width of a single line on this text display to `width`.
    pub fn set_line_width(&self, width: i32) {
        self.entity_data.lock().line_width.set(width);
    }

    /// Gets the text opacity of this text display.
    ///
    /// The client uses this value as the glyph alpha, with `255` representing full opacity.
    ///
    /// **Note:** This property is interpolated.
    #[must_use]
    pub fn text_opacity(&self) -> u8 {
        *self.entity_data.lock().text_opacity.get() as u8
    }

    /// Sets the opacity of this text display to `opacity`.
    ///
    /// The client uses this value as the glyph alpha, with `255` representing full opacity.
    ///
    /// **Note:** This property is interpolated.
    pub fn set_text_opacity(&self, opacity: u8) {
        self.entity_data.lock().text_opacity.set(opacity as i8);
    }

    /// Gets the background color of this text display.
    ///
    /// **Note:** This property is interpolated.
    #[must_use]
    pub fn background_color(&self) -> i32 {
        *self.entity_data.lock().background_color.get()
    }

    /// Sets the background color of this text display to `color`.
    ///
    /// **Note:** This property is interpolated.
    pub fn set_background_color(&self, color: i32) {
        self.entity_data.lock().background_color.set(color);
    }

    /// Gets whether a shadow is present for the text in this text display.
    #[must_use]
    pub fn shadow(&self) -> bool {
        self.flags().contains(TextDisplayFlags::SHADOW)
    }

    /// Sets whether a shadow is present for the text in this text display to `shadow`.
    pub fn set_shadow(&self, shadow: bool) {
        self.update_flags(|flags| flags.set(TextDisplayFlags::SHADOW, shadow));
    }

    /// Gets whether the text in this text display is see-through.
    #[must_use]
    pub fn see_through(&self) -> bool {
        self.flags().contains(TextDisplayFlags::SEE_THROUGH)
    }

    /// Sets whether the text in this text display is see-through to `state`.
    pub fn set_see_through(&self, state: bool) {
        self.update_flags(|flags| flags.set(TextDisplayFlags::SEE_THROUGH, state));
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
    pub fn set_default_background(&self, state: bool) {
        self.update_flags(|flags| flags.set(TextDisplayFlags::USE_DEFAULT_BACKGROUND, state));
    }

    /// Gets the [`Alignment`] of this text display.
    #[must_use]
    pub fn alignment(&self) -> Alignment {
        self.flags().into()
    }

    /// Sets the alignment of this text display to `alignment`.
    pub fn set_alignment(&self, alignment: Alignment) {
        self.update_flags(|flags| {
            flags.set(
                TextDisplayFlags::ALIGN_LEFT,
                matches!(alignment, Alignment::Left),
            );
            flags.set(
                TextDisplayFlags::ALIGN_RIGHT,
                matches!(alignment, Alignment::Right),
            );
        });
    }

    fn flags(&self) -> TextDisplayFlags {
        TextDisplayFlags(*self.entity_data.lock().style_flags.get())
    }

    fn set_flags(&self, flags: TextDisplayFlags) {
        self.entity_data.lock().style_flags.set(flags.0);
    }

    /// Edits the flags under one lock, so concurrent edits cannot lose a flag.
    fn update_flags(&self, edit: impl FnOnce(&mut TextDisplayFlags)) {
        let mut data = self.entity_data.lock();
        let mut flags = TextDisplayFlags(*data.style_flags.get());
        edit(&mut flags);
        data.style_flags.set(flags.0);
    }

    fn save_text_display(&self, nbt: &mut NbtCompound) {
        self.display().save(nbt);

        {
            let data = self.entity_data.lock();
            let text: &TextComponent = data.text.get();
            nbt.insert("text", text);
        }
        nbt.insert("line_width", self.line_width());
        nbt.insert("background", self.background_color());
        nbt.insert("text_opacity", self.text_opacity());

        nbt.insert("shadow", self.shadow());
        nbt.insert("see_through", self.see_through());
        nbt.insert("default_background", self.default_background());

        nbt.insert("alignment", self.alignment());
    }

    fn load_text_display(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.display().load(nbt);

        self.set_line_width(nbt.int_value("line_width").unwrap_or(DEFAULT_LINE_WIDTH));
        self.set_text_opacity(
            nbt.byte_value("text_opacity")
                .unwrap_or(DEFAULT_TEXT_OPACITY) as u8,
        );
        self.set_background_color(
            nbt.int_value("background")
                .unwrap_or(DEFAULT_BACKGROUND_COLOR),
        );

        let mut flags = TextDisplayFlags::empty();
        if nbt.byte_value("shadow").is_some_and(|v| v != 0) {
            flags.insert(TextDisplayFlags::SHADOW);
        }
        if nbt.byte_value("see_through").is_some_and(|v| v != 0) {
            flags.insert(TextDisplayFlags::SEE_THROUGH);
        }
        if nbt.byte_value("default_background").is_some_and(|v| v != 0) {
            flags.insert(TextDisplayFlags::USE_DEFAULT_BACKGROUND);
        }
        self.set_flags(flags);

        let alignment = nbt.get("alignment").and_then(Alignment::from_nbt_tag);
        if let Some(alignment) = alignment {
            self.set_alignment(alignment);
        }

        // TODO: Resolve the text component with this display entity's world.
        let text = nbt.get("text").and_then(TextComponent::from_nbt_tag);
        if let Some(text) = text {
            self.set_text(text);
        }
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
        match tag.string()?.to_str().as_ref() {
            "center" => Some(Self::Center),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;
    use crate::test_support::load_additional;

    fn text_display(id: i32) -> Arc<TextDisplayEntity> {
        TextDisplayEntity::new(
            &vanilla_entities::TEXT_DISPLAY,
            id,
            DVec3::ZERO,
            Weak::new(),
        )
    }

    #[test]
    fn text_display_round_trips_its_text_properties() {
        init_vanilla_registry();
        let display = text_display(1);
        display.set_text(TextComponent::plain("steel"));
        display.set_line_width(80);
        display.set_text_opacity(128);
        display.set_background_color(0x1234_5678);
        display.set_shadow(true);
        display.set_default_background(true);
        display.set_alignment(Alignment::Right);

        let mut nbt = NbtCompound::new();
        display.save_additional(&mut nbt);
        let loaded = text_display(2);
        load_additional(&*loaded, &nbt);

        assert_eq!(loaded.text(), TextComponent::plain("steel"));
        assert_eq!(loaded.line_width(), 80);
        assert_eq!(loaded.text_opacity(), 128);
        assert_eq!(loaded.background_color(), 0x1234_5678);
        assert!(loaded.shadow());
        assert!(!loaded.see_through());
        assert!(loaded.default_background());
        assert_eq!(loaded.alignment(), Alignment::Right);
    }

    #[test]
    fn changing_alignment_keeps_the_other_flags() {
        init_vanilla_registry();
        let display = text_display(1);
        display.set_see_through(true);
        display.set_alignment(Alignment::Left);
        display.set_alignment(Alignment::Right);

        assert_eq!(display.alignment(), Alignment::Right);
        assert!(display.see_through());

        display.set_alignment(Alignment::Center);
        assert_eq!(display.alignment(), Alignment::Center);
        assert!(display.see_through());
    }

    #[test]
    fn loading_without_alignment_resets_it_to_center() {
        init_vanilla_registry();
        let display = text_display(1);
        display.set_alignment(Alignment::Left);

        load_additional(&*display, &NbtCompound::new());

        assert_eq!(display.alignment(), Alignment::Center);
        assert_eq!(display.line_width(), DEFAULT_LINE_WIDTH);
        assert_eq!(display.text_opacity(), u8::MAX);
        assert_eq!(display.background_color(), DEFAULT_BACKGROUND_COLOR);
    }
}
