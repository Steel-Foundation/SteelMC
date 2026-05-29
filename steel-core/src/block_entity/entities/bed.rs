//! Bed block entity impl

use std::any::Any;
use std::sync::{Arc, Weak};

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::loot_table::DyeColor;
use steel_registry::{vanilla_block_entity_types, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId};

use crate::block_entity::BlockEntity;
use crate::world::World;

/// Bed block entity
pub struct BedBlockEntity {
    level: Weak<World>,
    pos: BlockPos,
    state: BlockStateId,
    removed: bool,
    color: DyeColor,
}

impl BedBlockEntity {
    /// Creates a new bed block entity
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self::with_color(level, pos, state, Self::color_from_state(state))
    }

    /// Creates a new bed block entity with an explicit color
    #[must_use]
    pub const fn with_color(
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
        color: DyeColor,
    ) -> Self {
        Self {
            level,
            pos,
            state,
            removed: false,
            color,
        }
    }

    /// Returns this bed color
    #[must_use]
    pub const fn get_color(&self) -> DyeColor {
        self.color
    }

    fn color_from_state(state: BlockStateId) -> DyeColor {
        let block = state.get_block();
        if block == &vanilla_blocks::WHITE_BED {
            DyeColor::White
        } else if block == &vanilla_blocks::ORANGE_BED {
            DyeColor::Orange
        } else if block == &vanilla_blocks::MAGENTA_BED {
            DyeColor::Magenta
        } else if block == &vanilla_blocks::LIGHT_BLUE_BED {
            DyeColor::LightBlue
        } else if block == &vanilla_blocks::YELLOW_BED {
            DyeColor::Yellow
        } else if block == &vanilla_blocks::LIME_BED {
            DyeColor::Lime
        } else if block == &vanilla_blocks::PINK_BED {
            DyeColor::Pink
        } else if block == &vanilla_blocks::GRAY_BED {
            DyeColor::Gray
        } else if block == &vanilla_blocks::LIGHT_GRAY_BED {
            DyeColor::LightGray
        } else if block == &vanilla_blocks::CYAN_BED {
            DyeColor::Cyan
        } else if block == &vanilla_blocks::PURPLE_BED {
            DyeColor::Purple
        } else if block == &vanilla_blocks::BLUE_BED {
            DyeColor::Blue
        } else if block == &vanilla_blocks::BROWN_BED {
            DyeColor::Brown
        } else if block == &vanilla_blocks::GREEN_BED {
            DyeColor::Green
        } else if block == &vanilla_blocks::RED_BED {
            DyeColor::Red
        } else if block == &vanilla_blocks::BLACK_BED {
            DyeColor::Black
        } else {
            log::warn!(
                "unknown bed block '{}' for BedBlockEntity color; defaulting to white",
                block.key
            );
            DyeColor::White
        }
    }
}

impl BlockEntity for BedBlockEntity {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_type(&self) -> BlockEntityTypeRef {
        &vanilla_block_entity_types::BED
    }

    fn get_block_pos(&self) -> BlockPos {
        self.pos
    }

    fn get_block_state(&self) -> BlockStateId {
        self.state
    }

    fn set_block_state(&mut self, state: BlockStateId) {
        self.state = state;
    }

    fn is_removed(&self) -> bool {
        self.removed
    }

    fn set_removed(&mut self) {
        self.removed = true;
    }

    fn clear_removed(&mut self) {
        self.removed = false;
    }

    fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    fn load_additional(&mut self, _nbt: &BorrowedNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}
}
