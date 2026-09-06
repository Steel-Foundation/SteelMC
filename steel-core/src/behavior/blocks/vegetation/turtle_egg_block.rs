//! Vanilla turtle egg block behavior.
//!
//! Turtle eggs are placed in clusters of one to four in a single block space.
//! While sitting on sand they slowly crack (three stages) and then hatch, and
//! they are trampled by most living entities that walk or fall onto them.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{
    level_events, sound_events, vanilla_entities, vanilla_game_events, vanilla_game_rules,
    vanilla_items, vanilla_world_clocks,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{
    BlockBehavior, EntityFallDamage, EntityFallOnContext, default_can_be_replaced,
};
use crate::behavior::context::BlockPlaceContext;
use crate::block_entity::SharedBlockEntity;
use crate::entity::Entity;
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;

/// Cracking stages a turtle egg passes through before it hatches.
const MAX_HATCH_LEVEL: u8 = 2;
/// Maximum number of eggs that can occupy a single block space.
const MAX_EGGS: u8 = 4;

/// Length of one full Minecraft day, in ticks.
const TICKS_PER_DAY: i64 = 24_000;
/// Base per-random-tick chance that an egg advances a cracking stage.
const BASE_HATCH_CHANCE: f32 = 0.002;
/// Start (inclusive) of the pre-dawn window where eggs always advance.
const HATCH_WINDOW_START: i64 = 21_062;
/// End (exclusive) of the pre-dawn window where eggs always advance.
const HATCH_WINDOW_END: i64 = 21_905;

/// One in this many chance to trample an egg while standing on it each tick.
const STEP_TRAMPLE_ODDS: i32 = 100;
/// One in this many chance to trample an egg by falling onto it.
const FALL_TRAMPLE_ODDS: i32 = 3;

/// Particle count for the "egg placed on sand" effect.
const TURTLE_EGG_PLACEMENT_PARTICLE_COUNT: i32 = 15;

/// Dimension timeline tag that carries the day timeline. Only dimensions on this
/// tag (the overworld and its caves variant) apply the pre-dawn hatch boost;
/// nether and end eggs stay at the base chance, matching vanilla's per-dimension
/// resolution of the `gameplay/turtle_egg_hatch_chance` attribute.
const OVERWORLD_TIMELINE_TAG: &str = "#minecraft:in_overworld";

const HATCH: &IntProperty = &BlockStateProperties::HATCH;
const EGGS: &IntProperty = &BlockStateProperties::EGGS;

/// Behavior for vanilla turtle eggs.
#[block_behavior]
pub struct TurtleEggBlock {
    block: BlockRef,
}

impl TurtleEggBlock {
    /// Creates a new turtle egg block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns whether the block below `pos` is a sand type (sand, red sand, or
    /// suspicious sand). Turtle eggs only crack and hatch on top of sand.
    fn on_sand(world: &Arc<World>, pos: BlockPos) -> bool {
        world
            .get_block_state(pos.below())
            .get_block()
            .has_tag(&BlockTag::SAND)
    }

    /// Rolls whether an egg should advance its cracking stage this random tick.
    ///
    /// In 26.2 this chance comes from the `gameplay/turtle_egg_hatch_chance`
    /// environment attribute: base 0.002, raised to 1.0 by a day-timeline
    /// `maximum` modifier during the pre-dawn window (ticks 21062 to 21904).
    /// Vanilla resolves the attribute per dimension, and only dimensions tagged
    /// `#minecraft:in_overworld` include that day timeline, so eggs in the nether
    /// or end keep the 0.002 base regardless of the overworld time of day.
    ///
    /// Steel's timeline sampler in `world::environment` only handles
    /// `multiply`/replace modifiers and exposes sky light and sun angle, so it
    /// cannot resolve this attribute yet; the turtle curve is reproduced inline
    /// off the overworld day clock, gated on the same timeline tag.
    // TODO(environment-attributes): replace this inline curve with a single
    // attribute lookup once the timeline sampler learns the `maximum` modifier
    // and exposes a public environment-attribute getter.
    fn should_update_hatch_level(world: &Arc<World>) -> bool {
        let day_time = world
            .clock_total_ticks(&vanilla_world_clocks::OVERWORLD)
            .unwrap_or(0)
            .rem_euclid(TICKS_PER_DAY);

        let in_hatch_window = (HATCH_WINDOW_START..HATCH_WINDOW_END).contains(&day_time)
            && world.dimension_type.timelines == Some(OVERWORLD_TIMELINE_TAG);
        let chance = if in_hatch_window {
            1.0
        } else {
            BASE_HATCH_CHANCE
        };

        chance > 0.0 && rand::random::<f32>() < chance
    }

