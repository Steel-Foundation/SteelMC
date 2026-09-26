use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_damage_type_tags;
use steel_utils::BlockPos;

use super::random_pos::default_random_pos;
use super::selector::{Goal, GoalControls};
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::entity::PathfinderMob;
use crate::fluid::FluidStateExt as _;

const WATER_CHECK_DISTANCE_VERTICAL: i32 = 1;
const ON_FIRE_WATER_SEARCH_RANGE: i32 = 5;
const ESCAPE_DISTANCE_HORIZONTAL: i32 = 5;
const ESCAPE_DISTANCE_VERTICAL: i32 = 4;

pub struct PanicGoal {
    wanted_position: Option<DVec3>,
    speed_modifier: f64,
    is_running: bool,
    always_seeks_water: bool,
    water_search_range: i32,
}

impl PanicGoal {
    #[must_use]
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            wanted_position: None,
            speed_modifier,
            is_running: false,
            always_seeks_water: false,
            water_search_range: ON_FIRE_WATER_SEARCH_RANGE,
        }
    }

    /// Heads for water within `range` blocks whenever it panics, not only while on fire.
    #[must_use]
    pub(crate) const fn always_seeking_water(mut self, range: i32) -> Self {
        self.always_seeks_water = true;
        self.water_search_range = range;
        self
    }

    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.is_running
    }

    fn should_panic(mob: &dyn PathfinderMob) -> bool {
        mob.last_damage_source()
            .is_some_and(|source| source.is(&vanilla_damage_type_tags::DamageTypeTag::PANIC_CAUSES))
    }

    fn find_random_position(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(position) =
            default_random_pos(mob, ESCAPE_DISTANCE_HORIZONTAL, ESCAPE_DISTANCE_VERTICAL)
        else {
            return false;
        };

        self.wanted_position = Some(position);
        true
    }
}

impl Goal for PanicGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn is_panic_goal(&self) -> bool {
        true
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        if !Self::should_panic(mob) {
            return false;
        }

        if (self.always_seeks_water || mob.is_on_fire())
            && let Some(water_pos) = look_for_water(mob, self.water_search_range)
        {
            self.wanted_position = Some(block_pos_corner(water_pos));
            return true;
        }

        self.find_random_position(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        !mob.mob_base().navigation().lock().is_done()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        if let Some(wanted_position) = self.wanted_position {
            mob.move_to_pos(wanted_position, self.speed_modifier);
        }
        self.is_running = true;
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.is_running = false;
    }
}

fn look_for_water(mob: &dyn PathfinderMob, xz_dist: i32) -> Option<BlockPos> {
    let world = mob.level()?;
    let mob_position = mob.block_position();
    let block_state = world.get_block_state(mob_position);
    let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
    if !behavior
        .get_collision_shape(
            block_state,
            world.as_ref(),
            mob_position,
            BlockCollisionContext::empty(),
        )
        .is_empty()
    {
        return None;
    }

    mob_position.find_closest_match(xz_dist, WATER_CHECK_DISTANCE_VERTICAL, |pos| {
        world.get_block_state(pos).get_fluid_state().is_water()
    })
}

fn block_pos_corner(pos: BlockPos) -> DVec3 {
    DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()))
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use std::sync::Arc;

    use steel_registry::{
        init_vanilla_registry, vanilla_blocks, vanilla_damage_types, vanilla_entities,
    };
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::damage::DamageSource;
    use crate::entity::entities::PigEntity;
    use crate::entity::{LivingEntity, SharedEntity, next_entity_id};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk, test_world};

    #[test]
    fn panic_goal_uses_move_control() {
        let goal = PanicGoal::new(1.25);

        assert_eq!(goal.controls(), GoalControls::MOVE);
        assert!(!goal.is_running());
    }

    #[test]
    fn panic_goal_uses_vanilla_panic_damage_tag() {
        init_vanilla_registry();
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!PanicGoal::should_panic(&pig));

        assert!(pig.hurt_server(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            1.0
        ));
        assert!(!PanicGoal::should_panic(&pig));

        assert!(pig.hurt_server(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK),
            2.0
        ));
        assert!(PanicGoal::should_panic(&pig));
    }

    #[test]
    fn a_goal_always_seeking_water_heads_for_water_without_being_on_fire() {
        const WATER_SEARCH_RANGE: i32 = 7;

        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("panic_goal_always_seeks_water");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let water = BlockPos::new(8, 64, 14);
        world.set_block(
            water,
            vanilla_blocks::WATER.default_state(),
            UpdateFlags::UPDATE_NONE,
        );
        let pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&pig) as SharedEntity)
            .expect("pig should attach to the loaded test chunk");
        assert!(pig.hurt_server(
            &world,
            &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK),
            1.0
        ));
        let mut goal = PanicGoal::new(1.25).always_seeking_water(WATER_SEARCH_RANGE);

        assert!(goal.can_use(pig.as_ref()));
        assert_eq!(goal.wanted_position, Some(block_pos_corner(water)));
    }
}
