//! Vanilla `Display.BlockDisplay`: renders a block state without collision.

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entity_data::BlockDisplayEntityData;
use steel_utils::BlockStateId;
use steel_utils::locks::CheckedMutex;

use crate::block_entity::block_state_nbt;
use crate::entity::prelude::*;
use crate::entity::{EntityBase, EntityProps};

/// A block display.
// TODO: persist the shared `Display` state (transformation, billboard,
// interpolation, view range, shadow, brightness, glow color, size).
#[entity_behavior(class = "BlockDisplay")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_block_display, load = Self::load_block_display)]
#[entity(key = "steel:entity/block_display", constructors)]
pub struct BlockDisplayEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<BlockDisplayEntityData>,
}

impl BlockDisplayEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        // `Display.tick` never calls `super`.
        tick: Self::tick_display,
        piston_push_reaction: |_| PushReaction::IgnoreEntity,
        is_ignoring_block_triggers: |_| true,
        ..EntityProps::new()
    };

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
    use std::sync::Arc;

    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;
    use glam::DVec3;
    use std::sync::Weak;

    use crate::entity::{RemovalReason, SharedEntity};
    use crate::test_support::load_additional;

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

        let loaded = BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        load_additional(&*loaded, &nbt);
        assert_eq!(loaded.block_state(), state);
    }

    #[test]
    fn block_display_leaves_a_removed_vehicle() {
        init_vanilla_registry();
        let display: SharedEntity = BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            1,
            DVec3::ZERO,
            Weak::new(),
        );
        let vehicle: SharedEntity = BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        EntityBase::restore_passenger_relationship(&vehicle, &display);
        vehicle.set_removed(RemovalReason::Discarded);

        Arc::clone(&display).tick();
        assert!(display.vehicle().is_none());
    }
}