    /// Vanilla `TurtleEggBlock.canDestroyEgg`: which entities are able to trample
    /// an egg. Turtles and bats never do, and only living entities can; players
    /// always may, other mobs only when `mobGriefing` is enabled.
    fn can_destroy_egg(world: &Arc<World>, entity: &dyn Entity) -> bool {
        let entity_type = entity.entity_type();
        if entity_type == &vanilla_entities::TURTLE || entity_type == &vanilla_entities::BAT {
            return false;
        }
        if !entity.is_living_entity() {
            return false;
        }
        entity_type == &vanilla_entities::PLAYER
            || world.get_game_rule(&vanilla_game_rules::MOB_GRIEFING)
    }

    /// Vanilla `TurtleEggBlock.destroyEgg`: a random chance to remove one egg
    /// from the cluster when a qualifying entity steps or falls on it.
    fn destroy_egg(
        &self,
        world: &Arc<World>,
        state: BlockStateId,
        pos: BlockPos,
        entity: &dyn Entity,
        odds: i32,
    ) {
        if state.get_block() == self.block
            && Self::can_destroy_egg(world, entity)
            && rand::random_range(0..odds) == 0
        {
            Self::decrease_eggs(world, pos, state);
        }
    }

    /// Vanilla `TurtleEggBlock.decreaseEggs`: removes one egg from the cluster,
    /// destroying the block once the last egg is gone.
    fn decrease_eggs(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        world.play_block_sound(
            &sound_events::ENTITY_TURTLE_EGG_BREAK,
            pos,
            0.7,
            0.9 + rand::random::<f32>() * 0.2,
            None,
        );

        let eggs = state.get_value(EGGS);
        if eggs <= 1 {
            world.destroy_block(pos, false);
        } else {
            world.set_block(
                pos,
                state.set_value(EGGS, eggs - 1),
                UpdateFlags::UPDATE_CLIENTS,
            );
            world.game_event(
                &vanilla_game_events::BLOCK_DESTROY,
                pos,
                &GameEventContext::new(None, Some(state)),
            );
            world.level_event(
                level_events::PARTICLES_DESTROY_BLOCK,
                pos,
                level_events::encode_block_state_data(u32::from(state.0)),
                None,
            );
        }
    }
}

