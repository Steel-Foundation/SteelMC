//! Beehive block entity implementation.

use std::any::Any;
use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::{vanilla_block_entity_types, vanilla_entities};
use steel_utils::{BlockPos, BlockStateId};

use crate::block_entity::BlockEntity;
use crate::world::World;

/// Maximum number of occupants in a vanilla beehive.
pub const BEEHIVE_MAX_OCCUPANTS: usize = 3;
/// Minimum occupation time for bees without nectar.
pub const BEEHIVE_MIN_OCCUPATION_TICKS_NECTARLESS: i32 = 600;

struct BeeOccupant {
    entity_data: NbtCompound,
    ticks_in_hive: i32,
    min_ticks_in_hive: i32,
}

impl BeeOccupant {
    fn worldgen(ticks_in_hive: i32) -> Self {
        Self {
            entity_data: default_bee_entity_data(),
            ticks_in_hive,
            min_ticks_in_hive: BEEHIVE_MIN_OCCUPATION_TICKS_NECTARLESS,
        }
    }

    fn load(nbt: NbtCompoundView<'_, '_>) -> Self {
        let entity_data = nbt
            .compound("entity_data")
            .map_or_else(default_bee_entity_data, |entity_data| {
                entity_data.to_owned()
            });
        let ticks_in_hive = nbt.int("ticks_in_hive").unwrap_or(0);
        let min_ticks_in_hive = nbt
            .int("min_ticks_in_hive")
            .unwrap_or(BEEHIVE_MIN_OCCUPATION_TICKS_NECTARLESS);

        Self {
            entity_data,
            ticks_in_hive,
            min_ticks_in_hive,
        }
    }

    fn save(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("entity_data", self.entity_data.clone());
        nbt.insert("ticks_in_hive", self.ticks_in_hive);
        nbt.insert("min_ticks_in_hive", self.min_ticks_in_hive);
        nbt
    }
}

fn default_bee_entity_data() -> NbtCompound {
    let mut entity_data = NbtCompound::new();
    entity_data.insert("id", vanilla_entities::BEE.key.to_string());
    entity_data
}

/// Beehive and bee nest block entity.
///
/// Currently stores and persists occupants for worldgen bee nests. Full vanilla
/// occupant ticking/release is blocked on bee entity support.
pub struct BeehiveBlockEntity {
    level: Weak<World>,
    pos: BlockPos,
    state: BlockStateId,
    removed: bool,
    stored: Vec<BeeOccupant>,
}

impl BeehiveBlockEntity {
    /// Creates a new beehive block entity.
    #[must_use]
    pub const fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            level,
            pos,
            state,
            removed: false,
            stored: Vec::new(),
        }
    }

    /// Stores a vanilla worldgen bee occupant.
    ///
    /// Mirrors `BeehiveBlockEntity.Occupant.create(ticksInHive)`.
    pub fn store_worldgen_bee(&mut self, ticks_in_hive: i32) {
        if self.push_occupant(BeeOccupant::worldgen(ticks_in_hive)) {
            BlockEntity::set_changed(self);
        }
    }

    /// Returns the number of stored occupants.
    #[must_use]
    pub const fn occupant_count(&self) -> usize {
        self.stored.len()
    }

    /// Returns whether the hive currently stores no occupants.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.stored.is_empty()
    }

    fn push_occupant(&mut self, occupant: BeeOccupant) -> bool {
        if self.stored.len() >= BEEHIVE_MAX_OCCUPANTS {
            return false;
        }

        self.stored.push(occupant);
        true
    }
}

impl BlockEntity for BeehiveBlockEntity {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_type(&self) -> BlockEntityTypeRef {
        &vanilla_block_entity_types::BEEHIVE
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

    fn load_additional(&mut self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        self.stored.clear();

        if let Some(bees) = nbt.list("bees")
            && let Some(compounds) = bees.compounds()
        {
            for compound in compounds {
                self.push_occupant(BeeOccupant::load(compound));
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let bees = self
            .stored
            .iter()
            .map(BeeOccupant::save)
            .collect::<Vec<_>>();
        nbt.insert("bees", NbtList::Compound(bees));
    }

    fn is_ticking(&self) -> bool {
        // TODO: Release occupants after their minimum hive time once bee entities exist.
        false
    }
}
