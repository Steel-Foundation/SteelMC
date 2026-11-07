//! Vanilla `Endermite`.

use std::sync::{LazyLock, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::sound_events;
use steel_registry::vanilla_entity_data::EndermiteEntityData;
use steel_utils::locks::CheckedMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::ai::goal::{
    ClimbOnTopOfPowderSnowGoal, FloatGoal, HurtByTargetGoal, LookAtPlayerGoal, MeleeAttackGoal,
    NearestAttackableTargetGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::component::definition::MobDefinition;
use crate::entity::component::pipeline::{Stage, StageId};
use crate::entity::mob::Parts as _;
use crate::entity::monster::{self, Enemy};
use crate::entity::prelude::*;
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityBaseLoad, EntityMovementEmission, EntityProps,
    LivingEntityBase, LivingParts as _, LivingProps, MobBase, MobProps, RemovalReason, SpawnRule,
};
use crate::world::World;

/// Ticks an endermite lives unless it is persistent.
const MAX_LIFE: i32 = 2400;
const XP_REWARD: i32 = 3;

/// An endermite: a short-lived hostile mob.
#[entity_behavior(class = "Endermite", spawn_rule)]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_life, load = Self::load_life)]
pub struct EndermiteEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    living_base: LivingEntityBase,
    #[entity(component)]
    mob_base: MobBase,
    life: CheckedMutex<i32>,
    #[entity(synced)]
    entity_data: CheckedMutex<EndermiteEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EndermiteEntity`.
unsafe impl DowncastType for EndermiteEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/endermite");
}

impl Enemy for EndermiteEntity {}

impl DefaultEntityPush for EndermiteEntity {}

impl EndermiteEntity {
    /// `Endermite.checkEndermiteSpawnRules`.
    // TODO: outside spawners, vanilla also requires no player within 5
    // blocks; spawn rules get a `LevelReader`, which cannot find players.
    pub(crate) const SPAWN_RULE: SpawnRule = |entity_type, level, spawn_reason, pos, _| {
        monster::check_any_light_monster_spawn_rules(entity_type, level, spawn_reason, pos)
            && spawn_reason.is_spawner()
    };

    const LIVING: LivingProps<Self> = LivingProps {
        sound_volume: |_| 0.4,
        hurt_sound: |_, _| Some(&sound_events::ENTITY_ENDERMITE_HURT),
        death_sound: |_| Some(&sound_events::ENTITY_ENDERMITE_DEATH),
        ..monster::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |_| Some(&sound_events::ENTITY_ENDERMITE_AMBIENT),
        ..monster::mob_props()
    };
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: |endermite, entity| {
            endermite.living().set_y_body_rot(endermite.rotation().0);
            endermite.living().tick_living_entity(entity);
        },
        set_y_body_rot: |endermite, y_body_rot| {
            endermite.set_rotation((y_body_rot, endermite.rotation().1));
            endermite.living().set_y_body_rot(y_body_rot);
        },
        movement_emission: |_| EntityMovementEmission::Events,
        play_step_sound: |endermite, _, _| {
            endermite.play_sound(&sound_events::ENTITY_ENDERMITE_STEP, 0.15, 1.0);
        },
        ..monster::entity_props()
    };
    const GOALS: StageId = StageId("endermite.goals");
    const LIFE: StageId = StageId("endermite.life");

    /// Vanilla `Endermite`: the monster chains with its goals and lifetime.
    fn define() -> MobDefinition<Self> {
        let mut definition = monster::definition::<Self>();
        definition.construct =
            definition
                .construct
                .then([Stage::effect(Self::GOALS, |mob_base: &MobBase| {
                    mob_base.set_xp_reward(XP_REWARD);
                    Self::register_goals(mob_base);
                })]);
        definition.ai_step = definition
            .ai_step
            .then([Stage::effect(Self::LIFE, Self::age)]);
        definition
    }

    fn definition() -> &'static MobDefinition<Self> {
        static DEFINITION: LazyLock<MobDefinition<EndermiteEntity>> =
            LazyLock::new(EndermiteEntity::define);
        &DEFINITION
    }

    /// Creates an endermite.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs an endermite from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = EndermiteEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &Self::LIVING, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(&Self::MOB, Self::definition()),
            life: CheckedMutex::new(0),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// `Endermite.registerGoals`.
    fn register_goals(mob_base: &MobBase) {
        let mut goal_selector = mob_base.goal_selector().lock();
        goal_selector.add_goal(1, FloatGoal::new(mob_base));
        goal_selector.add_goal(1, ClimbOnTopOfPowderSnowGoal::new());
        goal_selector.add_goal(2, MeleeAttackGoal::new(1.0, false));
        goal_selector.add_goal(3, WaterAvoidingRandomStrollGoal::new(1.0));
        goal_selector.add_goal(7, LookAtPlayerGoal::new(8.0));
        goal_selector.add_goal(8, RandomLookAroundGoal::new());
        drop(goal_selector);

        let mut target_selector = mob_base.target_selector().lock();
        target_selector.add_goal(1, HurtByTargetGoal::new().set_alert_others([]));
        target_selector.add_goal(
            2,
            NearestAttackableTargetGoal::new_for_players(true, |_, _| true),
        );
    }

    /// `Endermite.aiStep` after `super`: ages unless persistent, and
    /// disappears at the end of its life.
    fn age(&self) {
        // VANILLA CLIENT-LOCAL: the portal particles.
        let expired = {
            let mut life = self.life.lock();
            if !self.mob().is_persistence_required() {
                *life += 1;
            }
            *life >= MAX_LIFE
        };
        if expired {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    fn save_life(&self, nbt: &mut NbtCompound) {
        nbt.insert("Lifetime", *self.life.lock());
    }

    fn load_life(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        *self.life.lock() = nbt.int("Lifetime").unwrap_or(0);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Arc;

    use simdnbt::borrow::read_compound;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::entity::{SharedEntity, next_entity_id};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn endermite_disappears_at_the_end_of_its_life_unless_persistent() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("endermite_lifetime");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));

        let spawn = |x| {
            let endermite = Arc::new(EndermiteEntity::new(
                &vanilla_entities::ENDERMITE,
                next_entity_id(),
                DVec3::new(x, 65.0, 8.0),
                Arc::downgrade(world),
            ));
            world
                .try_add_entity(Arc::clone(&endermite) as SharedEntity)
                .expect("test endermite should attach to the loaded chunk");
            endermite
        };

        let endermite = spawn(8.0);
        *endermite.life.lock() = MAX_LIFE - 2;
        endermite.age();
        assert!(!endermite.is_removed());
        endermite.age();
        assert!(endermite.is_removed());

        let persistent = spawn(9.0);
        persistent.mob().set_persistence_required();
        *persistent.life.lock() = MAX_LIFE - 1;
        persistent.age();
        assert!(!persistent.is_removed());
    }

    #[test]
    fn endermite_saves_and_loads_its_life() {
        init_vanilla_registry();
        let endermite =
            EndermiteEntity::new(&vanilla_entities::ENDERMITE, 1, DVec3::ZERO, Weak::new());
        *endermite.life.lock() = 1234;
        assert_eq!(endermite.mob().xp_reward(), XP_REWARD);

        let mut nbt = NbtCompound::new();
        endermite.save_additional(&mut nbt);
        assert_eq!(nbt.int("Lifetime"), Some(1234));

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("reborrow failed: {error}"));
        let loaded =
            EndermiteEntity::new(&vanilla_entities::ENDERMITE, 2, DVec3::ZERO, Weak::new());
        loaded.load_additional((&borrowed).into());
        assert_eq!(*loaded.life.lock(), 1234);
    }
}
