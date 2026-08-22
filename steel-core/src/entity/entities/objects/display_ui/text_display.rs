//! Text display entity implementation.
//!
//! Display entities render a block, item, or text without collision.
//! They're commonly used for visual effects, holograms, and decorations.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::TextDisplayEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};
use text_components::TextComponent;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::world::World;
/// A text display entity that renders a floating text component at its position.
///
/// Text displays are purely visual entities with no collision.
/// They support transformation (translation, rotation, scale) and
/// interpolation for smooth animations.
#[entity_behavior(class = "TextDisplay")]
pub struct TextDisplayEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced entity data for network serialization.
    entity_data: SyncMutex<TextDisplayEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `TextDisplayEntity`.
unsafe impl DowncastType for TextDisplayEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/text_display");
}

impl TextDisplayEntity {
    /// Creates a new text display entity.
    ///
    /// The `id` should be obtained from `next_entity_id()`.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(TextDisplayEntityData::new()),
        }
    }

    /// Creates a new Text display entity with a specific UUID.
    ///
    /// The `id` should be obtained from `next_entity_id()`.
    #[must_use]
    pub fn with_uuid(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        uuid: Uuid,
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(TextDisplayEntityData::new()),
        }
    }

    /// Creates a text display entity from saved data.
    ///
    /// Display entities have no physical collision, but vanilla base state is
    /// still persisted and should round-trip through the shared base.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(TextDisplayEntityData::new()),
        }
    }

    /// Gets a reference to the entity data for reading/modifying synced state.
    pub const fn entity_data(&self) -> &SyncMutex<TextDisplayEntityData> {
        &self.entity_data
    }

    /// Sets the displayed text component.
    pub fn set_text(&self, text: TextComponent) {
        self.entity_data.lock().text.set(Box::new(text));
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

    fn is_ignoring_block_triggers(&self) -> bool {
        true
    }
    // TODO: Only `text` is persisted for now. text_opacity and the shared
    // Display-layer fields (translation, scale, rotation, billboard,
    // brightness_override, view_range, shadow_radius/strength, width,
    // height, glow_color_override, interpolation timing) are network-synced
    // correctly but not saved to NBT — matches BlockDisplayEntity's current
    // scope, not unique to this entity.
    fn save_additional(&self, nbt: &mut NbtCompound) {
        let text = self.entity_data.lock().text.get().clone();
        nbt.insert("text", text.to_codec_nbt());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(tag) = nbt.get("text")
            && let Some(text) = TextComponent::from_nbt(&tag.to_owned())
        {
            self.entity_data.lock().text.set(Box::new(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::ToString;
    use steel_registry::vanilla_entities;
    use text_components::TextComponent;

    #[test]
    fn text_display_persists_text_content() {
        let display = TextDisplayEntity::new(
            &vanilla_entities::TEXT_DISPLAY,
            1,
            DVec3::new(0.0, 70.0, 0.0),
            Weak::new(),
        );
        display.set_text(TextComponent::plain("Hello Steel".to_string()));

        let mut nbt = NbtCompound::new();
        display.save_additional(&mut nbt);

        assert_eq!(
            nbt.string("text").map(ToString::to_string),
            Some("Hello Steel".to_owned())
        );
    }
}
