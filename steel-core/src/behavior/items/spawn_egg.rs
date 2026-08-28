//! Spawn egg item behavior (`SpawnEggItem`).

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::components::EntityData;
use steel_registry::data_components::vanilla_components::ENTITY_DATA;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::vanilla_game_events;
use steel_utils::{BlockPos, Direction};

use crate::behavior::item_utils::{consume_item_stack, get_player_pov_hit_result};
use crate::behavior::{
    BLOCK_BEHAVIORS, BlockCollisionContext, BlockStateBehaviorExt as _, InteractionResult,
    ItemBehavior, UseItemContext, UseOnContext,
};
use crate::entity::{
    AgeableMob, ENTITIES, Entity, EntitySpawnReason, LivingEntity, SharedEntity, next_entity_id,
    type_spawn,
};
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{ClipFluid, World};

/// Behavior for spawn eggs.
#[item_behavior(class = "SpawnEggItem")]
pub struct SpawnEggItem;

impl SpawnEggItem {
    /// Vanilla `SpawnEggItem.getType`.
    #[must_use]
    pub fn get_type(stack: &ItemStack) -> Option<EntityTypeRef> {
        stack.get(ENTITY_DATA).map(EntityData::entity_type)
    }

    /// Vanilla `SpawnEggItem.spawnsEntity`.
    #[must_use]
    pub fn spawns_entity(stack: &ItemStack, entity_type: EntityTypeRef) -> bool {
        Self::get_type(stack).is_some_and(|spawn_type| spawn_type.key == entity_type.key)
    }

    /// Vanilla `SpawnEggItem.spawnOffspringFromSpawnEgg`.
    #[must_use]
    pub fn spawn_offspring_from_spawn_egg(
        player: &Player,
        parent: &(impl Entity + ?Sized),
        entity_type: EntityTypeRef,
        world: &Arc<World>,
        pos: DVec3,
        spawn_egg_stack: &mut ItemStack,
    ) -> Option<SharedEntity> {
        if !Self::spawns_entity(spawn_egg_stack, entity_type) {
            return None;
        }

        let offspring = if let Some(animal) = parent.as_animal() {
            animal.get_breed_offspring(world, animal)?
        } else {
            if !type_spawn::can_spawn(entity_type, world) {
                return None;
            }
            ENTITIES.create_or_raw(entity_type, next_entity_id(), pos, Arc::downgrade(world))
        };

        if let Some(ageable) = offspring.as_ageable_mob() {
            AgeableMob::set_baby(ageable, true);
        }
        if !offspring
            .as_living_entity()
            .is_some_and(LivingEntity::is_baby)
        {
            return None;
        }

        if offspring.snap_to(pos, 0.0, 0.0).is_err() {
            return None;
        }
        offspring.apply_components_from_item_stack(spawn_egg_stack);

        if let Err(error) = world.try_add_entity(Arc::clone(&offspring)) {
            log::debug!("failed to add spawn-egg offspring: {error}");
        }
        consume_item_stack(spawn_egg_stack, Some(player), 1);
        Some(offspring)
    }

    fn spawn_mob(
        entity_type: EntityTypeRef,
        user: &Player,
        item_stack: &mut ItemStack,
        world: &Arc<World>,
        spawn_pos: BlockPos,
        try_move_down: bool,
        moved_up: bool,
    ) -> InteractionResult {
        if type_spawn::spawn(
            entity_type,
            world,
            Some(item_stack),
            Some(user),
            spawn_pos,
            EntitySpawnReason::SpawnItemUse,
            try_move_down,
            moved_up,
        )
        .is_none()
        {
            return InteractionResult::Fail;
        }

        consume_item_stack(item_stack, Some(user), 1);
        world.game_event(
            &vanilla_game_events::ENTITY_PLACE,
            spawn_pos,
            &GameEventContext::new(Some(user), None),
        );
        InteractionResult::Success
    }
}

impl ItemBehavior for SpawnEggItem {
    fn is_spawn_egg(&self) -> bool {
        true
    }

    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let item_type = context.inv.with_item(|item| Self::get_type(item));
        let Some(entity_type) = item_type else {
            return InteractionResult::Fail;
        };
        if !type_spawn::can_spawn(entity_type, context.world) {
            return InteractionResult::Fail;
        }

        let pos = context.hit_result.block_pos;
        let clicked_face = context.hit_result.direction;
        let block_state = context.world.get_block_state(pos);

