//! Vanilla `Blaze`.

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::sound_events;
use steel_registry::vanilla_entity_data::BlazeEntityData;
use steel_utils::locks::SyncMutex;

use crate::entity::ai::goal::{
    BlazeAttackGoal, HurtByTargetGoal, LookAtPlayerGoal, MoveTowardsRestrictionGoal,
    NearestAttackableTargetGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::ai::path::PathType;
use crate::entity::mob;
use crate::entity::monster::{self, Enemy};
use crate::entity::prelude::*;
use crate::entity::projectile::triangle_random;
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityProps, EntityTable, LivingEntityBase, LivingProps,
    LivingTable, MobBase, MobProps, MobTable, SpawnRule,
};
use crate::physics::MoveResult;

const FALL_SPEED_MULTIPLIER: f64 = 0.6;
const LOOK_AT_PLAYER_RANGE: f64 = 8.0;
const XP_REWARD: i32 = 10;
const ALLOWED_HEIGHT_OFFSET_INITIAL: f32 = 0.5;
/// Triangle mode of the random allowed height offset.
const ALLOWED_HEIGHT_OFFSET_MODE: f64 = 0.5;
const ALLOWED_HEIGHT_OFFSET_DEVIATION: f64 = 6.891;
const ALLOWED_HEIGHT_OFFSET_CHANGE_INTERVAL: i32 = 100;
/// Vanilla `0.3F` literals, widened the way Java widens floats inside the
/// double expression.
const RISE_IMPULSE_TARGET_Y: f64 = 0.3_f32 as f64;
const RISE_IMPULSE_SCALE: f64 = 0.3_f32 as f64;
const WATER_MALUS: f32 = -1.0;
const LAVA_MALUS: f32 = 8.0;
const FIRE_MALUS: f32 = 0.0;
/// Blazes never choose a water-friendly stroll target.
const STROLL_WATER_PROBABILITY: f32 = 0.0;
/// Charged flag bit in the synced `flags` byte.
const CHARGED_FLAG: i8 = 1;
/// A blaze always renders at full brightness.
const LIGHT_LEVEL_DEPENDENT_MAGIC_VALUE: f32 = 1.0;

/// How far above its own eyes a blaze lets its target be before rising.
struct HeightOffset {
    allowed: f32,
    next_change_tick: i32,
}

/// A blaze.
#[entity_behavior(class = "Blaze", spawn_rule)]
#[derive(Entity)]
#[entity(key = "steel:entity/blaze", constructors)]
pub struct BlazeEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    living_base: LivingEntityBase,
    #[entity(component)]
    mob_base: MobBase,
    height_offset: SyncMutex<HeightOffset>,
    #[entity(synced)]
    entity_data: SyncMutex<BlazeEntityData>,
}

impl Enemy for BlazeEntity {}

impl DefaultEntityPush for BlazeEntity {}

impl EntityTable for BlazeEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        // `Blaze.isOnFire` reports the charged state instead of fire ticks.
        is_on_fire: Self::is_charged,
        light_level_dependent_magic_value: |_| LIGHT_LEVEL_DEPENDENT_MAGIC_VALUE,
        ..monster::entity_props()
    };
}

impl BlazeEntity {
    pub(crate) const SPAWN_RULE: SpawnRule = |entity_type, level, spawn_reason, pos, _| {
        monster::check_any_light_monster_spawn_rules(entity_type, level, spawn_reason, pos)
    };

