//! Enderman: teleports, reacts to being stared at, and carries blocks.

use std::iter::empty;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_math::vector::normalize;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::shapes::is_shape_full_block;
use steel_registry::data_components::PotionContents;
use steel_registry::data_components::vanilla_components::POTION_CONTENTS;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;
use steel_registry::vanilla_entity_data::EndermanEntityData;
use steel_registry::vanilla_fluid_tags::FluidTag;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_registry::vanilla_potion_tags::PotionTag;
use steel_registry::{
    level_events, sound_events, vanilla_attributes, vanilla_blocks, vanilla_entities,
    vanilla_game_events, vanilla_items,
};
use steel_utils::locks::SyncMutex;
use steel_utils::random::RandomSource;
use steel_utils::types::UpdateFlags;
use steel_utils::{
    BlockPos, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey, Identifier,
};

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, BlockLootContext};
use crate::block_entity::block_state_nbt;
use crate::enchantment_provider::{ENDERMAN_LOOT_DROP, EnchantmentProvider as _};
use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};
use crate::entity::ai::goal::{
    EndermanLookForPlayerGoal, FloatGoal, Goal, GoalControls, HurtByTargetGoal, LookAtPlayerGoal,
    MeleeAttackGoal, NearestAttackableTargetGoal, RandomLookAroundGoal,
    ResetUniversalAngerTargetGoal, WaterAvoidingRandomStrollGoal, reduced_tick_delay,
};
use crate::entity::ai::path::PathType;
use crate::entity::damage::DamageSource;
use crate::entity::leash::Leashable;
use crate::entity::projectile::ThrownPotionItem;
use crate::entity::{
    AttributeModifier, AttributeModifierOperation, DefaultEntityPush, Entity, EntityBase,
    EntityBaseLoad, EntityEventSource, EntityPose, EntitySpawnReason, EntitySyncedData,
    LivingEntity, LivingEntityBase, Mob, MobBase, Monster, NeutralMob, NeutralMobBase,
    PathfinderMob, SharedEntity, SpawnRule, sync_dirty_mob_effects,
};
use crate::fluid::get_fluid_state;
use crate::physics::MoveResult;
use crate::world::game_event::GameEventContext;
use crate::world::{ClipBlockShape, ClipFluid, World};

const SPEED_MODIFIER_ATTACKING_ID: Identifier = Identifier::vanilla_static("attacking");
const SPEED_MODIFIER_ATTACKING_AMOUNT: f64 = 0.15_f32 as f64;
const MAX_TELEPORT_DIAMETER: f64 = 64.0;
/// Random teleport Y offset drawn from `[-32, 31]`.
const TELEPORT_Y_OFFSET_MIN: i32 = -32;
const TELEPORT_Y_OFFSET_BOUND: i32 = 32;
const TELEPORT_TOWARDS_SPREAD: f64 = 8.0;
const TELEPORT_TOWARDS_DISTANCE: f64 = 16.0;
const TELEPORT_TOWARDS_Y_OFFSET_MIN: i32 = -8;
const TELEPORT_TOWARDS_Y_OFFSET_BOUND: i32 = 16;
const MIN_DEAGGRESSION_TIME: i32 = 600;
const DAYLIGHT_BRIGHTNESS_THRESHOLD: f32 = 0.5;
const DAYLIGHT_BRIGHTNESS_BASE: f32 = 0.4;
const DAYLIGHT_BRIGHTNESS_SCALE: f32 = 2.0;
const DAYLIGHT_ROLL_SCALE: f32 = 30.0;
/// Gaze tolerance used to detect a staring player.
const GAZE_CONE_SIZE: f64 = 0.025;
const FREEZE_MAX_DISTANCE_SQR: f64 = 256.0;
const LEAVE_BLOCK_DELAY: i32 = 2000;
const TAKE_BLOCK_DELAY: i32 = 20;
const XP_REWARD: i32 = 5;

/// An enderman entity.
#[entity_behavior(class = "Enderman", spawn_rule)]
pub struct EndermanEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    neutral_base: NeutralMobBase,
    entity_data: SyncMutex<EndermanEntityData>,
    target_change_time: SyncMutex<i32>,
}

// SAFETY: The owner-scoped type key uniquely identifies EndermanEntity.
unsafe impl DowncastType for EndermanEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/enderman");
}

