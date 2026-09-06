//! Vanilla behavior trait implementations for [`TurtleEntity`].
//!
//! The entity struct, its state accessors, and construction live in the parent
//! module; this file carries the `Entity` through `PathfinderMob` stack so neither
//! file grows unwieldy.

use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{sound_events, vanilla_attributes};
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId};

use super::{
    ARRIVED_DISTANCE, BABY_SCALE, CLIMB_SPEED_SHARE, DEFAULT_STEP_HEIGHT, SPEED_LERP, SWIM_DRAG,
    SWIM_PUSH, SWIM_SINK_HOME_DISTANCE, SWIM_SINK_SPEED, TurtleEntity, closer_to_center_than,
};
use crate::behavior::InteractionResult;
use crate::entity::ai::control::MoveControlOperation;
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, MOVE_CONTROL_MAX_TURN,
    Mob, MobBase, PathfinderMob, SpawnGroupData, rotlerp,
};
use crate::physics::{MoveResult, MoverType};
use crate::player::Player;
use crate::world::World;

impl Entity for TurtleEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if AgeableMob::is_baby(self) {
            self.entity_type.dimensions.scale(BABY_SCALE * scale)
        } else if self.entity_type.fixed {
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

    fn max_up_step(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::STEP_HEIGHT)
            .unwrap_or(f64::from(DEFAULT_STEP_HEIGHT)) as f32
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        let sound = if AgeableMob::is_baby(self) {
            &sound_events::ENTITY_TURTLE_SHAMBLE_BABY
        } else {
            &sound_events::ENTITY_TURTLE_SHAMBLE
        };
        self.play_sound(sound, 0.15, 1.0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        let home = self.home_pos();
        nbt.insert(
            "home_pos",
            NbtTag::IntArray(vec![home.x(), home.y(), home.z()]),
        );
        nbt.insert("has_egg", self.has_egg());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);

        if let Some(home) = nbt.int_array("home_pos")
            && home.len() == 3
        {
            self.set_home_pos(BlockPos::new(home[0], home[1], home[2]));
        } else {
            self.set_home_pos(self.block_position());
        }
        self.set_has_egg(nbt.byte("has_egg").is_some_and(|value| value != 0));
    }
}

impl LivingEntity for TurtleEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
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

    fn sound_volume(&self) -> f32 {
        0.4
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(if AgeableMob::is_baby(self) {
            &sound_events::ENTITY_TURTLE_HURT_BABY
        } else {
            &sound_events::ENTITY_TURTLE_HURT
        })
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(if AgeableMob::is_baby(self) {
            &sound_events::ENTITY_TURTLE_DEATH_BABY
        } else {
            &sound_events::ENTITY_TURTLE_DEATH
        })
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();

        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        self.tick_laying_egg();
        result
    }

    /// Vanilla `Turtle.travelInWater`: turtles swim under their own rules rather
    /// than the shared water travel, which is why they are quick in water and
    /// slow everywhere else.
    ///
    /// The push and the drag are flat values, so swimming speed does not follow
    /// the movement-speed attribute the way walking does. Vanilla also skips the
    /// shared path's fluid-falling adjustment and its jump out of water, so the
    /// gravity and surface arguments go unused here.
    fn travel_in_water(
        &self,
        input: DVec3,
        _base_gravity: f64,
        _is_falling: bool,
        _old_y: f64,
    ) -> Option<MoveResult> {
        self.move_relative(SWIM_PUSH, input);
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        let mut velocity = self.velocity() * SWIM_DRAG;

        // A turtle with somewhere to be holds its depth. One that is just
        // drifting settles slowly toward the sea floor.
        let drifting = Mob::target(self).is_none()
            && (!self.going_home()
                || !closer_to_center_than(
                    self.home_pos(),
                    self.position(),
                    SWIM_SINK_HOME_DISTANCE,
                ));
        if drifting {
            velocity.y -= SWIM_SINK_SPEED;
        }
        self.set_velocity(velocity);

        Some(result)
    }
}

impl AgeableMob for TurtleEntity {
    fn ageable_base(&self) -> &AgeableMobBase {
        &self.ageable_base
    }

    fn is_age_locked(&self) -> bool {
        *self.entity_data.lock().ageable_mob().age_locked.get()
    }

    fn set_age_locked(&self, age_locked: bool) {
        self.entity_data
            .lock()
            .ageable_mob_mut()
            .age_locked
            .set(age_locked);
    }

    fn set_synced_baby(&self, baby: bool) {
        self.entity_data.lock().ageable_mob_mut().baby.set(baby);
    }

    fn age_boundary_changed(&self, baby: bool) {
        self.refresh_dimensions();
        if !baby {
            self.drop_turtle_scute();
        }
    }
}

impl Animal for TurtleEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        TurtleEntity::is_food(item_stack)
    }

    fn can_fall_in_love(&self) -> bool {
        self.in_love_time() <= 0 && !self.has_egg()
    }
}

impl Mob for TurtleEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn custom_server_ai_step(&self) {
        Animal::custom_server_ai_step_animal(self);
    }

    /// Vanilla `Turtle.TurtleMoveControl`: a turtle steers itself rather than
    /// using the shared move control, which is what makes it lumber on land and
    /// glide in water.
    ///
    /// Three things differ from the shared one. Speed is trimmed every tick by
    /// [`Self::trim_turtle_speed`] before anything else. It eases toward its
    /// target speed instead of snapping to it, so it takes a moment to get going.
    /// And it steers until its path is finished rather than for a single tick,
    /// with no jumping, because a turtle swims over obstacles instead of hopping
    /// them.
    fn tick_move_control(&self) {
        self.trim_turtle_speed();

        let move_control = self.mob_base().controls().lock().move_control;
        let steering = matches!(move_control.operation(), MoveControlOperation::MoveTo)
            && !self.mob_base().navigation().lock().is_done();
        if !steering {
            self.set_mob_speed(0.0);
            return;
        }

        let delta = move_control.wanted_position() - self.position();
        let distance = delta.length();
        if distance < ARRIVED_DISTANCE {
            self.set_mob_speed(0.0);
            return;
        }

        let y_rot = (delta.z.atan2(delta.x) as f32).to_degrees() - 90.0;
        let (yaw, pitch) = self.rotation();
        self.set_rotation((rotlerp(yaw, y_rot, MOVE_CONTROL_MAX_TURN), pitch));

        let movement_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED);
        let target_speed = (move_control.speed_modifier() * movement_speed) as f32;
        let speed = self
            .get_speed()
            .mul_add(1.0 - SPEED_LERP, SPEED_LERP * target_speed);
        self.set_mob_speed(speed);

        // Climb or dive toward the target, since a swimming turtle cannot jump
        // its way up to one.
        let mut velocity = self.velocity();
        velocity.y += f64::from(speed) * (delta.y / distance) * CLIMB_SPEED_SHARE;
        self.set_velocity(velocity);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        (!self.is_in_water() && self.on_ground() && !AgeableMob::is_baby(self))
            .then_some(&sound_events::ENTITY_TURTLE_AMBIENT_LAND)
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.set_home_pos(self.block_position());
        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }

    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        Animal::mob_interact_animal(self, player, hand)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl PathfinderMob for TurtleEntity {}
