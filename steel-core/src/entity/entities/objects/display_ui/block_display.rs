//! Vanilla `Display.BlockDisplay`: renders a block state without collision.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entity_data::BlockDisplayEntityData;
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::block_state_nbt;
use crate::entity::prelude::*;
use crate::entity::{EntityBase, EntityBaseLoad, EntityProps};
use crate::world::World;

/// A block display.
// TODO: persist the shared `Display` state (transformation, billboard,
// interpolation, view range, shadow, brightness, glow color, size).
#[entity_behavior(class = "BlockDisplay")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_block_display, load = Self::load_block_display)]
pub struct BlockDisplayEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<BlockDisplayEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BlockDisplayEntity`.
unsafe impl DowncastType for BlockDisplayEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/block_display");
}

impl BlockDisplayEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: |display, _| display.tick_display(),
        piston_push_reaction: |_| PushReaction::IgnoreEntity,
        is_ignoring_block_triggers: |_| true,
        ..EntityProps::new()
    };

    /// Creates a new block display.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a block display from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: CheckedMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// The displayed block state.
    #[must_use]
    pub fn block_state(&self) -> BlockStateId {
        *self.entity_data.lock().block_state.get()
    }

    /// Sets the displayed block state.
    pub fn set_block_state(&self, state: BlockStateId) {
        self.entity_data.lock().block_state.set(state);
    }

    /// `Display.tick` on the server: leave a removed vehicle. Displays skip the
    /// base entity tick.
    fn tick_display(&self) {
        if self.vehicle().is_some_and(|vehicle| vehicle.is_removed()) {
            self.stop_riding();
        }
    }

    fn save_block_display(&self, nbt: &mut NbtCompound) {
        nbt.insert(
            "block_state",
            NbtTag::Compound(block_state_nbt::save(self.block_state())),
        );
    }

    fn load_block_display(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let state = nbt
            .compound("block_state")
            .and_then(block_state_nbt::load)
            .unwrap_or_else(|| vanilla_blocks::AIR.default_state());
        self.set_block_state(state);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Arc;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;
    use crate::entity::{RemovalReason, SharedEntity};

    #[test]
    fn block_display_saves_its_block_state_by_name() {
        init_vanilla_registry();
        let display = BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            1,
            DVec3::ZERO,
            Weak::new(),
        );
        let state = vanilla_blocks::OAK_STAIRS.default_state();
        display.set_block_state(state);

        let mut nbt = NbtCompound::new();
        display.save_additional(&mut nbt);
        assert_eq!(
            nbt.compound("block_state")
                .and_then(|state| state.string("id"))
                .map(ToString::to_string),
            Some("minecraft:oak_stairs".to_owned())
        );
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

        let loaded = BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        loaded.load_additional((&borrowed).into());
        assert_eq!(loaded.block_state(), state);
    }

    #[test]
    fn block_display_leaves_a_removed_vehicle() {
        init_vanilla_registry();
        let display: SharedEntity = Arc::new(BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let vehicle: SharedEntity = Arc::new(BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));
        EntityBase::restore_passenger_relationship(&vehicle, &display);
        vehicle.set_removed(RemovalReason::Discarded);

        Arc::clone(&display).tick();
        assert!(display.vehicle().is_none());
    }
}