impl EndermanEntity {
    /// Requires darkness for spawner spawns.
    pub(crate) const SPAWN_RULE: SpawnRule = Self::check_enderman_spawn_rules;

    pub(crate) fn check_enderman_spawn_rules(
        entity_type: EntityTypeRef,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
        random: &mut RandomSource,
    ) -> bool {
        <Self as Monster>::check_monster_spawn_rules(entity_type, level, spawn_reason, pos, random)
    }

    /// Creates a new enderman entity instance.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Loads a saved enderman entity.
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
        mob_base.set_xp_reward(XP_REWARD);
        {
            let mut malus = mob_base.pathfinding_malus().lock();
            malus.set(PathType::Water, -1.0);
        }

        let mut entity_data = EndermanEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, EndermanFreezeWhenLookedAt::new());
            goal_selector.add_goal(2, MeleeAttackGoal::new(1.0, false));
            goal_selector.add_goal(7, WaterAvoidingRandomStrollGoal::with_probability(1.0, 0.0));
            goal_selector.add_goal(8, LookAtPlayerGoal::new(8.0));
            goal_selector.add_goal(8, RandomLookAroundGoal::new());
            goal_selector.add_goal(10, EndermanLeaveBlockGoal::new());
            goal_selector.add_goal(11, EndermanTakeBlockGoal::new());
        }

        {
            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(1, EndermanLookForPlayerGoal::new());
            target_selector.add_goal(2, HurtByTargetGoal::new().set_alert_others(empty()));
            target_selector.add_goal(
                3,
                NearestAttackableTargetGoal::new_with_interval(10, true, false, |target, _| {
                    target.entity_type() == &vanilla_entities::ENDERMITE
                }),
            );
            target_selector.add_goal(4, ResetUniversalAngerTargetGoal::new(false));
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            neutral_base: NeutralMobBase::new(),
            entity_data: SyncMutex::new(entity_data),
            target_change_time: SyncMutex::new(0),
        }
    }

    /// Returns the block state carried by this enderman, if any.
    #[must_use]
    pub fn carried_block_state(&self) -> Option<BlockStateId> {
        *self.entity_data.lock().carry_state.get()
    }

    /// Sets or clears the carried block state.
    pub fn set_carried_block(&self, state: Option<BlockStateId>) {
        self.entity_data.lock().carry_state.set(state);
    }

    /// Returns whether this enderman is creepy (has a target).
    #[must_use]
    pub fn is_creepy(&self) -> bool {
        *self.entity_data.lock().creepy.get()
    }

    /// Returns whether a player has stared at this enderman.
    #[must_use]
    pub fn has_been_stared_at(&self) -> bool {
        *self.entity_data.lock().stared_at.get()
    }

    /// Marks this enderman as having been stared at.
    pub fn set_being_stared_at(&self) {
        self.entity_data.lock().stared_at.set(true);
    }

    fn target_change_time(&self) -> i32 {
        *self.target_change_time.lock()
    }

    /// Returns whether `entity` is a player looking at this enderman unmasked.
    #[must_use]
    pub fn is_being_stared_by(&self, entity: &dyn LivingEntity) -> bool {
        if entity.as_player().is_none() || entity.is_wearing_gaze_disguise() {
            return false;
        }
        self.is_looking_at_me(entity, GAZE_CONE_SIZE, true, false, &[self.get_eye_y()])
    }

    /// Teleports to an exact position and emits teleport effects.
    pub(crate) fn teleport(&self, x: f64, y: f64, z: f64) -> bool {
        if self.is_passenger() {
            return false;
        }
        let Some(world) = self.level() else {
            return false;
        };

        let old_pos = self.position();
        let invalid_position = |state: BlockStateId| {
            state
                .get_block()
                .has_tag(&BlockTag::ENDERMAN_DOES_NOT_TELEPORT_TO)
        };
        if !self.random_teleport_with_predicate(&world, x, y, z, true, &invalid_position) {
            return false;
        }

        world.game_event_at(
            &vanilla_game_events::TELEPORT,
            old_pos,
            &GameEventContext::new(Some(self.as_entity_event_source()), None),
        );
        if !self.is_silent() {
            let old_block_pos = BlockPos::containing(old_pos.x, old_pos.y, old_pos.z);
            let packed = clamped_pack_difference_in_position(
                old_block_pos,
                self.block_position(),
                POSITION_PACK_RADIUS,
                POSITION_PACK_RADIUS,
                POSITION_PACK_RADIUS,
            );
            world.level_event(
                level_events::PARTICLES_ENDERMAN_TELEPORT,
                old_block_pos,
                packed,
                None,
            );
            let position = self.position();
            world.play_sound_at(
                &sound_events::ENTITY_ENDERMAN_TELEPORT,
                self.sound_source(),
                position,
                1.0,
                1.0,
                None,
            );
            self.play_sound(&sound_events::ENTITY_ENDERMAN_TELEPORT, 1.0, 1.0);
        }
        true
    }

    /// Teleports to a random position within the teleport diameter.
    pub(crate) fn teleport_randomly(&self) -> bool {
        if !LivingEntity::is_alive(self) {
            return false;
        }
        let position = self.position();
        let x = position.x + (rand::random::<f64>() - 0.5) * MAX_TELEPORT_DIAMETER;
        let y = position.y
            + f64::from(rand::random_range(
                TELEPORT_Y_OFFSET_MIN..TELEPORT_Y_OFFSET_BOUND,
            ));
        let z = position.z + (rand::random::<f64>() - 0.5) * MAX_TELEPORT_DIAMETER;
        self.teleport(x, y, z)
    }

    /// Teleports away from `target`, biased in the opposite direction.
    pub(crate) fn teleport_towards(&self, target: &dyn Entity) -> bool {
        let position = self.position();
        let target_position = target.position();
        let from_y = position.y + f64::from(self.base().dimensions().height) * 0.5;
        let direction = normalize(DVec3::new(
            position.x - target_position.x,
            from_y - target.get_eye_y(),
            position.z - target_position.z,
        ));
        let x = position.x + (rand::random::<f64>() - 0.5) * TELEPORT_TOWARDS_SPREAD
            - direction.x * TELEPORT_TOWARDS_DISTANCE;
        let y = position.y
            + f64::from(rand::random_range(
                TELEPORT_TOWARDS_Y_OFFSET_MIN..TELEPORT_TOWARDS_Y_OFFSET_BOUND,
            ))
            - direction.y * TELEPORT_TOWARDS_DISTANCE;
        let z = position.z + (rand::random::<f64>() - 0.5) * TELEPORT_TOWARDS_SPREAD
            - direction.z * TELEPORT_TOWARDS_DISTANCE;
        self.teleport(x, y, z)
    }

    fn repeatedly_try_to_teleport(&self) {
        if self.is_passenger() {
            return;
        }
        for _ in 0..MAX_TELEPORT_ATTEMPTS {
            if self.teleport_randomly() {
                return;
            }
        }
    }

    fn hurt_with_clean_water(
        &self,
        world: &World,
        source: &DamageSource,
        potion_item: ItemStack,
        damage: f32,
    ) -> bool {
        let contents = potion_item.get_or_default(POTION_CONTENTS, PotionContents::empty());
        contents.is_in_tag(&PotionTag::HURTS_WATER_SENSITIVE_ENTITIES)
            && LivingEntity::default_hurt_server(self, world, source, damage)
    }
}

