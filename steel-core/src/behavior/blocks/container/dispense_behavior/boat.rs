use glam::DVec3;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use super::DefaultDispenseBehavior;
use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::world::World;

pub struct BoatDispenseBehavior {
    entity_type: EntityTypeRef,
}

impl BoatDispenseBehavior {
    pub fn new(entity_type: EntityTypeRef) -> Self {
        Self { entity_type }
    }
}

impl DispenseItemBehavior for BoatDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let offset = facing.offset();
        let center = pos.get_center();

        // TODO Move Magic Number
        let just_outside_dispenser: f64 = (0.5625 + self.entity_type.dimensions.width / 2.0).into();
        let spawn_pos = DVec3::new(
            center.0 + offset.0 as f64 * just_outside_dispenser,
            center.1 + offset.1 as f64 * 1.125,
            center.2 + offset.2 as f64 * just_outside_dispenser,
        );
        let front_pos = pos.relative(facing);

        let y_offset = if world
            .get_block_state(front_pos)
            .get_fluid_state()
            .is_water()
        {
            1.0
        } else {
            if world.get_block_state(front_pos).is_air()
                || !world
                    .get_block_state(front_pos.below())
                    .get_fluid_state()
                    .is_water()
            {
                return DefaultDispenseBehavior.dispense(world, pos, state, item);
            }
            0.0
        };

        // TODO implement boat spawning

        ItemStack::empty()
    }
}
