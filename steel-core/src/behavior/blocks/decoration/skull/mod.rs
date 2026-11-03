mod abstract_skull_block;
mod piglin_wall_skull_block;
mod player_head_block;
mod player_wall_head_block;
mod skull_block;
mod wall_skull_block;
mod wither_skull_block;
mod wither_wall_skull_block;

use std::io;
use std::sync::Arc;

pub use piglin_wall_skull_block::PiglinWallSkullBlock;
pub use player_head_block::PlayerHeadBlock;
pub use player_wall_head_block::PlayerWallHeadBlock;
pub use skull_block::SkullBlock;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::data_components::DataComponentPatch;
use steel_registry::data_components::vanilla_components::{CUSTOM_NAME, NOTE_BLOCK_SOUND, PROFILE};
use steel_registry::resolvable_profile::PartialProfile;
use steel_registry::{PlayerSkinPatch, ProfileProperty};
use steel_registry::{REGISTRY, ResolvableProfile, blocks::BlockRef, item_stack::ItemStack};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction, Downcast, Identifier};
use text_components::TextComponent;
pub use wall_skull_block::WallSkullBlock;
pub use wither_skull_block::WitherSkullBlock;
pub use wither_wall_skull_block::WitherWallSkullBlock;

use crate::behavior::skull_types;
use crate::block_entity::entities::SkullBlockEntity;
use crate::world::World;

/// Static reference to a skull type
pub type SkullTypeRef = &'static SkullType;

steel_registry::impl_registry_entry_eq!(SkullType);

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

impl SkullType {
    /// Returns the vanilla skull type whose floor or wall block is `block`
    #[must_use]
    pub fn from_block(block: BlockRef) -> Option<SkullTypeRef> {
        skull_types::ALL
            .iter()
            .copied()
            .find(|skull_type| skull_type.block == block || skull_type.wall_block == block)
    }
}

/// Data stored by a skull
#[derive(Clone, Default, PartialEq, Eq, Debug)]
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

    /// Reads the `SkullData` from the `ItemStack`'s components
    #[must_use]
    pub fn from_item(item: &ItemStack) -> Self {
        Self {
            owner: item.get(PROFILE).cloned(),
            note_block_sound: item.get(NOTE_BLOCK_SOUND).cloned(),
            custom_name: item.get(CUSTOM_NAME).cloned(),
        }
    }
}

/// How a skull is attached when placed as a block
#[derive(Debug, Clone, Copy)]
pub enum SkullPlacement {
    /// Skull placed on the floor (values above 15 wrap)
    ///
    /// See [`steel_math::convert_to_rotation_segment`]
    Floor {
        /// The rotation in 16 segments
        rotation: u8,
    },
    /// Skull placed on a wall
    Wall {
        /// Cardinal direction
        facing: Direction,
    },
}

/// Builder for skull items
#[derive(Debug, PartialEq, Eq)]
pub struct Skull {
    skull_type: SkullTypeRef,
    data: SkullData,
}

impl Skull {
    /// Creates a new `Skull` from the given `SkullTypeRef`
    ///
    /// See [`Skull::of`] for creating a player's skull from their gameprofile
    /// See [`Skull::player_named`] for creating a player's skull from their name
    /// See [`Skull::texture`] for creating a player skull from a texture base64 string
    #[must_use]
    pub fn new(skull_type: SkullTypeRef) -> Self {
        Self {
            skull_type,
            data: SkullData::default(),
        }
    }

    /// Creates a player head from the profile of a player
    /// ```ignore
    /// # use steel_core::behavior::blocks::Skull;
    /// let skull = Skull::of(player.gameprofile.clone());
    /// ```
    #[must_use]
    pub fn of(player: impl Into<ResolvableProfile>) -> Self {
        Self {
            skull_type: &skull_types::PLAYER,
            data: SkullData {
                owner: Some(player.into()),
                ..SkullData::default()
            },
        }
    }

    /// Creates a player head that the client resolves by player name
    /// ```
    /// # use steel_core::behavior::blocks::Skull;
    /// let skull = Skull::player_named("Notch");
    /// ```
    pub fn player_named(name: impl Into<String>) -> io::Result<Self> {
        Ok(Self {
            skull_type: &skull_types::PLAYER,
            data: SkullData {
                owner: Some(ResolvableProfile::dynamic_name(
                    name.into(),
                    PlayerSkinPatch::default(),
                )?),
                ..SkullData::default()
            },
        })
    }