impl DefaultEntityPush for EndermanEntity {}

impl Entity for EndermanEntity {
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
        sync_dirty_mob_effects(&self.living_base, &self.entity_data);
    }

    fn max_up_step(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::STEP_HEIGHT)
            .unwrap_or(f64::from(DEFAULT_STEP_HEIGHT)) as f32
    }

    fn sound_source(&self) -> SoundSource {
        Monster::sound_source_monster(self)
    }

    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        Monster::fall_sounds_monster(self)
    }

    fn swim_sound(&self) -> SoundEventRef {
        Monster::swim_sound_monster(self)
    }

    fn swim_splash_sound(&self) -> SoundEventRef {
        Monster::swim_splash_sound_monster(self)
    }

    fn projectile_receives_side_effects_on_hit(&self, was_hurt: bool) -> bool {
        was_hurt
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        if let Some(state) = self.carried_block_state() {
            nbt.insert(
                CARRIED_BLOCK_STATE_KEY,
                NbtTag::Compound(block_state_nbt::save(state)),
            );
        }
        self.add_persistent_anger_save_data(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.set_carried_block(
            nbt.compound(CARRIED_BLOCK_STATE_KEY)
                .and_then(block_state_nbt::load)
                .filter(|state| !state.is_air()),
        );
        self.read_persistent_anger_save_data(&nbt);
    }
}