        if let Some(block_entity) = context.world.get_block_entity(pos)
            && let Some(spawner) = block_entity.as_spawner()
        {
            spawner.set_entity_id(entity_type);
            context.world.send_block_updated(pos);
            context.world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(Some(context.player), None),
            );
            context.inv.with_item(|item| {
                consume_item_stack(item, Some(context.player), 1);
            });
            return InteractionResult::Success;
        }

        let collision_empty = BLOCK_BEHAVIORS
            .get_behavior(block_state.get_block())
            .get_collision_shape(
                block_state,
                context.world.as_ref(),
                pos,
                BlockCollisionContext::empty(),
            )
            .is_empty();
        let spawn_pos = if collision_empty {
            pos
        } else {
            pos.relative(clicked_face)
        };
        let moved_up = pos != spawn_pos && clicked_face == Direction::Up;

        context.inv.with_item(|item| {
            Self::spawn_mob(
                entity_type,
                context.player,
                item,
                context.world,
                spawn_pos,
                true,
                moved_up,
            )
        })
    }

    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let hit_result =
            get_player_pov_hit_result(context.world, context.player, ClipFluid::SourceOnly);
        if hit_result.miss {
            return InteractionResult::Pass;
        }

        let item_type = context.inv.with_item(|item| Self::get_type(item));
        let Some(entity_type) = item_type else {
            return InteractionResult::Fail;
        };
        if !type_spawn::can_spawn(entity_type, context.world) {
            return InteractionResult::Fail;
        }

        let pos = hit_result.block_pos;
        if !context.world.get_block_state(pos).is_liquid_block() {
            return InteractionResult::Pass;
        }

        let may_use = context.inv.with_item(|item| {
            context.world.may_interact(context.player, pos)
                && context
                    .player
                    .may_use_item_at(pos, hit_result.direction, item)
        });
        if !may_use {
            return InteractionResult::Fail;
        }

        let result = context.inv.with_item(|item| {
            Self::spawn_mob(
                entity_type,
                context.player,
                item,
                context.world,
                pos,
                false,
                false,
            )
        });
        if result == InteractionResult::Success {
            context.inv.with_item(|item| {
                context
                    .player
                    .award_stat(&vanilla_stat_types::ITEM_USED, item.item());
            });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::SpawnEggItem;
    use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};
    use crate::bootstrap::init_globals_once;
    use crate::entity::entities::PigEntity;
    use crate::entity::{AgeableMob, Entity, next_entity_id};
    use crate::player::Player;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;
    use glam::DVec3;
    use std::sync::Arc;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::items::item::BlockHitResult;
    use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_items};
    use steel_utils::types::{Difficulty, InteractionHand, UpdateFlags};
    use steel_utils::{BlockPos, ChunkPos, Direction};

    fn test_player(world: &Arc<World>, name: &str) -> Arc<Player> {
        TestPlayerBuilder::new(Arc::clone(world), name, 1).build()
    }

    fn pig_egg() -> ItemStack {
        ItemStack::with_count(&vanilla_items::PIG_SPAWN_EGG, 2)
    }

    #[test]
    fn get_type_reads_the_entity_data_component() {
        init_globals_once();
        let stack = ItemStack::new(&vanilla_items::PIG_SPAWN_EGG);
        assert_eq!(
            SpawnEggItem::get_type(&stack).map(|entity_type| entity_type.key.path.as_ref()),
            Some("pig")
        );
        assert!(SpawnEggItem::spawns_entity(&stack, &vanilla_entities::PIG));
        assert!(!SpawnEggItem::spawns_entity(&stack, &vanilla_entities::COW));
    }

    #[test]
    fn use_on_spawns_beside_a_solid_block_and_consumes_the_egg() {
        init_globals_once();
        let world = fresh_test_world("spawn_egg_use_on_stone");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let pos = BlockPos::new(0, 64, 0);
        assert!(world.set_block(
            pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_CLIENTS
        ));

        let player = test_player(&world, "spawn_egg_user");
        player
            .inventory
            .lock()
            .set_item_in_hand(InteractionHand::MainHand, pig_egg());

        let mut context = UseOnContext::new(
            &player,
            InteractionHand::MainHand,
            BlockHitResult {
                location: DVec3::new(0.5, 65.0, 0.5),
                direction: Direction::Up,
                block_pos: pos,
                miss: false,
                inside: false,
                world_border_hit: false,
            },
            &world,
            player.inventory.clone(),
        );

        assert_eq!(
            SpawnEggItem.use_on(&mut context),
            InteractionResult::Success
        );
        assert_eq!(
            player
                .inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .count(),
            1
        );

        let spawned = world.get_entities_in_aabb(&steel_utils::WorldAabb::from_min_max(
            DVec3::new(-1.0, 64.0, -1.0),
            DVec3::new(2.0, 68.0, 2.0),
        ));
        assert!(
            spawned
                .iter()
                .any(|entity| entity.entity_type().key.path.as_ref() == "pig"),
            "pig spawn egg should add a pig"
        );
    }

    #[test]
    fn peaceful_rejects_hostile_spawn_eggs() {
        init_globals_once();
        let world = fresh_test_world("spawn_egg_peaceful");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        world.set_difficulty(Difficulty::Peaceful);
        let pos = BlockPos::new(0, 64, 0);
        assert!(world.set_block(
            pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_CLIENTS
        ));

        let player = test_player(&world, "spawn_egg_peaceful");
        player.inventory.lock().set_item_in_hand(
            InteractionHand::MainHand,
            ItemStack::new(&vanilla_items::ZOMBIE_SPAWN_EGG),
        );

        let mut context = UseOnContext::new(
            &player,
            InteractionHand::MainHand,
            BlockHitResult {
                location: DVec3::new(0.5, 65.0, 0.5),
                direction: Direction::Up,
                block_pos: pos,
                miss: false,
                inside: false,
                world_border_hit: false,
            },
            &world,
            player.inventory.clone(),
        );

        assert_eq!(SpawnEggItem.use_on(&mut context), InteractionResult::Fail);
        assert_eq!(
            player
                .inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .count(),
            1
        );
    }

    #[test]
    fn spawn_offspring_from_matching_egg_creates_a_baby() {
        init_globals_once();
        let world = fresh_test_world("spawn_egg_offspring");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let parent = PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(0.5, 64.0, 0.5),
            Arc::downgrade(&world),
        );
        let player = test_player(&world, "spawn_egg_offspring");
        let mut stack = pig_egg();

        let offspring = SpawnEggItem::spawn_offspring_from_spawn_egg(
            &player,
            &parent,
            &vanilla_entities::PIG,
            &world,
            parent.position(),
            &mut stack,
        )
        .expect("matching pig egg should spawn offspring");

        assert!(offspring.as_ageable_mob().is_some_and(AgeableMob::is_baby));
        assert_eq!(stack.count(), 1);
    }
}
