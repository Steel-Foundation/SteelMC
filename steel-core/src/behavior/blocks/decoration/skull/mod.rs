mod abstract_skull_block;
mod piglin_wall_skull_block;
mod player_head_block;
mod player_wall_head_block;
mod skull_block;
mod wall_skull_block;
mod wither_skull_block;
mod wither_wall_skull_block;

use std::io::Result;

pub use piglin_wall_skull_block::PiglinWallSkullBlock;
pub use player_head_block::PlayerHeadBlock;
pub use player_wall_head_block::PlayerWallHeadBlock;
pub use skull_block::SkullBlock;
use steel_registry::PlayerSkinPatch;
use steel_registry::data_components::DataComponentPatch;
use steel_registry::data_components::vanilla_components::{CUSTOM_NAME, NOTE_BLOCK_SOUND, PROFILE};
use steel_registry::{REGISTRY, ResolvableProfile, blocks::BlockRef, item_stack::ItemStack};
use steel_utils::Identifier;
use text_components::TextComponent;
pub use wall_skull_block::WallSkullBlock;
pub use wither_skull_block::WitherSkullBlock;
pub use wither_wall_skull_block::WitherWallSkullBlock;

use crate::behavior::skull_types;

/// Static reference to a skull type
pub type SkullTypeRef = &'static SkullType;

/// A skull kind and its floor and wall blocks
#[derive(Debug)]
pub struct SkullType {
    /// Key of this skull type
    pub key: Identifier,
    /// Block used when the skull is placed on the floor
    pub block: BlockRef,
    /// Block used when the skull is placed on a wall
    pub wall_block: BlockRef,
}

/// Data stored by a skull
#[derive(Clone, Default)]
pub struct SkullData {
    /// Player whose skin the skull shows
    pub owner: Option<ResolvableProfile>,
    /// Sound a note block plays below this skull
    pub note_block_sound: Option<Identifier>,
    /// Custom name for skull items
    pub custom_name: Option<TextComponent>,
}

impl SkullData {
    /// Creates a component patch with all data of this skull
    #[must_use]
    pub fn component_patch(&self) -> DataComponentPatch {
        let mut patch = self.custom_name_patch();
        if let Some(owner) = self.owner.clone() {
            patch.set(PROFILE, owner);
        }
        if let Some(sound) = self.note_block_sound.clone() {
            patch.set(NOTE_BLOCK_SOUND, sound);
        }
        patch
    }

    /// Creates a component patch with only the custom name
    #[must_use]
    pub fn custom_name_patch(&self) -> DataComponentPatch {
        let mut patch = DataComponentPatch::new();
        if let Some(name) = self.custom_name.clone() {
            patch.set(CUSTOM_NAME, name);
        }
        patch
    }
}

/// Builder for skull items
pub struct Skull {
    skull_type: SkullTypeRef,
    data: SkullData,
}

impl Skull {
    /// Creates a player head
    /// TODO: make player actually impl Into<ResolvableProfile>
    pub fn of(player: impl Into<ResolvableProfile>) -> Self {
        Self {
            skull_type: &skull_types::PLAYER,
            data: SkullData {
                owner: Some(player.into()),
                ..SkullData::default()
            },
        }
    }

    /// Constructs
    /// TODO: remove this
    pub fn from_player_name(name: String) -> Result<Self> {
        Ok(Self {
            skull_type: &skull_types::PLAYER,
            data: SkullData {
                owner: Some(ResolvableProfile::dynamic_name(
                    name,
                    PlayerSkinPatch::default(),
                )?),
                note_block_sound: None,
                custom_name: None,
            },
        })
    }

    /// Sets the custom name
    #[must_use]
    pub fn custom_name(mut self, custom_name: impl Into<TextComponent>) -> Self {
        self.data.custom_name = Some(custom_name.into());
        self
    }

    /// Sets the custom note block sound
    #[must_use]
    pub fn note_block_sound(mut self, note_block_sound: Identifier) -> Self {
        self.data.note_block_sound = Some(note_block_sound);
        self
    }

    /// Builds the skull as an item stack
    #[must_use]
    pub fn to_item(&self) -> ItemStack {
        let item = REGISTRY.items.by_block(self.skull_type.block);
        ItemStack::with_count_and_patch(item, 1, self.data.component_patch())
    }
}
