//! Bed block entity impl

use std::any::Any;
use std::sync::{Arc, Weak};

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::loot_table::DyeColor;
use steel_registry::vanilla_block_entity_types;
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
        match state.get_block().key.path.as_ref() {
            "white_bed" => DyeColor::White,
            "orange_bed" => DyeColor::Orange,
            "magenta_bed" => DyeColor::Magenta,
            "light_blue_bed" => DyeColor::LightBlue,
            "yellow_bed" => DyeColor::Yellow,
            "lime_bed" => DyeColor::Lime,
            "pink_bed" => DyeColor::Pink,
            "gray_bed" => DyeColor::Gray,
            "light_gray_bed" => DyeColor::LightGray,
            "cyan_bed" => DyeColor::Cyan,
            "purple_bed" => DyeColor::Purple,
            "blue_bed" => DyeColor::Blue,
            "brown_bed" => DyeColor::Brown,
            "green_bed" => DyeColor::Green,
            "red_bed" => DyeColor::Red,
            "black_bed" => DyeColor::Black,
            other => {
                log::warn!(
                    "unknown bed block '{}' for BedBlockEntity color; defaulting to white",
                    other
                );
                DyeColor::White
            }
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