    /// `Blaze`'s constructor chain: `Monster`'s, then its own malus, reward
    /// and goals.
    fn construct(mob_base: &MobBase) {
        monster::construct(mob_base);
        {
            let mut malus = mob_base.pathfinding_malus().lock();
            malus.set(PathType::Water, WATER_MALUS);
            malus.set(PathType::Lava, LAVA_MALUS);
            malus.set(PathType::FireInNeighbor, FIRE_MALUS);
            malus.set(PathType::Fire, FIRE_MALUS);
        }
        mob_base.set_xp_reward(XP_REWARD);
        Self::register_goals(mob_base);
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = BlazeEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(Self::construct),
            height_offset: SyncMutex::new(HeightOffset {
                allowed: ALLOWED_HEIGHT_OFFSET_INITIAL,
                next_change_tick: 0,
            }),
            entity_data: SyncMutex::new(entity_data),
        }
    }

    fn register_goals(mob_base: &MobBase) {
        let goal_selector = mob_base.goal_selector();
        goal_selector.add_goal(4, BlazeAttackGoal::new());
        goal_selector.add_goal(5, MoveTowardsRestrictionGoal::new(1.0));
        goal_selector.add_goal(
            7,
            WaterAvoidingRandomStrollGoal::with_probability(1.0, STROLL_WATER_PROBABILITY),
        );
        goal_selector.add_goal(8, LookAtPlayerGoal::new(LOOK_AT_PLAYER_RANGE));
        goal_selector.add_goal(8, RandomLookAroundGoal::new());

        let target_selector = mob_base.target_selector();
        target_selector.add_goal(1, HurtByTargetGoal::new().set_alert_others([]));
        target_selector.add_goal(
            2,
            NearestAttackableTargetGoal::new_for_players(true, |_, _| true),
        );
    }

    /// Whether this blaze is charged and readying a fireball volley.
    #[must_use]
    pub fn is_charged(&self) -> bool {
        *self.entity_data.lock().flags.get() & CHARGED_FLAG != 0
    }

    /// Sets the charged flag in synced data.
    pub fn set_charged(&self, charged: bool) {
        let mut data = self.entity_data.lock();
        let flags = *data.flags.get();
        data.flags.set(if charged {
            flags | CHARGED_FLAG
        } else {
            flags & !CHARGED_FLAG
        });
    }

    /// Rolls a new allowed height offset every
    /// [`ALLOWED_HEIGHT_OFFSET_CHANGE_INTERVAL`] ticks.
    fn tick_height_offset(&self) {
        let mut height_offset = self.height_offset.lock();
        height_offset.next_change_tick -= 1;
        if height_offset.next_change_tick > 0 {
            return;
        }

        height_offset.next_change_tick = ALLOWED_HEIGHT_OFFSET_CHANGE_INTERVAL;
        height_offset.allowed =
            triangle_random(ALLOWED_HEIGHT_OFFSET_MODE, ALLOWED_HEIGHT_OFFSET_DEVIATION) as f32;
    }

    /// Floats upward while the target is above the allowed height offset.
    fn apply_rise_impulse(&self) {
        let mob = self.mob();
        let Some(target) = mob.target() else {
            return;
        };
        let Some(target_living) = target.as_living_entity() else {
            return;
        };
        let allowed_height_offset = self.height_offset.lock().allowed;
        if target.eye_y() <= self.eye_y() + f64::from(allowed_height_offset)
            || !mob.can_attack(&target_living)
        {
            return;
        }
        let velocity = self.velocity();
        let impulse = (RISE_IMPULSE_TARGET_Y - velocity.y) * RISE_IMPULSE_SCALE;
        self.set_velocity(velocity + DVec3::new(0.0, impulse, 0.0));
        self.mark_velocity_sync();
    }
}

impl MobTable for BlazeEntity {
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |_| Some(&sound_events::ENTITY_BLAZE_AMBIENT),
        custom_server_ai_step: Self::mob_custom_server_ai_step,
        ..monster::mob_props()
    };
}

impl BlazeEntity {
    fn mob_custom_server_ai_step(&self) {
        self.tick_height_offset();
        self.apply_rise_impulse();
        mob::custom_server_ai_step(self);
    }
}

impl LivingTable for BlazeEntity {
    const LIVING: LivingProps<Self> = LivingProps {
        hurt_sound: |_, _| Some(&sound_events::ENTITY_BLAZE_HURT),
        death_sound: |_| Some(&sound_events::ENTITY_BLAZE_DEATH),
        ai_step: Self::living_ai_step,
        ..monster::living_props()
    };
}

impl BlazeEntity {
    /// `Blaze.aiStep` slows its fall before `super`.
    fn living_ai_step(&self) -> Option<MoveResult> {
        let velocity = self.velocity();
        if !self.on_ground() && velocity.y < 0.0 {
            self.set_velocity(velocity * DVec3::new(1.0, FALL_SPEED_MULTIPLIER, 1.0));
        }
        // VANILLA CLIENT-LOCAL: the burn sound and smoke particles.
        monster::ai_step(self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::next_entity_id;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn water_damages_the_blaze_on_its_ai_step() {
        init_vanilla_registry();
        init_behaviors();
        let fixture = fresh_test_world("blaze_water_damage");
        let world = &fixture.world;
        let position = DVec3::new(8.5, 64.0, 8.5);
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(8, 64, 8),
            vanilla_blocks::WATER.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let blaze = BlazeEntity::new(
            &vanilla_entities::BLAZE,
            next_entity_id(),
            position,
            Arc::downgrade(world),
        );

        assert!(blaze.update_fluid_interaction());
        assert!(blaze.is_in_water());
        let _ = blaze.living().ai_step();

        assert_eq!(blaze.living().health(), 19.0);
    }
}