impl LivingEntity for EndermanEntity {
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
        1.0
    }

    fn can_randomly_teleport_to(&self, x: f64, y: f64, z: f64) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let below = BlockPos::containing(x, y, z).below();
        !get_fluid_state(&world, below)
            .fluid_id
            .has_tag(&FluidTag::WATER)
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDERMAN_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDERMAN_DEATH)
    }

    fn should_drop_experience(&self) -> bool {
        Monster::should_drop_experience_monster(self)
    }

    fn should_drop_loot(&self, world: &World) -> bool {
        Monster::should_drop_loot_monster(self, world)
    }

    fn ai_step(&self, entity: &SharedEntity) -> Option<MoveResult> {
        if let Some(world) = self.level() {
            self.update_persistent_anger(world.as_ref(), true);
        }
        Monster::monster_ai_step(self, entity)
    }

    fn server_ai_step(&self, entity: &SharedEntity) {
        Mob::mob_server_ai_step(self, entity);
    }

    fn drop_custom_death_loot(&self, _source: &DamageSource, _killed_by_player: bool) {
        let Some(carried) = self.carried_block_state() else {
            return;
        };
        let Some(world) = self.level() else {
            return;
        };

        let mut tool = ItemStack::new(&vanilla_items::DIAMOND_AXE);
        {
            let mut random = world.loot_random.lock();
            ENDERMAN_LOOT_DROP.enchant(&mut tool, &mut random);
        }
        let context = BlockLootContext::new(&world, self.block_position())
            .with_tool(&tool)
            .with_entity(Some(self.as_entity_event_source()))
            .with_origin(self.position());
        for item in context.get_drops(carried) {
            self.spawn_at_location(item, 0.0);
        }
    }

    fn hurt_server(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.is_invulnerable_to(world, source) {
            return false;
        }

        let potion_item = source
            .direct_entity()
            .and_then(|entity| entity.as_thrown_potion_item())
            .map(ThrownPotionItem::thrown_potion_item);

        if source.is(&DamageTypeTag::IS_PROJECTILE) && !self.is_passenger() {
            self.repeatedly_try_to_teleport();
            return false;
        }

        if let Some(potion_item) = potion_item {
            let result = self.hurt_with_clean_water(world, source, potion_item, amount);
            self.repeatedly_try_to_teleport();
            return result;
        }

        let result = LivingEntity::default_hurt_server(self, world, source, amount);
        if source
            .causing_entity()
            .and_then(|entity| entity.as_living_entity())
            .is_none()
            && rand::random_range(0..TELEPORT_CHANCE_BOUND) != 0
        {
            let _ = self.teleport_randomly();
        }
        result
    }
}

impl Mob for EndermanEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }

    fn tick_goal_selectors(&self, entity: &SharedEntity) {
        PathfinderMob::tick_pathfinder_goal_selectors(self, entity);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        if self.is_creepy() {
            Some(&sound_events::ENTITY_ENDERMAN_SCREAM)
        } else {
            Some(&sound_events::ENTITY_ENDERMAN_AMBIENT)
        }
    }

    fn requires_custom_persistence(&self) -> bool {
        self.is_passenger() || self.is_leashed() || self.carried_block_state().is_some()
    }

    fn custom_server_ai_step(&self) {
        let Some(world) = self.level() else {
            return;
        };
        if world.is_bright_outside()
            && self.tick_count() >= self.target_change_time() + MIN_DEAGGRESSION_TIME
        {
            let brightness = self.get_light_level_dependent_magic_value();
            if brightness > DAYLIGHT_BRIGHTNESS_THRESHOLD
                && world.can_see_sky(self.block_position())
                && rand::random::<f32>() * DAYLIGHT_ROLL_SCALE
                    < (brightness - DAYLIGHT_BRIGHTNESS_BASE) * DAYLIGHT_BRIGHTNESS_SCALE
            {
                self.set_target(None);
                let _ = self.teleport_randomly();
            }
        }
    }

    fn set_target(&self, target: Option<&SharedEntity>) -> bool {
        let accepted = self
            .mob_base()
            .set_target(target, |target| self.is_valid_target(target));
        if self.target_unchecked().is_some() {
            *self.target_change_time.lock() = self.tick_count();
            self.entity_data.lock().creepy.set(true);
            let mut attributes = self.attributes().lock();
            if !attributes.has_modifier(
                vanilla_attributes::MOVEMENT_SPEED,
                &SPEED_MODIFIER_ATTACKING_ID,
            ) {
                attributes.add_modifier(
                    vanilla_attributes::MOVEMENT_SPEED,
                    AttributeModifier {
                        id: SPEED_MODIFIER_ATTACKING_ID,
                        amount: SPEED_MODIFIER_ATTACKING_AMOUNT,
                        operation: AttributeModifierOperation::AddValue,
                    },
                    false,
                );
            }
        } else {
            *self.target_change_time.lock() = 0;
            {
                let mut data = self.entity_data.lock();
                data.creepy.set(false);
                data.stared_at.set(false);
            }
            self.attributes().lock().remove_modifier(
                vanilla_attributes::MOVEMENT_SPEED,
                &SPEED_MODIFIER_ATTACKING_ID,
            );
        }
        accepted
    }
}