    /// Creates a player head showing a texture from a base64 encoded string
    /// ```
    /// # use steel_core::behavior::blocks::Skull;
    /// const TEXTURE: &str = "eyJ0ZXh0dXJlcyI6eyJTS0lOIjp7InVybCI6Imh0dHA6Ly90ZXh0dXJlcy5taW5lY3JhZnQubmV0L3RleHR1cmUvN2M2OWQ0MTA3NmE4ZGVhNGYwNmQzZjFhOWFjNDdjYzk5Njk4OGI3NGEwOTEzYWIyYWMxYTc0Y2FmNzA4MTkxOCJ9fX0=";
    /// let skull = Skull::texture(TEXTURE);
    /// ```
    pub fn texture(value: impl Into<String>) -> io::Result<Self> {
        let property = ProfileProperty::new("textures".to_owned(), value.into(), None)?;
        let profile = PartialProfile::new(None, None, vec![property])?;
        Ok(Self {
            skull_type: &skull_types::PLAYER,
            data: SkullData {
                owner: Some(ResolvableProfile::static_partial(
                    profile,
                    PlayerSkinPatch::default(),
                )),
                ..SkullData::default()
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

    /// Reads the skull data from an `ItemStack` and returns the skull
    #[must_use]
    pub fn from_item(item: &ItemStack) -> Option<Self> {
        let skull_type = SkullType::from_block(item.item().block?)?;
        Some(Self {
            skull_type,
            data: SkullData::from_item(item),
        })
    }

    /// Reads the skull placed at `pos`
    #[must_use]
    pub fn from_world(world: &World, pos: BlockPos) -> Option<Self> {
        let skull_type = SkullType::from_block(world.get_block_state(pos).get_block())?;
        let block_entity = world.get_block_entity(pos)?;
        let skull = block_entity.downcast_ref::<SkullBlockEntity>()?;
        Some(Self {
            skull_type,
            data: skull.data(),
        })
    }

    /// Returns the block state of this skull for the given placement
    pub fn block_state(
        &self,
        placement: SkullPlacement,
    ) -> Result<BlockStateId, InvalidPlacementError> {
        match placement {
            SkullPlacement::Floor { rotation } => Ok(self
                .skull_type
                .block
                .default_state()
                .set_value(&BlockStateProperties::ROTATION_16, rotation & 15)),
            SkullPlacement::Wall { facing } if facing.is_horizontal() => Ok(self
                .skull_type
                .wall_block
                .default_state()
                .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)),
            SkullPlacement::Wall { facing } => Err(InvalidPlacementError::InvalidFacing(facing)),
        }
    }

    /// Places this skull as a block and block entity
    ///
    /// > must run in Steel’s serialized world-mutation phase
    ///
    /// See [`World::set_block`] for the warning
    pub fn place(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        placement: SkullPlacement,
        update_flags: UpdateFlags,
    ) -> Result<(), InvalidPlacementError> {
        world.set_block(pos, self.block_state(placement)?, update_flags);
        let Some(block_entity) = world.get_block_entity(pos) else {
            return Ok(());
        };
        let Some(skull) = block_entity.downcast_ref::<SkullBlockEntity>() else {
            return Ok(());
        };
        skull.set_data(self.data.clone());
        Ok(())
    }
}

/// Error during Skull placement
#[derive(Debug, thiserror::Error)]
pub enum InvalidPlacementError {
    /// Wall skulls can only face horizontally
    #[error("wall skulls cannot face: {0:?}")]
    InvalidFacing(Direction),
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;
    use steel_utils::{BlockPos, ChunkPos, types::UpdateFlags};

    use crate::{
        behavior::{
            blocks::{Skull, SkullPlacement},
            init_behaviors,
        },
        block_entity::init_block_entities,
        test_support::{fresh_test_world, insert_ready_full_chunk},
    };

    #[test]
    fn roundtrip() {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = &fresh_test_world("skull_place").world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let pos = BlockPos::new(0, 0, 0);
        let skull = Skull::player_named("Cuz_Im_Clicks").expect("valid player");
        let skull = skull.custom_name("Cuz_Im_Clicks");
        skull
            .place(
                world,
                pos,
                SkullPlacement::Floor { rotation: 0 },
                UpdateFlags::UPDATE_ALL,
            )
            .expect("placement should work");

        let s = Skull::from_world(world, pos).expect("should succeed");
        assert_eq!(skull, s);
    }
}
