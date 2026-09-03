// Adapted from Pumpkin (GPL-3.0): https://github.com/Snowiiii/Pumpkin
//! Enderman entity implementation

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::fluid::FluidStateExt as _;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::sound_events;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;
use steel_registry::vanilla_entity_data::EndermanEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt as _, vanilla_attributes, vanilla_damage_types, vanilla_entities,
    vanilla_game_events,
};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey, Identifier};

use std::sync::{Arc, Weak};

use crate::behavior::BlockCollisionContext;
use crate::entity::ai::goal::{
    FloatGoal, Goal, GoalControls, HurtByTargetGoal, LookAtPlayerGoal, MeleeAttackGoal,
    NearestAttackableTargetGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::entity::damage::DamageSource;
use crate::entity::is_in_rain;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason, EntitySyncedData,
    LivingEntity, LivingEntityBase, LivingEntitySyncedData, Mob, MobBase, MoveResult,
    PathfinderMob, SharedEntity, SpawnGroupData,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::{CollisionWorld as _, WorldCollisionProvider};
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::player_spawn_finder::aabb_contains_any_liquid;
use crate::world::{LevelReader as _, World};

/// Vanilla `EnderMan.SPEED_MODIFIER_ATTACKING_ID`.
const ATTACKING_SPEED_MODIFIER_ID: &str = "attacking";
/// Vanilla `EnderMan.SPEED_MODIFIER_ATTACKING` amount, added to movement speed.
const ATTACKING_SPEED_MODIFIER_AMOUNT: f64 = 0.15;
/// Vanilla `EnderMan.MIN_DEAGGRESSION_TIME`.
const MIN_DEAGGRESSION_TIME: i32 = 600;
/// Vanilla `EnderMan.EndermanFreezeWhenLookedAt` range, squared.
const FREEZE_RANGE_SQR: f64 = 256.0;
/// Vanilla `EnderMan.hurtServer` projectile dodge attempts.
const PROJECTILE_DODGE_ATTEMPTS: usize = 64;
/// Vanilla `LivingEntity.isLookingAtMe` gaze threshold.
const GAZE_THRESHOLD: f64 = 0.025;
/// Vanilla `Mob.getHeadRotSpeed` / `Mob.getMaxHeadXRot`, used by
/// `LookControl.setLookAt` when no explicit limits are given.
const DEFAULT_HEAD_ROT_SPEED: f32 = 10.0;
const DEFAULT_MAX_HEAD_X_ROT: f32 = 40.0;

/// Vanilla enderman entity.
#[entity_behavior(class = "EnderMan")]
pub struct EndermanEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    /// Vanilla `EnderMan.targetChangeTime`.
    target_change_time: SyncMutex<i32>,
    entity_data: SyncMutex<EndermanEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EndermanEntity`.
unsafe impl DowncastType for EndermanEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/enderman");
}

/// Returns vanilla `EnderMan.isBeingStaredBy` — the player is looking straight
/// into the enderman's eyes. Shared by the enderman and its freeze goal.
fn is_being_stared_by(mob: &dyn PathfinderMob, player: &Player) -> bool {
    // Vanilla `LivingEntity.PLAYER_NOT_WEARING_DISGUISE_ITEM` (carved pumpkin).
    let mut wearing_disguise = false;
    player.with_equipment_slot(EquipmentSlot::Head, &mut |item_stack| {
        wearing_disguise = REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::GAZE_DISGUISE_EQUIPMENT);
    });
    if wearing_disguise {
        return false;
    }

    // Vanilla `LivingEntity.isLookingAtMe`.
    let view = player.look_angle().normalize();
    let player_position = player.position();
    let mob_position = mob.position();
    let to_enderman = DVec3::new(
        mob_position.x - player_position.x,
        mob.get_eye_y() - player.get_eye_y(),
        mob_position.z - player_position.z,
    );
    let distance = to_enderman.length();
    if distance <= 0.0 {
        return false;
    }

    let dot = view.dot(to_enderman / distance);
    dot > 1.0 - GAZE_THRESHOLD / distance && player.has_line_of_sight(mob)
}

/// Vanilla `EnderMan.EndermanFreezeWhenLookedAt`: while the current target is a
/// nearby player staring into its eyes, the enderman stops and stares back
/// instead of charging.
///
/// Claiming `MOVE` is what actually freezes it — the goal selector stops
/// `MeleeAttackGoal` while this goal runs.
struct EndermanFreezeWhenLookedAtGoal {
    target: Option<SharedEntity>,
}

impl Goal for EndermanFreezeWhenLookedAtGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::JUMP | GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(target) = mob.target() else {
            return false;
        };
        let Some(player) = target.as_player() else {
            return false;
        };
        if mob.position().distance_squared(target.position()) > FREEZE_RANGE_SQR {
            return false;
        }

        self.target = is_being_stared_by(mob, player).then_some(target);
        self.target.is_some()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        mob.mob_base().navigation().lock().stop();
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(target) = &self.target else {
            return;
        };
        let position = target.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(position.x, target.get_eye_y(), position.z),
            DEFAULT_HEAD_ROT_SPEED,
            DEFAULT_MAX_HEAD_X_ROT,
        );
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.target = None;
    }
}

impl EndermanEntity {
    /// Creates a new enderman entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates an enderman entity from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        let mob_base = MobBase::new();
        let mut entity_data = EndermanEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, EndermanFreezeWhenLookedAtGoal { target: None });
            goal_selector.add_goal(2, MeleeAttackGoal::new(1.0, false));
            goal_selector.add_goal(7, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(8, LookAtPlayerGoal::new(8.0));
            goal_selector.add_goal(8, RandomLookAroundGoal::new());
            // TODO: Add enderman block pickup/place goals (EndermanTakeBlockGoal,
            // EndermanLeaveBlockGoal) once carried blocks are supported.
        }

        {
            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(1, HurtByTargetGoal::new());
            target_selector.add_goal(
                2,
                NearestAttackableTargetGoal::new(true, |target, _world| {
                    target.entity_type() == &vanilla_entities::ENDERMITE
                }),
            );
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            target_change_time: SyncMutex::new(0),
            entity_data: SyncMutex::new(entity_data),
        }
    }

    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }

        let display = self.living_base.mob_effect_display_state();

        {
            let mut entity_data = self.entity_data.lock();
            let living = entity_data.living_entity_mut();
            living.effect_particles.set(display.particles);
            living.effect_ambience.set(display.ambient);
        }

        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }

    /// Marks the enderman as being stared at, mirroring vanilla
    /// `EnderMan.setBeingStaredAt` plus the angry-face flag and scream.
    fn set_being_stared_at(&self) {
        {
            let mut entity_data = self.entity_data.lock();
            entity_data.ender_man_mut().creepy.set(true);
            entity_data.ender_man_mut().stared_at.set(true);
        }
        self.play_sound(&sound_events::ENTITY_ENDERMAN_SCREAM, 1.0, 1.0);
    }

    /// Vanilla `EnderMan.teleport()`: teleport to a random nearby position.
    fn teleport(&self) -> bool {
        if !Entity::is_alive(self) {
            return false;
        }

        let position = self.position();
        let x = position.x + (rand::random::<f64>() - 0.5) * 64.0;
        let y = position.y + f64::from(rand::random_range(0..64) - 32);
        let z = position.z + (rand::random::<f64>() - 0.5) * 64.0;
        self.teleport_to(x, y, z)
    }

    /// Vanilla `EnderMan.teleport(x, y, z)`: the landing block must be
    /// motion-blocking and dry before attempting the actual teleport.
    fn teleport_to(&self, x: f64, y: f64, z: f64) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        let min_y = world.get_min_y();
        let mut pos = BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);
        while pos.y() > min_y && !world.get_block_state(pos).blocks_motion() {
            pos = pos.below();
        }

        let landing_state = world.get_block_state(pos);
        if !landing_state.blocks_motion() || landing_state.get_fluid_state().is_water() {
            return false;
        }

        let old_position = self.position();
        if !self.random_teleport(x, y, z) {
            return false;
        }

        world.game_event(
            &vanilla_game_events::TELEPORT,
            BlockPos::from(old_position),
            &GameEventContext::new(Some(self), None),
        );
        if !self.is_silent() {
            // Vanilla plays the sound once at the old position and once at the
            // new one (`EnderMan.teleport`, ~293).
            world.play_sound_at(
                &sound_events::ENTITY_ENDERMAN_TELEPORT,
                self.sound_source(),
                old_position,
                1.0,
                1.0,
                None,
            );
            self.play_sound(&sound_events::ENTITY_ENDERMAN_TELEPORT, 1.0, 1.0);
        }
        true
    }

    /// Vanilla `LivingEntity.randomTeleport()`: land on a solid block, verify
    /// the destination is clear, and roll back when it is not.
    fn random_teleport(&self, x: f64, mut y: f64, z: f64) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        let old_position = self.position();
        let min_y = world.get_min_y();
        let mut pos = BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);

        let mut landed = false;
        while !landed && pos.y() > min_y {
            let below = pos.below();
            if world.get_block_state(below).blocks_motion() {
                landed = true;
            } else {
                y -= 1.0;
                pos = below;
            }
        }

        let mut ok = false;
        if landed && self.try_set_position(DVec3::new(x, y, z)).is_ok() {
            ok = self.has_teleport_landing_space(&world);
        }

        if !ok {
            let _ = self.try_set_position(old_position);
            return false;
        }

        self.mob_base.navigation().lock().stop();
        true
    }

    /// Returns whether the destination bounding box is free of block and entity
    /// collision and does not contain liquid (vanilla `Entity.noCollision` plus
    /// `Level.containsAnyLiquid`).
    fn has_teleport_landing_space(&self, world: &Arc<World>) -> bool {
        let aabb = self.bounding_box();
        let collision_world = WorldCollisionProvider::for_entity(world, self);
        !collision_world.has_entity_collision(&aabb)
            && !collision_world
                .has_block_collision_with_context(&aabb, BlockCollisionContext::empty())
            && !aabb_contains_any_liquid(world, aabb)
    }
}

impl Entity for EndermanEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        // Vanilla `EnderMan.hurtServer` projectile branch: endermen dodge
        // projectiles instead of taking the hit.
        // TODO: Vanilla only reaches this branch for non-potion projectiles —
        // a clean-water splash potion still hurts. Steel has no thrown-potion
        // entity yet, so that rule cannot be mirrored.
        if source.is(&DamageTypeTag::IS_PROJECTILE) {
            return (0..PROJECTILE_DODGE_ATTEMPTS).any(|_| self.teleport());
        }

        let result = LivingEntity::hurt_server(self, world, source, amount);

        // Vanilla `EnderMan.hurtServer`: for non-projectile damage from a
        // non-living source (e.g. drowning), teleport away 9 times out of 10.
        let cause_is_living = source.causing_entity_id.is_some_and(|id| {
            world
                .get_entity_by_id(id)
                .is_some_and(|entity| entity.is_living_entity())
        });
        if !cause_is_living && rand::random_range(0..10) != 0 {
            self.teleport();
        }

        result
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        // TODO: Save carried block state
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        // TODO: Load carried block state
    }
}

impl LivingEntity for EndermanEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn living_synced_data(&self) -> Option<&dyn LivingEntitySyncedData> {
        Some(&self.entity_data)
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data
            .lock()
            .living_entity_mut()
            .health
            .set(clamped);
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDERMAN_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDERMAN_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();
        // TODO: Dodge incoming projectiles.
        result
    }
}

impl Mob for EndermanEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    /// Vanilla `EnderMan.setTarget`: any target turns the enderman creepy and
    /// grants the attacking speed boost; clearing it resets both.
    fn set_target(&self, target: Option<&SharedEntity>) -> bool {
        let set = self
            .mob_base()
            .set_target(target, |target| self.is_valid_target(target));
        let modifier_id = Identifier::vanilla_static(ATTACKING_SPEED_MODIFIER_ID);

        if set && target.is_some() {
            *self.target_change_time.lock() = self.tick_count();
            self.entity_data.lock().ender_man_mut().creepy.set(true);
            // `add_modifier` is a no-op when the modifier is already present,
            // which is vanilla's `hasModifier` guard.
            self.attributes().lock().add_modifier(
                vanilla_attributes::MOVEMENT_SPEED,
                AttributeModifier {
                    id: modifier_id,
                    amount: ATTACKING_SPEED_MODIFIER_AMOUNT,
                    operation: AttributeModifierOperation::AddValue,
                },
                false,
            );
        } else {
            *self.target_change_time.lock() = 0;
            {
                let mut entity_data = self.entity_data.lock();
                entity_data.ender_man_mut().creepy.set(false);
                entity_data.ender_man_mut().stared_at.set(false);
            }
            self.attributes()
                .lock()
                .remove_modifier(vanilla_attributes::MOVEMENT_SPEED, &modifier_id);
        }

        set
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        if *self.entity_data.lock().ender_man().creepy.get() {
            Some(&sound_events::ENTITY_ENDERMAN_SCREAM)
        } else {
            Some(&sound_events::ENTITY_ENDERMAN_AMBIENT)
        }
    }

    fn custom_server_ai_step(&self) {
        let Some(world) = self.level() else {
            return;
        };

        // Vanilla `EnderMan.customServerAiStep`: endermen abandon their target
        // and flee daylight once they have held it long enough.
        if world.is_bright_outside()
            && self.tick_count() >= *self.target_change_time.lock() + MIN_DEAGGRESSION_TIME
        {
            let block_position = self.block_position();
            let brightness = world.light_level_dependent_magic_value(block_position);
            if brightness > 0.5
                && world.can_see_sky(block_position)
                && f64::from(rand::random::<f32>()) * 30.0 < f64::from(brightness - 0.4) * 2.0
            {
                self.set_target(None);
                self.teleport();
            }
        }

        // Vanilla `EnderMan.customServerAiStep`: drown damage in water or rain.
        if self.is_in_water() || is_in_rain(self) {
            self.hurt(
                world.as_ref(),
                &DamageSource::environment(&vanilla_damage_types::DROWN),
                1.0,
            );
        }

        // Vanilla `EnderMan.EndermanLookForPlayerGoal`: anger when stared at.
        if self.target().is_none() {
            let follow_range = self
                .attributes()
                .lock()
                .required_value(vanilla_attributes::FOLLOW_RANGE);
            let position = self.position();
            let eye_origin = DVec3::new(position.x, self.get_eye_y(), position.z);
            if let Some(player) = world.nearest_player(eye_origin, follow_range, |player| {
                self.is_valid_target(player) && is_being_stared_by(self, player)
            }) {
                self.set_being_stared_at();
                let target: SharedEntity = player;
                self.set_target(Some(&target));
            } else {
                // Calm down while nobody is staring (vanilla resets these flags
                // when the target is cleared).
                self.set_target(None);
            }
        }
    }

    fn finalize_spawn(
        &self,
        world: &std::sync::Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_mob_base(world, spawn_reason, group_data)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl PathfinderMob for EndermanEntity {}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::*;

    fn spawn(id: i32) -> EndermanEntity {
        init_vanilla_registry();
        EndermanEntity::new(&vanilla_entities::ENDERMAN, id, DVec3::ZERO, Weak::new())
    }

    fn movement_speed(enderman: &EndermanEntity) -> f64 {
        enderman
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED)
    }

    #[test]
    fn enderman_target_grants_the_vanilla_attacking_speed_boost() {
        let enderman = spawn(1);
        let calm_speed = movement_speed(&enderman);

        let target: SharedEntity = Arc::new(spawn(2));
        assert!(Mob::set_target(&enderman, Some(&target)));
        assert!(*enderman.entity_data.lock().ender_man().creepy.get());
        let angry_speed = movement_speed(&enderman);
        assert!((angry_speed - calm_speed - ATTACKING_SPEED_MODIFIER_AMOUNT).abs() < 1.0e-9);

        assert!(Mob::set_target(&enderman, None));
        assert!(!*enderman.entity_data.lock().ender_man().creepy.get());
        assert!((movement_speed(&enderman) - calm_speed).abs() < 1.0e-9);
    }

    #[test]
    fn enderman_freeze_goal_claims_move_and_jump() {
        let goal = EndermanFreezeWhenLookedAtGoal { target: None };

        assert_eq!(goal.controls(), GoalControls::JUMP | GoalControls::MOVE);
        assert!(goal.requires_update_every_tick());
    }
}