impl NeutralMob for EndermanEntity {
    fn neutral_mob_base(&self) -> &NeutralMobBase {
        &self.neutral_base
    }
}

impl Monster for EndermanEntity {}

impl PathfinderMob for EndermanEntity {
    fn get_walk_target_value(&self, _pos: BlockPos) -> f32 {
        0.0
    }
}

const CARRIED_BLOCK_STATE_KEY: &str = "carriedBlockState";
const MAX_TELEPORT_ATTEMPTS: i32 = 64;
const TELEPORT_CHANCE_BOUND: i32 = 10;
const POSITION_PACK_RADIUS: i32 = 127;
const DEFAULT_STEP_HEIGHT: f32 = 0.6;

/// Packs block offsets into three 8-bit channels for teleport particles.
fn clamped_pack_difference_in_position(
    pos: BlockPos,
    test_pos: BlockPos,
    x_radius: i32,
    y_radius: i32,
    z_radius: i32,
) -> i32 {
    let x_diff = (test_pos.x() - pos.x()).clamp(-127, 127);
    let y_diff = (test_pos.y() - pos.y()).clamp(-127, 127);
    let z_diff = (test_pos.z() - pos.z()).clamp(-127, 127);
    ((x_radius.clamp(0, 127) + x_diff) & 0xFF) << 16
        | ((y_radius.clamp(0, 127) + y_diff) & 0xFF) << 8
        | ((z_radius.clamp(0, 127) + z_diff) & 0xFF)
}

struct EndermanFreezeWhenLookedAt;

impl EndermanFreezeWhenLookedAt {
    const fn new() -> Self {
        Self
    }
}

impl Goal for EndermanFreezeWhenLookedAt {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::JUMP
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(enderman) = mob.downcast_ref::<EndermanEntity>() else {
            return false;
        };
        let Some(target) = mob.target() else {
            return false;
        };
        let Some(player) = target.as_player() else {
            return false;
        };
        target.position().distance_squared(mob.position()) <= FREEZE_MAX_DISTANCE_SQR
            && enderman.is_being_stared_by(player)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        mob.mob_base().navigation().lock().stop();
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, _entity: &SharedEntity) {
        let Some(target) = mob.target() else {
            return;
        };
        let position = target.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(position.x, target.get_eye_y(), position.z),
            DEFAULT_LOOK_Y_MAX_ROT_SPEED,
            DEFAULT_LOOK_X_MAX_ROT_ANGLE,
        );
    }
}

struct EndermanLeaveBlockGoal;

impl EndermanLeaveBlockGoal {
    const fn new() -> Self {
        Self
    }

    fn can_place_block(
        world: &World,
        pos: BlockPos,
        carried: BlockStateId,
        target_state: BlockStateId,
        below_state: BlockStateId,
        below: BlockPos,
        mob: &dyn Entity,
    ) -> bool {
        if !target_state.is_air() || below_state.is_air() {
            return false;
        }
        if below_state.get_block() == &vanilla_blocks::BEDROCK {
            return false;
        }

        let below_behavior = BLOCK_BEHAVIORS.get_behavior(below_state.get_block());
        let below_shape = below_behavior.get_collision_shape(
            below_state,
            world,
            below,
            BlockCollisionContext::empty(),
        );
        if !is_shape_full_block(below_shape) {
            return false;
        }

        let carried_behavior = BLOCK_BEHAVIORS.get_behavior(carried.get_block());
        carried_behavior.can_survive(carried, world, pos)
            && world
                .get_entities_in_aabb_excluding(&steel_utils::WorldAabb::single(pos.0), mob, |_| {
                    true
                })
                .is_empty()
    }
}