impl BlockBehavior for TurtleEggBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let existing = context.world.get_block_state(context.place_pos());
        if existing.get_block() == self.block {
            // Clicking an existing cluster with another egg adds to it.
            Some(existing.set_value(EGGS, (existing.get_value(EGGS) + 1).min(MAX_EGGS)))
        } else {
            Some(self.block.default_state())
        }
    }

    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        if !context.is_secondary_use_active()
            && context.with_item(|item| item.is(&vanilla_items::TURTLE_EGG))
            && state.get_value(EGGS) < MAX_EGGS
        {
            true
        } else {
            default_can_be_replaced(state, context)
        }
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        // Play the "placed on sand" particle effect (data is the particle count).
        if Self::on_sand(world, pos) {
            world.level_event(
                level_events::PARTICLES_TURTLE_EGG_PLACEMENT,
                pos,
                TURTLE_EGG_PLACEMENT_PARTICLE_COUNT,
                None,
            );
        }
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !Self::should_update_hatch_level(world) || !Self::on_sand(world, pos) {
            return;
        }

        let hatch = state.get_value(HATCH);
        if hatch < MAX_HATCH_LEVEL {
            world.play_block_sound(
                &sound_events::ENTITY_TURTLE_EGG_CRACK,
                pos,
                0.7,
                0.9 + rand::random::<f32>() * 0.2,
                None,
            );
            world.set_block(
                pos,
                state.set_value(HATCH, hatch + 1),
                UpdateFlags::UPDATE_CLIENTS,
            );
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(None, Some(state)),
            );
        } else {
            world.play_block_sound(
                &sound_events::ENTITY_TURTLE_EGG_HATCH,
                pos,
                0.7,
                0.9 + rand::random::<f32>() * 0.2,
                None,
            );
            world.remove_block(pos, false);
            world.game_event(
                &vanilla_game_events::BLOCK_DESTROY,
                pos,
                &GameEventContext::new(None, Some(state)),
            );

            let eggs = state.get_value(EGGS);
            for _ in 0..eggs {
                world.level_event(
                    level_events::PARTICLES_DESTROY_BLOCK,
                    pos,
                    level_events::encode_block_state_data(u32::from(state.0)),
                    None,
                );
                // TODO(turtle-entity): spawn one baby Turtle per egg here once the
                // Turtle entity lands. Vanilla creates it with age -24000, calls
                // setHomePos(pos), snaps it just above the nest, and adds it to the
                // world. Wired up in the follow-up entity PR.
            }
        }
    }

    fn step_on(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        // Sneaking ("stepping carefully") avoids trampling.
        if !entity.is_stepping_carefully() {
            self.destroy_egg(world, state, pos, entity, STEP_TRAMPLE_ODDS);
        }
        self.default_step_on(state, world, pos, entity);
    }

    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        if let Some(entity) = context.source_entity()
            && entity.entity_type() != &vanilla_entities::ZOMBIE
        {
            // Vanilla excludes `instanceof Zombie`, which also covers husks,
            // drowned, zombie villagers, and zombified piglins. None of those
            // exist in Steel yet, so this single-type check is currently exact.
            // TODO(zombie-family): widen this to the whole zombie set (a shared
            // "is a zombie" predicate or an entity-type tag) once those entities
            // land, so falling zombies do not double up with their trample AI.
            self.destroy_egg(world, state, pos, entity, FALL_TRAMPLE_ODDS);
        }
        self.default_fall_on(state, world, pos, context)
    }

    fn player_destroy(
        &self,
        world: &Arc<World>,
        _player: &Player,
        pos: BlockPos,
        state: BlockStateId,
        _block_entity: Option<&SharedBlockEntity>,
        _tool: &ItemStack,
    ) {
        // Vanilla calls super.playerDestroy (loot/stats) and then decreaseEggs so a
        // cluster is broken one egg at a time. Steel's break pipeline
        // (game_mode::block_breaking::destroy_block) already removes the block
        // before invoking player_destroy, exactly like vanilla, so decrease_eggs
        // re-places the cluster with one fewer egg when more than one remained.
        Self::decrease_eggs(world, pos, state);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_world_clocks};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    /// A day-time tick inside the pre-dawn window where eggs always advance, so
    /// random ticks are deterministic in tests.
    const ALWAYS_HATCH_DAY_TIME: i64 = 21_500;

    fn prepare(key: &'static str) -> (Arc<World>, BlockPos) {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world(key);
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        world.set_clock_total_ticks(&vanilla_world_clocks::OVERWORLD, ALWAYS_HATCH_DAY_TIME);
        (world, pos)
    }

    #[test]
    fn eggs_crack_twice_then_hatch_on_sand() {
        let (world, pos) = prepare("turtle_egg_hatch");
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::SAND.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            pos,
            vanilla_blocks::TURTLE_EGG.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::TURTLE_EGG);

        behavior.random_tick(world.get_block_state(pos), &world, pos);
        assert_eq!(world.get_block_state(pos).get_value(HATCH), 1);

        behavior.random_tick(world.get_block_state(pos), &world, pos);
        assert_eq!(world.get_block_state(pos).get_value(HATCH), 2);

        // Final advance hatches the egg and removes the block. Spawning the baby
        // turtle is stubbed until the Turtle entity lands, so only the removal is
        // asserted here.
        behavior.random_tick(world.get_block_state(pos), &world, pos);
        assert!(world.get_block_state(pos).is_air());
    }

    #[test]
    fn eggs_do_not_advance_off_sand() {
        let (world, pos) = prepare("turtle_egg_off_sand");
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            pos,
            vanilla_blocks::TURTLE_EGG.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::TURTLE_EGG);

        behavior.random_tick(world.get_block_state(pos), &world, pos);
        assert_eq!(world.get_block_state(pos).get_value(HATCH), 0);
    }
}
