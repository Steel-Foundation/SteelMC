use glam::DVec3;
use std::sync::Arc;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::{level_events, sound_events, vanilla_entities};
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::world::World;

pub struct TntDispenseBehavior;

impl DispenseItemBehavior for TntDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        mut item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);

        let spawn_pos = DVec3::new(
            target_pos.x() as f64 + 0.5,
            target_pos.y() as f64,
            target_pos.z() as f64 + 0.5,
        );

        let id = crate::entity::next_entity_id();
        if let Some(entity) = crate::entity::ENTITIES.create(
            &vanilla_entities::TNT,
            id,
            spawn_pos,
            Arc::downgrade(world),
        ) {
            let _ = world.try_add_entity(entity);
            world.play_sound(
                &sound_events::ENTITY_TNT_PRIMED,
                SoundSource::Blocks,
                pos,
                1.0,
                1.0,
                None,
            );
            world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);
            item.shrink(1);
        }

        item
    }
}
