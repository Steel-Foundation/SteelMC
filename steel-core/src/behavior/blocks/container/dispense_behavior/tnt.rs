use std::sync::Arc;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::{sound_events, vanilla_entities, vanilla_game_events, vanilla_game_rules};
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use super::{DispenseItemBehavior, DispenseResult};
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::entity::{Entity, entities::PrimedTntEntity, next_entity_id};

use crate::world::World;
use crate::world::game_event::GameEventContext;

pub struct TntDispenseBehavior;

impl DispenseItemBehavior for TntDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult {
        if !world.get_game_rule(&vanilla_game_rules::TNT_EXPLODES) {
            return DispenseResult::Failure;
        }

        // TODO: sulfur cube - vanilla first tries SulfurCubeBlockDispenseItemBehavior.dispenseBlock.
        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);

        let tnt = Arc::new(PrimedTntEntity::primed(
            &vanilla_entities::TNT,
            next_entity_id(),
            target_pos.get_bottom_center(),
            world,
            None,
        ));
        let tnt_pos = tnt.position();
        let _ = world.try_add_entity(tnt);
        world.play_sound_at(
            &sound_events::ENTITY_TNT_PRIMED,
            SoundSource::Blocks,
            tnt_pos,
            1.0,
            1.0,
            None,
        );
        world.game_event(
            &vanilla_game_events::ENTITY_PLACE,
            target_pos,
            &GameEventContext::default(),
        );

        item.shrink(1);
        DispenseResult::Success
    }
}
