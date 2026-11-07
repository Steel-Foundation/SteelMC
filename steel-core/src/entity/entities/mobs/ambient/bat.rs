//! Vanilla `Bat`: rests on cave ceilings and flutters through the dark.

use std::f32::consts::PI;
use std::sync::{LazyLock, Weak};

use glam::DVec3;
use rand::RngExt as _;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_math::wrap_degrees;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::BatEntityData;
use steel_registry::{level_events, sound_events};
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::chunk::heightmap::HeightmapType;
use crate::entity::SharedEntity;
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::component::definition::MobDefinition;
use crate::entity::component::pipeline::{Stage, StageId};
use crate::entity::mob;
use crate::entity::prelude::*;
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityBaseLoad, EntityMovementEmission, EntityProps,
    LivingEntityBase, LivingParts as _, LivingProps, LivingTravelInput, Mob, MobBase, MobProps,
    SpawnRule,
};
use crate::world::{World, is_redstone_conductor};

const FLAG_RESTING: i8 = 1;
const RESTING_TARGETING_RANGE: f64 = 4.0;

/// A bat.
#[entity_behavior(class = "Bat", spawn_rule)]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_flags, load = Self::load_flags)]
pub struct BatEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    living_base: LivingEntityBase,
    #[entity(component)]
    mob_base: MobBase,
    target_position: CheckedMutex<Option<BlockPos>>,
    #[entity(synced)]
    entity_data: CheckedMutex<BatEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BatEntity`.
unsafe impl DowncastType for BatEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/bat");
}

impl DefaultEntityPush for BatEntity {}

impl BatEntity {
    /// `Bat.checkBatSpawnRules`: below the surface, in the dark, on blocks
    /// bats spawn on.
    pub(crate) const SPAWN_RULE: SpawnRule = |entity_type, level, spawn_reason, pos, random| {
        if pos.y() >= level.height_at(HeightmapType::WorldSurface, pos.x(), pos.z())
            || random.random::<bool>()
            || i32::from(level.max_local_raw_brightness(pos, level.sky_darkening()))
                > random.random_range(0..4)
        {
            return false;
        }
        level
            .get_block_state(pos.below())
            .get_block()
            .has_tag(&BlockTag::BATS_SPAWNABLE_ON)
            && Mob::check_mob_spawn_rules(entity_type, level, spawn_reason, pos)
    };

    const LIVING: LivingProps<Self> = LivingProps {
        sound_volume: |_| 0.1,
        voice_pitch: |bat| bat.living().voice_pitch_default() * 0.95,
        hurt_sound: |_, _| Some(&sound_events::ENTITY_BAT_HURT),
        death_sound: |_| Some(&sound_events::ENTITY_BAT_DEATH),
        push_entities: |_| {},
        ..mob::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |bat| {
            if bat.is_resting() && rand::random_range(0..4) != 0 {
                None
            } else {
                Some(&sound_events::ENTITY_BAT_AMBIENT)
            }
        },
        // `AmbientCreature.canBeLeashed`.
        mob_can_be_leashed: |_| false,
        ..MobProps::new()
    };
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: Self::entity_tick,
        hurt: |bat, world, source, amount| {
            let living = bat.living();
            if !living.is_invulnerable_to(world, source) && bat.is_resting() {
                bat.set_resting(false);
            }
            living.hurt_server(world, source, amount)
        },
        is_hard_collision_relevant: |_| false,
        movement_emission: |_| EntityMovementEmission::Events,
        check_fall_damage: |_, _, _, _, _, _, _| {},
        is_flapping: |bat| !bat.is_resting() && bat.tick_count() % 10 == 0,
        is_pushable: |_| false,
        is_ignoring_block_triggers: |_| true,
        push_entity: |_, _| {},
        ..mob::entity_props()
    };
    const FLY: StageId = StageId("bat.fly");

    /// Vanilla `Bat`: the mob chains with its resting and flying.
    fn define() -> MobDefinition<Self> {
        let mut definition = mob::stages::definition::<Self>();
        definition.custom_server_ai_step = definition
            .custom_server_ai_step
            .then([Stage::effect(Self::FLY, Self::fly)]);
        definition
    }

    fn definition() -> &'static MobDefinition<Self> {
        static DEFINITION: LazyLock<MobDefinition<BatEntity>> = LazyLock::new(BatEntity::define);
        &DEFINITION
    }

    /// Creates a bat, resting.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a bat from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = BatEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &Self::LIVING, &mut entity_data);
        entity_data.id_flags.set(FLAG_RESTING);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(&Self::MOB, Self::definition()),
            target_position: CheckedMutex::new(None),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// Whether the bat hangs from a ceiling.
    #[must_use]
    pub fn is_resting(&self) -> bool {
        (self.entity_data.lock().id_flags.get() & FLAG_RESTING) != 0
    }

    /// Hangs the bat from the ceiling or lets it fly.
    pub fn set_resting(&self, resting: bool) {
        let mut data = self.entity_data.lock();
        let flags = *data.id_flags.get();
        data.id_flags.set(if resting {
            flags | FLAG_RESTING
        } else {
            flags & !FLAG_RESTING
        });
    }

    /// `Bat.tick`: a resting bat hangs still under its block; a flying one
    /// loses most of its vertical speed.
    fn entity_tick(&self, entity: &SharedEntity) {
        self.living().tick_living_entity(entity);
        if self.is_resting() {
            self.set_velocity(DVec3::ZERO);
            let height = f64::from(self.base().dimensions().height);
            let current = self.position();
            let hanging = DVec3::new(current.x, current.y.floor() + 1.0 - height, current.z);
            if hanging != current
                && let Err(error) = self.try_set_position(hanging)
            {
                log::debug!("bat {} failed to hang: {error}", self.id());
            }
        } else {
            let velocity = self.velocity();
            self.set_velocity(DVec3::new(velocity.x, velocity.y * 0.6, velocity.z));
        }
    }

    /// `Bat.customServerAiStep`.
    fn fly(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let pos = self.block_position();
        let above = pos.above();
        if self.is_resting() {
            self.tick_resting(&world, pos, above);
        } else {
            self.tick_flying(&world, above);
        }
    }

    /// A resting bat looks around and takes off when its ceiling is gone or
    /// a player comes close.
    fn tick_resting(&self, world: &World, pos: BlockPos, above: BlockPos) {
        if is_redstone_conductor(world, world.get_block_state(above), pos) {
            if rand::random_range(0..200) == 0 {
                self.set_y_head_rot(rand::random_range(0..360) as f32);
            }
            let conditions = TargetingConditions::for_non_combat().range(RESTING_TARGETING_RANGE);
            let living = self.living();
            let player_near = world
                .nearest_player(self.position(), RESTING_TARGETING_RANGE, |player| {
                    conditions.test(world, Some(&living), &player.living())
                })
                .is_some();
            if !player_near {
                return;
            }
        }
        self.set_resting(false);
        if !self.is_silent() {
            world.level_event(level_events::SOUND_BAT_LIFTOFF, pos, 0, None);
        }
    }

    /// A flying bat steers toward a random nearby air block and sometimes
    /// settles under a ceiling.
    fn tick_flying(&self, world: &World, above: BlockPos) {
        let mut target = *self.target_position.lock();
        if target.is_some_and(|target| {
            !world.get_block_state(target).is_air() || target.y() <= world.get_min_y()
        }) {
            target = None;
        }
        if target.is_none()
            || rand::random_range(0..30) == 0
            || target.is_some_and(|target| target.closer_to_center_than(self.position(), 2.0))
        {
            target = Some(self.random_target_position());
        }
        *self.target_position.lock() = target;
        let Some(target) = target else {
            return;
        };

        let position = self.position();
        let delta = DVec3::new(
            f64::from(target.x()) + 0.5 - position.x,
            f64::from(target.y()) + 0.1 - position.y,
            f64::from(target.z()) + 0.5 - position.z,
        );
        let movement = self.velocity();
        let new_movement = movement
            + DVec3::new(
                (delta.x.signum() * 0.5 - movement.x) * 0.1,
                (delta.y.signum() * f64::from(0.7_f32) - movement.y) * 0.1,
                (delta.z.signum() * 0.5 - movement.z) * 0.1,
            );
        self.set_velocity(new_movement);

        let y_rot = (new_movement.z.atan2(new_movement.x) * 180.0 / f64::from(PI)) as f32 - 90.0;
        let (yaw, pitch) = self.rotation();
        let rot_diff = wrap_degrees(y_rot - yaw);
        let living = self.living();
        let input = living.travel_input();
        living.set_travel_input(LivingTravelInput::new(
            input.sideways(),
            input.vertical(),
            0.5,
        ));
        self.set_rotation((yaw + rot_diff, pitch));

        if rand::random_range(0..100) == 0
            && is_redstone_conductor(world, world.get_block_state(above), above)
        {
            self.set_resting(true);
        }
    }

    fn random_target_position(&self) -> BlockPos {
        let position = self.position();
        BlockPos::containing(
            position.x + f64::from(rand::random_range(0..7) - rand::random_range(0..7)),
            position.y + f64::from(rand::random_range(0..6)) - 2.0,
            position.z + f64::from(rand::random_range(0..7) - rand::random_range(0..7)),
        )
    }

    fn save_flags(&self, nbt: &mut NbtCompound) {
        nbt.insert("BatFlags", *self.entity_data.lock().id_flags.get());
    }

    fn load_flags(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.entity_data
            .lock()
            .id_flags
            .set(nbt.byte("BatFlags").unwrap_or(0));
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;
    use crate::entity::leash::Leashable;

    #[test]
    fn bat_starts_resting_and_saves_its_flags() {
        init_vanilla_registry();
        let bat = BatEntity::new(&vanilla_entities::BAT, 1, DVec3::ZERO, Weak::new());
        assert!(bat.is_resting());
        assert!(!bat.as_leashable().is_some_and(Leashable::can_be_leashed));

        bat.set_resting(false);
        let mut nbt = NbtCompound::new();
        bat.save_additional(&mut nbt);
        assert_eq!(nbt.byte("BatFlags"), Some(0));

        let loaded = BatEntity::new(&vanilla_entities::BAT, 2, DVec3::ZERO, Weak::new());
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("reborrow failed: {error}"));
        loaded.load_additional((&borrowed).into());
        assert!(!loaded.is_resting());
    }

    #[test]
    fn bat_flaps_every_ten_ticks_while_flying() {
        init_vanilla_registry();
        let bat = BatEntity::new(&vanilla_entities::BAT, 1, DVec3::ZERO, Weak::new());
        assert!(!bat.is_flapping(), "resting bats never flap");

        bat.set_resting(false);
        assert!(bat.is_flapping());
        bat.advance_tick_count();
        assert!(!bat.is_flapping());
    }
}
