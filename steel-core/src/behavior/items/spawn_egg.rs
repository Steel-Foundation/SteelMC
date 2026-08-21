//! Data-driven vanilla spawn-egg behavior.

use steel_macros::item_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::vanilla_components::ENTITY_DATA;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::{vanilla_blocks, vanilla_game_events};
use steel_utils::types::InteractionHand;

use super::place_on_water_block_item::get_player_pov_hit_result;
use crate::behavior::{
    BLOCK_BEHAVIORS, BlockCollisionContext, InteractionResult, ItemBehavior, UseItemContext,
    UseOnContext,
};
use crate::entity::{
    AgeableMob, EntitySpawnPlacement, EntitySpawnReason, EntitySpawnRequest, LivingEntity, Mob,
    SharedEntity, add_spawned_entity, apply_item_stack_components, create_entity_instance,
    spawn_entity,
};
use crate::world::ClipFluid;
use crate::world::game_event::GameEventContext;

/// Behavior for every data-driven `SpawnEggItem` registry entry.
#[item_behavior(class = "SpawnEggItem")]
pub struct SpawnEggItem;

impl SpawnEggItem {
    fn entity_type(stack: &ItemStack) -> Option<EntityTypeRef> {
        stack.get(ENTITY_DATA).map(|data| data.entity_type())
    }

    fn spawn_mob(
        world: &std::sync::Arc<crate::world::World>,
        player: &crate::player::Player,
        inventory: &crate::behavior::InventoryAccess,
        stack: &ItemStack,
        spawn_pos: steel_utils::BlockPos,
        try_move_down: bool,
        moved_up: bool,
    ) -> InteractionResult {
        let Some(entity_type) = Self::entity_type(stack) else {
            return InteractionResult::Fail;
        };

        let request = EntitySpawnRequest {
            entity_type,
            placement: EntitySpawnPlacement::Block {
                pos: spawn_pos,
                try_move_down,
                moved_up,
            },
            reason: EntitySpawnReason::SpawnItemUse,
            finalize_spawn: true,
            play_ambient_sound: true,
            item_stack: Some(stack),
        };
        if spawn_entity(world, request).is_err() {
            return InteractionResult::Fail;
        }

        inventory.with_item(|item| item.shrink(1));
        world.game_event(
            &vanilla_game_events::ENTITY_PLACE,
            spawn_pos,
            &GameEventContext::new(Some(player), None),
        );
        InteractionResult::Success
    }

    /// Runs the interaction before normal mob handling.
    pub(crate) fn interact_with_mob<M: Mob + ?Sized>(
        stack: &mut ItemStack,
        parent: &M,
    ) -> InteractionResult {
        if Self::spawn_offspring(stack, parent).is_none() {
            return InteractionResult::Pass;
        }
        stack.shrink(1);
        InteractionResult::SuccessServer
    }

    fn spawn_offspring<M: Mob + ?Sized>(stack: &ItemStack, parent: &M) -> Option<SharedEntity> {
        let entity_type = Self::entity_type(stack)?;
        if entity_type != parent.entity_type() {
            return None;
        }

        let world = parent.level()?;
        let offspring = if let Some(animal) = parent.as_animal() {
            animal.get_breed_offspring(&world, animal)?
        } else {
            create_entity_instance(&world, entity_type, parent.position()).ok()?
        };

        let ageable = offspring.as_ageable_mob()?;
        ageable.set_baby(true);
        if !AgeableMob::is_baby(ageable) {
            return None;
        }

        offspring.base().set_position_local(parent.position());
        offspring.set_rotation((0.0, 0.0));
        offspring.set_old_position_to_current();
        offspring.base().set_old_rotation_to_current();
        apply_item_stack_components(&offspring, stack).ok()?;
        add_spawned_entity(&world, offspring.clone()).ok()?;
        Some(offspring)
    }
}

impl ItemBehavior for SpawnEggItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let stack = context.inv.with_item(|item| item.clone());
        if Self::entity_type(&stack).is_none() {
            return InteractionResult::Fail;
        }

        let clicked_pos = context.hit_result.block_pos;
        let clicked_state = context.world.get_block_state(clicked_pos);
        if clicked_state.get_block() == &vanilla_blocks::SPAWNER {
            // Spawner block-entity mutation is a separate foundation; do not
            // incorrectly create a mob beside a spawner in its place.
            return InteractionResult::Fail;
        }

        let clicked_face = context.hit_result.direction;
        let shape = BLOCK_BEHAVIORS
            .get_behavior(clicked_state.get_block())
            .get_collision_shape(
                clicked_state,
                context.world.as_ref(),
                clicked_pos,
                BlockCollisionContext::empty(),
            );
        let spawn_pos = if shape.is_empty() {
            clicked_pos
        } else {
            clicked_face.relative(clicked_pos)
        };

        Self::spawn_mob(
            context.world,
            context.player,
            &context.inv,
            &stack,
            spawn_pos,
            true,
            spawn_pos != clicked_pos && clicked_face == steel_utils::Direction::Up,
        )
    }

    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let hit_result =
            get_player_pov_hit_result(context.world, context.player, ClipFluid::SourceOnly);
        if hit_result.miss {
            return InteractionResult::Pass;
        }

        let stack = context.inv.with_item(|item| item.clone());
        if Self::entity_type(&stack).is_none() {
            return InteractionResult::Fail;
        }

        let pos = hit_result.block_pos;
        let block = context.world.get_block_state(pos).get_block();
        if block != &vanilla_blocks::WATER && block != &vanilla_blocks::LAVA {
            return InteractionResult::Pass;
        }
        if !context.world.may_interact(context.player, pos) {
            return InteractionResult::Fail;
        }

        Self::spawn_mob(
            context.world,
            context.player,
            &context.inv,
            &stack,
            pos,
            false,
            false,
        )
    }

    fn interact_living_entity(
        &self,
        stack: &mut ItemStack,
        _player: &crate::player::Player,
        target: &dyn LivingEntity,
        _hand: InteractionHand,
    ) -> InteractionResult {
        let Some(parent) = target.as_mob() else {
            return InteractionResult::Pass;
        };
        Self::interact_with_mob(stack, parent)
    }
}