impl Goal for EndermanLeaveBlockGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(enderman) = mob.downcast_ref::<EndermanEntity>() else {
            return false;
        };
        if enderman.carried_block_state().is_none() {
            return false;
        }
        let Some(world) = mob.level() else {
            return false;
        };
        world.get_game_rule(&MOB_GRIEFING)
            && rand::random_range(0..reduced_tick_delay(LEAVE_BLOCK_DELAY)) == 0
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, _entity: &SharedEntity) {
        let Some(enderman) = mob.downcast_ref::<EndermanEntity>() else {
            return;
        };
        let Some(carried) = enderman.carried_block_state() else {
            return;
        };
        let Some(world) = mob.level() else {
            return;
        };
        let position = mob.position();
        let x = (position.x - 1.0 + rand::random::<f64>() * 2.0).floor() as i32;
        let y = (position.y + rand::random::<f64>() * 2.0).floor() as i32;
        let z = (position.z - 1.0 + rand::random::<f64>() * 2.0).floor() as i32;
        let pos = BlockPos::new(x, y, z);
        let target_state = world.get_block_state(pos);
        let below = pos.below();
        let below_state = world.get_block_state(below);
        let carried = world.update_from_neighbor_shapes(carried, pos);

        if Self::can_place_block(
            world.as_ref(),
            pos,
            carried,
            target_state,
            below_state,
            below,
            mob.as_entity_event_source(),
        ) {
            world.set_block(pos, carried, UpdateFlags::UPDATE_ALL);
            world.game_event(
                &vanilla_game_events::BLOCK_PLACE,
                pos,
                &GameEventContext::new(Some(mob.as_entity_event_source()), Some(carried)),
            );
            enderman.set_carried_block(None);
        }
    }
}

struct EndermanTakeBlockGoal;

impl EndermanTakeBlockGoal {
    const fn new() -> Self {
        Self
    }
}

