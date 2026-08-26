//! Turtle reproduction goals: breeding into an egg, and laying it on home sand.

use glam::DVec3;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_registry::{sound_events, vanilla_blocks, vanilla_game_events};
use steel_utils::types::UpdateFlags;

use super::{TurtleEntity, as_turtle, closer_to_center_than};
use crate::entity::ai::goal::{Goal, GoalControls, MoveToBlockGoal, reduced_tick_delay};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::entities::ExperienceOrbEntity;
use crate::entity::{AgeableMob, Animal, PathfinderMob, SharedEntity};
use crate::world::game_event::GameEventContext;

/// Number of eggs stored in a placed turtle egg cluster (`1..=4`).
const EGGS: &IntProperty = &BlockStateProperties::EGGS;
/// Vanilla `TurtleBreedGoal` partner search radius.
const PARTNER_SEARCH_RANGE: f64 = 8.0;
/// Vanilla `BreedGoal.SPAWN_CHILD_DISTANCE` squared: how close parents must be.
const BREED_DISTANCE_SQR: f64 = 9.0;
/// Vanilla `BreedGoal` love timer length before an egg is granted.
const BREED_TIME: i32 = 60;

/// Vanilla `Turtle.TurtleBreedGoal`: breeding gives the mother an egg to lay
/// instead of spawning a baby, and both parents grow back to adulthood.
///
/// Steel's shared `BreedGoal` spawns the child directly in its tick with no
/// override point, so the partner search and love timer are reimplemented here to
/// substitute the egg-granting step. A `breed()`-style hook on the shared goal
/// would remove this duplication; that is raised for discussion on the PR.
pub(crate) struct TurtleBreedGoal {
    partner: Option<SharedEntity>,
    love_time: i32,
    speed_modifier: f64,
}

impl TurtleBreedGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            partner: None,
            love_time: 0,
            speed_modifier,
        }
    }

    fn find_partner(mob: &dyn PathfinderMob, animal: &dyn Animal) -> Option<SharedEntity> {
        let world = mob.level()?;
        let search_box = mob.bounding_box().inflate(PARTNER_SEARCH_RANGE);
        let partner_targeting = TargetingConditions::for_non_combat()
            .range(PARTNER_SEARCH_RANGE)
            .ignore_line_of_sight();

        world.nearest_entity_in_aabb_matching(&search_box, mob.position(), |entity| {
            let Some(candidate) = entity.as_animal() else {
                return false;
            };
            if !partner_targeting.test(world.as_ref(), Some(mob), candidate) {
                return false;
            }
            if !animal.can_mate(candidate) {
                return false;
            }

            !entity
                .as_pathfinder_mob()
                .is_some_and(PathfinderMob::is_panicking)
        })
    }

    /// Vanilla `TurtleBreedGoal.breed`: mark the mother as carrying an egg, age
    /// both parents back to adulthood, clear love mode, and drop breeding XP.
    fn breed(mob: &dyn PathfinderMob, turtle: &TurtleEntity, partner_animal: &dyn Animal) {
        let Some(world) = mob.level() else {
            return;
        };

        turtle.set_has_egg(true);
        turtle.set_age(6000);
        partner_animal.set_age(6000);
        turtle.reset_love();
        partner_animal.reset_love();

        if world.get_game_rule(&MOB_DROPS) {
            let xp = rand::random_range(1..8);
            ExperienceOrbEntity::award(&world, mob.position(), xp);
        }
    }
}

impl Goal for TurtleBreedGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        if turtle.has_egg() || !turtle.is_in_love() {
            return false;
        }

        self.partner = Self::find_partner(mob, turtle);
        self.partner.is_some()
    }

    fn can_continue_to_use(&mut self, _mob: &dyn PathfinderMob) -> bool {
        let Some(partner) = &self.partner else {
            return false;
        };
        if !partner.is_alive() || self.love_time >= BREED_TIME {
            return false;
        }
        if partner
            .as_pathfinder_mob()
            .is_some_and(PathfinderMob::is_panicking)
        {
            return false;
        }

        partner.as_animal().is_some_and(Animal::is_in_love)
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.partner = None;
        self.love_time = 0;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(partner) = &self.partner else {
            return;
        };
        let Some(turtle) = as_turtle(mob) else {
            return;
        };
        let Some(partner_animal) = partner.as_animal() else {
            return;
        };

        let partner_position = partner.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(partner_position.x, partner.get_eye_y(), partner_position.z),
            10.0,
            mob.max_head_x_rot(),
        );
        mob.move_to_pos(partner_position, self.speed_modifier);

        self.love_time += 1;
        if self.love_time < reduced_tick_delay(BREED_TIME)
            || mob.position().distance_squared(partner_position) >= BREED_DISTANCE_SQR
        {
            return;
        }

        Self::breed(mob, turtle, partner_animal);
    }
}

/// Vanilla `Turtle.TurtleLayEggGoal`: walk to sand near home and, after a delay,
/// place a turtle egg cluster and clear the carried egg.
pub(crate) struct TurtleLayEggGoal {
    inner: MoveToBlockGoal,
}

impl TurtleLayEggGoal {
    pub(crate) fn new(speed_modifier: f64) -> Self {
        Self {
            inner: MoveToBlockGoal::new(speed_modifier, 16, |level, pos| {
                level.get_block_state(pos.above()).is_air()
                    && level
                        .get_block_state(pos)
                        .get_block()
                        .has_tag(&BlockTag::SAND)
            }),
        }
    }

    fn within_home(turtle: &TurtleEntity, mob: &dyn PathfinderMob) -> bool {
        closer_to_center_than(turtle.home_pos(), mob.position(), 9.0)
    }

    fn place_egg(&self, mob: &dyn PathfinderMob, turtle: &TurtleEntity) {
        let Some(world) = mob.level() else {
            return;
        };

        let egg_pos = self.inner.block_pos().above();
        let count = rand::random_range(1..=4u8);
        let egg_state = vanilla_blocks::TURTLE_EGG
            .default_state()
            .set_value(EGGS, count);

        world.play_sound(
            &sound_events::ENTITY_TURTLE_LAY_EGG,
            SoundSource::Blocks,
            mob.block_position(),
            0.3,
            0.9 + rand::random::<f32>() * 0.2,
            None,
        );
        world.set_block(egg_pos, egg_state, UpdateFlags::UPDATE_ALL);
        world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            egg_pos,
            &GameEventContext::new(Some(turtle), Some(egg_state)),
        );

        turtle.set_has_egg(false);
        turtle.set_laying_egg(false);
        turtle.set_in_love_time(600);
    }
}

impl Goal for TurtleLayEggGoal {
    fn controls(&self) -> GoalControls {
        self.inner.controls()
    }

    fn requires_update_every_tick(&self) -> bool {
        self.inner.requires_update_every_tick()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        turtle.has_egg() && Self::within_home(turtle, mob) && self.inner.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(turtle) = as_turtle(mob) else {
            return false;
        };
        self.inner.can_continue_to_use(mob) && turtle.has_egg() && Self::within_home(turtle, mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.inner.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.inner.tick(mob);

        let Some(turtle) = as_turtle(mob) else {
            return;
        };
        if mob.is_in_water() || !self.inner.is_reached_target() {
            return;
        }

        if turtle.lay_egg_counter() < 1 {
            turtle.set_laying_egg(true);
        } else if turtle.lay_egg_counter() > 200 {
            self.place_egg(mob, turtle);
        }

        if turtle.is_laying_egg() {
            turtle.increment_lay_egg_counter();
        }
    }
}
