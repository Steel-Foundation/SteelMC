use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_registry::{sound_events, vanilla_blocks, vanilla_custom_stats, vanilla_game_events};
use steel_utils::types::UpdateFlags;

use super::{TurtleEntity, as_turtle, closer_to_center_than};
use crate::behavior::blocks::vegetation::TurtleEggBlock;
use crate::entity::ai::goal::{BreedGoal, Goal, GoalControls, MoveToBlockGoal};
use crate::entity::entities::ExperienceOrbEntity;
use crate::entity::{AgeableMob, Animal, PathfinderMob};
use crate::world::game_event::GameEventContext;

const EGGS: &IntProperty = &BlockStateProperties::EGGS;
const POST_BREED_AGE: i32 = 6000;
const LAY_EGG_SEARCH_RANGE: i32 = 16;
const LAY_EGG_HOME_RANGE: f64 = 9.0;
const LAY_EGG_DURATION: i32 = 200;
const POST_LAY_LOVE_TIME: i32 = 600;
const MAX_EGGS_LAID: u8 = 4;
const LAY_EGG_SOUND_VOLUME: f32 = 0.3;
const LAY_EGG_PITCH_BASE: f32 = 0.9;
const LAY_EGG_PITCH_SPREAD: f32 = 0.2;

/// Breeding gives the mother an egg to lay instead of spawning a baby,
/// and both parents age back to adulthood.
pub(crate) struct TurtleBreedGoal {
    inner: BreedGoal,
}

impl TurtleBreedGoal {
    pub(crate) const fn new(speed_modifier: f64) -> Self {
        Self {
            inner: BreedGoal::new(speed_modifier).with_breed(Self::breed),
        }
    }

    fn breed(mob: &dyn PathfinderMob, partner_animal: &dyn Animal) {
        let (Some(turtle), Some(world)) = (as_turtle(mob), mob.level()) else {
            return;
        };

        if let Some(love_cause) = turtle
            .love_cause_uuid()
            .or_else(|| partner_animal.love_cause_uuid())
            && let Some(player) = world.players.get_by_uuid(&love_cause)
        {
            player.award_custom_stat(&vanilla_custom_stats::ANIMALS_BRED);
            // TODO(advancements): trigger the BRED_ANIMALS criterion once Steel
            // has an advancement / criteria-trigger system.
        }

        turtle.set_has_egg(true);
        turtle.set_age(POST_BREED_AGE);
        partner_animal.set_age(POST_BREED_AGE);
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
        self.inner.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_use(mob) && as_turtle(mob).is_some_and(|turtle| !turtle.has_egg())
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.inner.can_continue_to_use(mob)
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.inner.stop(mob);
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.inner.tick(mob);
    }
}

/// Walks to sand near home and, after a delay, places a turtle egg cluster.
pub(crate) struct TurtleLayEggGoal {
    inner: MoveToBlockGoal,
}

impl TurtleLayEggGoal {
    pub(crate) fn new(speed_modifier: f64) -> Self {
        Self {
            inner: MoveToBlockGoal::new(speed_modifier, LAY_EGG_SEARCH_RANGE, |level, pos| {
                level.get_block_state(pos.above()).is_air() && TurtleEggBlock::is_sand(level, pos)
            }),
        }
    }

    fn within_home(turtle: &TurtleEntity, mob: &dyn PathfinderMob) -> bool {
        closer_to_center_than(turtle.home_pos(), mob.position(), LAY_EGG_HOME_RANGE)
    }

    fn place_egg(&self, mob: &dyn PathfinderMob, turtle: &TurtleEntity) {
        let Some(world) = mob.level() else {
            return;
        };

        let egg_pos = self.inner.block_pos().above();
        let count = rand::random_range(1..=MAX_EGGS_LAID);
        let egg_state = vanilla_blocks::TURTLE_EGG
            .default_state()
            .set_value(EGGS, count);

        world.play_sound(
            &sound_events::ENTITY_TURTLE_LAY_EGG,
            SoundSource::Blocks,
            mob.block_position(),
            LAY_EGG_SOUND_VOLUME,
            LAY_EGG_PITCH_BASE + rand::random::<f32>() * LAY_EGG_PITCH_SPREAD,
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
        turtle.set_in_love_time(POST_LAY_LOVE_TIME);
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
        } else if turtle.lay_egg_counter() > LAY_EGG_DURATION {
            self.place_egg(mob, turtle);
        }

        if turtle.is_laying_egg() {
            turtle.increment_lay_egg_counter();
        }
    }
}
