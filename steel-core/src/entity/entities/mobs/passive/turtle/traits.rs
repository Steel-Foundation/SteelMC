use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{sound_events, vanilla_attributes};
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId};

use super::{
    ADULT_SCALE, AMBIENT_SOUND_INTERVAL, ARRIVED_DISTANCE, BABY_SCALE, CLIMB_SPEED_SHARE,
    DEFAULT_STEP_HEIGHT, NEXT_STEP_DISTANCE, PREFERRED_WALK_TARGET_VALUE,
    SPAWN_HEIGHT_ABOVE_SEA_LEVEL, SPEED_LERP, SWIM_DRAG, SWIM_PUSH, SWIM_SINK_HOME_DISTANCE,
    SWIM_SINK_SPEED, SWIM_SOUND_VOLUME_SCALE, TURTLE_BABY_DIMENSIONS, TurtleEntity,
    closer_to_center_than,
};
use crate::behavior::InteractionResult;
use crate::behavior::blocks::vegetation::TurtleEggBlock;
use crate::entity::ai::control::MoveControlOperation;
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, MOVE_CONTROL_MAX_TURN,
    Mob, MobBase, PathfinderMob, SpawnGroupData, rotlerp,
};
use crate::fluid::FluidStateExt as _;
use crate::physics::{MoveResult, MoverType};
use crate::player::Player;
use crate::world::{LevelReader, World};

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
            TURTLE_BABY_DIMENSIONS.scale(BABY_SCALE * scale)
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

    // TODO(lightning): Implement vanilla `Turtle.thunderHit` behavior.
    fn is_pushed_by_fluid(&self) -> bool {
        false
    }

    fn next_step(&self) -> f32 {
        self.base().movement_progress().move_dist() + NEXT_STEP_DISTANCE
    }

    fn swim_sound(&self) -> SoundEventRef {
        &sound_events::ENTITY_TURTLE_SWIM
    }

    fn play_swim_sound(&self, volume: f32) {
        self.default_play_swim_sound(volume * SWIM_SOUND_VOLUME_SCALE);
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

    fn get_age_scale(&self) -> f32 {
        if AgeableMob::is_baby(self) {
            BABY_SCALE
        } else {
            ADULT_SCALE
        }
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

    fn check_animal_spawn_rules(
        level: &dyn LevelReader,
        _spawn_reason: EntitySpawnReason,
        pos: BlockPos,
    ) -> bool {
        pos.y() < level.sea_level() + SPAWN_HEIGHT_ABOVE_SEA_LEVEL
            && TurtleEggBlock::on_sand(level, pos)
            && Self::is_bright_enough_to_spawn(level, pos)
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
        let steered_yaw = rotlerp(yaw, y_rot, MOVE_CONTROL_MAX_TURN);
        self.set_rotation((steered_yaw, pitch));
        self.set_y_body_rot(steered_yaw);

        let movement_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED);
        let target_speed = (move_control.speed_modifier() * movement_speed) as f32;
        let speed = self
            .get_speed()
            .mul_add(1.0 - SPEED_LERP, SPEED_LERP * target_speed);
        self.set_mob_speed(speed);

        let mut velocity = self.velocity();
        velocity.y += f64::from(speed) * (delta.y / distance) * CLIMB_SPEED_SHARE;
        self.set_velocity(velocity);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        (!self.is_in_water() && self.on_ground() && !AgeableMob::is_baby(self))
            .then_some(&sound_events::ENTITY_TURTLE_AMBIENT_LAND)
    }

    fn ambient_sound_interval(&self) -> i32 {
        AMBIENT_SOUND_INTERVAL
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

impl PathfinderMob for TurtleEntity {
    fn get_walk_target_value(&self, pos: BlockPos) -> f32 {
        let Some(world) = self.level() else {
            return 0.0;
        };

        if !self.going_home() && world.get_block_state(pos).get_fluid_state().is_water() {
            return PREFERRED_WALK_TARGET_VALUE;
        }

        if world
            .get_block_state(pos.below())
            .get_block()
            .has_tag(&BlockTag::SAND)
        {
            PREFERRED_WALK_TARGET_VALUE
        } else {
            world.pathfinding_cost_from_light_levels(pos)
        }
    }
}