impl Goal for EndermanTakeBlockGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(enderman) = mob.downcast_ref::<EndermanEntity>() else {
            return false;
        };
        if enderman.carried_block_state().is_some() {
            return false;
        }
        let Some(world) = mob.level() else {
            return false;
        };
        world.get_game_rule(&MOB_GRIEFING)
            && rand::random_range(0..reduced_tick_delay(TAKE_BLOCK_DELAY)) == 0
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, _entity: &SharedEntity) {
        let Some(enderman) = mob.downcast_ref::<EndermanEntity>() else {
            return;
        };
        let Some(world) = mob.level() else {
            return;
        };
        let position = mob.position();
        let x = (position.x - 2.0 + rand::random::<f64>() * 4.0).floor() as i32;
        let y = (position.y + rand::random::<f64>() * 3.0).floor() as i32;
        let z = (position.z - 2.0 + rand::random::<f64>() * 4.0).floor() as i32;
        let pos = BlockPos::new(x, y, z);
        let state = world.get_block_state(pos);
        let block_pos = mob.block_position();
        let from = DVec3::new(
            f64::from(block_pos.x()) + 0.5,
            f64::from(y) + 0.5,
            f64::from(block_pos.z()) + 0.5,
        );
        let to = DVec3::new(f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5);
        let reached = world
            .clip(from, to, ClipBlockShape::Outline, ClipFluid::None)
            .block_pos
            == pos;

        if state.get_block().has_tag(&BlockTag::ENDERMAN_HOLDABLE) && reached {
            world.remove_block(pos, false);
            world.game_event(
                &vanilla_game_events::BLOCK_DESTROY,
                pos,
                &GameEventContext::new(Some(mob.as_entity_event_source()), Some(state)),
            );
            enderman.set_carried_block(Some(state.get_block().default_state()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use simdnbt::borrow::read_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::{
        init_vanilla_registry, vanilla_attributes, vanilla_blocks, vanilla_entities,
    };
    use steel_utils::random::RandomSource;
    use steel_utils::random::legacy_random::LegacyRandom;
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::entities::PigEntity;
    use crate::entity::next_entity_id;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn set_target_tracks_creepy_state_and_attacking_speed() {
        init_vanilla_registry();
        let enderman =
            EndermanEntity::new(&vanilla_entities::ENDERMAN, 1, DVec3::ZERO, Weak::new());
        let target: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(2.0, 0.0, 0.0),
            Weak::new(),
        ));

        assert!(enderman.set_target(Some(&target)));
        assert!(enderman.is_creepy());
        assert!(enderman.attributes().lock().has_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            &SPEED_MODIFIER_ATTACKING_ID
        ));

        assert!(enderman.set_target(None));
        assert!(!enderman.is_creepy());
        assert!(!enderman.has_been_stared_at());
        assert!(!enderman.attributes().lock().has_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            &SPEED_MODIFIER_ATTACKING_ID
        ));
    }

    #[test]
    fn carried_block_and_anger_survive_nbt_round_trip() {
        init_vanilla_registry();
        let enderman =
            EndermanEntity::new(&vanilla_entities::ENDERMAN, 1, DVec3::ZERO, Weak::new());
        let carried = vanilla_blocks::STONE.default_state();
        enderman.set_carried_block(Some(carried));
        enderman.set_persistent_anger_end_time(1234);

        let mut nbt = NbtCompound::new();
        enderman.save_additional(&mut nbt);
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes)).expect("nbt should reborrow");

        let loaded = EndermanEntity::new(&vanilla_entities::ENDERMAN, 2, DVec3::ZERO, Weak::new());
        loaded.load_additional((&borrowed).into());
        assert_eq!(loaded.carried_block_state(), Some(carried));
        assert_eq!(loaded.persistent_anger_end_time(), 1234);
    }

    #[test]
    fn teleport_rejects_blocks_in_the_avoidance_tag() {
        init_vanilla_registry();
        init_behaviors();
        let fixture = fresh_test_world("enderman_teleport_avoidance");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        for x in [8, 9] {
            assert!(world.set_block(
                BlockPos::new(x, 63, 8),
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_ALL,
            ));
        }

        let enderman = Arc::new(EndermanEntity::new(
            &vanilla_entities::ENDERMAN,
            next_entity_id(),
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(world),
        ));

        let obstacle = BlockPos::new(9, 64, 8);
        assert!(world.set_block(
            obstacle,
            vanilla_blocks::CACTUS.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(!enderman.teleport(9.5, 64.0, 8.5));
        assert_eq!(enderman.position(), DVec3::new(8.5, 64.0, 8.5));

        assert!(world.set_block(
            obstacle,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(enderman.teleport(9.5, 64.0, 8.5));
        assert_eq!(enderman.position(), DVec3::new(9.5, 64.0, 8.5));
    }

    #[test]
    fn enderman_spawn_rule_requires_darkness_for_spawners() {
        init_vanilla_registry();
        let fixture = fresh_test_world("enderman_spawn_darkness");
        let pos = BlockPos::new(8, 65, 8);
        let mut dark_roll = RandomSource::Legacy(LegacyRandom::from_seed(5_120));
        let mut light_exempt_roll = RandomSource::Legacy(LegacyRandom::from_seed(5_120));

        assert!(!EndermanEntity::check_enderman_spawn_rules(
            &vanilla_entities::ENDERMAN,
            &fixture.world,
            EntitySpawnReason::Spawner,
            pos,
            &mut dark_roll,
        ));
        assert!(EndermanEntity::check_enderman_spawn_rules(
            &vanilla_entities::ENDERMAN,
            &fixture.world,
            EntitySpawnReason::TrialSpawner,
            pos,
            &mut light_exempt_roll,
        ));
    }

    #[test]
    fn persistent_anger_timer_expires() {
        init_vanilla_registry();
        let fixture = fresh_test_world("enderman_anger_timer");
        let world = &fixture.world;
        let enderman = EndermanEntity::new(
            &vanilla_entities::ENDERMAN,
            1,
            DVec3::ZERO,
            Arc::downgrade(world),
        );

        enderman.set_time_to_remain_angry(10);
        assert!(enderman.is_angry());
        enderman.stop_being_angry();
        assert!(!enderman.is_angry());
        assert_eq!(enderman.persistent_anger_end_time(), -1);
    }
}
