//! Vanilla `Zombie` family behavior shared by zombies, husks, and drowned.
//!
//! Vanilla models the family with inheritance: `Zombie` holds the baby state,
//! door breaking, drowning conversion, spawn bonuses, and equipment rolls, while
//! `Husk` and `Drowned` only tweak sounds, dimensions, and goals. Rust has no
//! class inheritance, so [`Zombie`] is a capability trait with the shared
//! defaults and each variant is a thin concrete struct.
//!
//! TODO(drowned): `Drowned` is not implemented yet. That variant still needs
//! `RangedAttackMob` and its ranged goal, trident item use, amphibious
//! navigation, drowned spawn rules and the `MORE_FREQUENT_DROWNED_SPAWNS` biome
//! tag, the ZombifiedPiglin alert for `HurtByTargetGoal`, and the
//! ZombieNautilus jockey.

use std::iter::empty;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::level_events;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::sound_events;
use steel_registry::vanilla_attributes;
use steel_registry::vanilla_entity_data::{HuskEntityData, ZombieEntityData};
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_entities, vanilla_items};
use steel_utils::locks::{MappedMutexGuard, MutexGuard, SyncMutex};
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, BlockStateId, Identifier};

use crate::entity::ai::goal::{
    BreakDoorGoal, GoalId, HurtByTargetGoal, LookAtPlayerGoal, NearestAttackableTargetGoal,
    RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::entity::conversion::{ConversionParams, ConversionStatus, ConversionTracker};
use crate::entity::damage::DamageSource;
use crate::entity::mob::{
    RANDOM_SPAWN_BONUS_ID, do_hurt_target_default, populate_default_equipment_slots_default,
};
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityEventSource, EntityPose, EntitySpawnReason,
    EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase, MobEffectInstance, Monster,
    PathfinderMob, SharedEntity, SpawnGroupData, ZombieGroupData,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::player::Player;
use crate::special_dates;
use crate::world::{DifficultyInstance, World};

/// Step height Zombie uses when the attribute is somehow missing.
const DEFAULT_STEP_HEIGHT: f32 = 0.6;
const XP_REWARD: i32 = 5;
const BABY_XP_MULTIPLIER: f64 = 2.5;
const BABY_SPAWN_CHANCE: f32 = 0.05;
const BABY_WIDTH: f32 = 0.49;
const BABY_HEIGHT: f32 = 0.98;
const BABY_VEHICLE_ATTACHMENT: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.1875, 0.0)];
const BABY_SPEED_MODIFIER_ID: Identifier = Identifier::vanilla_static("baby");
const BABY_SPEED_MODIFIER_AMOUNT: f64 = 0.5;
const ZOMBIE_ATTACK_SPEED_MODIFIER: f64 = 1.0;
const ZOMBIE_RAISE_ARM_TICKS_BEFORE_ATTACK: i32 = 5;
const PICKUP_LOOT_CHANCE: f32 = 0.55;
const ZOMBIE_RANDOM_SPAWN_BONUS_ID: Identifier =
    Identifier::vanilla_static("zombie_random_spawn_bonus");
const KNOCKBACK_RESISTANCE_SPAWN_BONUS_MAX: f64 = 0.05;
const FOLLOW_RANGE_SPAWN_BONUS_SCALE: f64 = 1.5;
const LEADER_ZOMBIE_BONUS_ID: Identifier = Identifier::vanilla_static("leader_zombie_bonus");
const LEADER_SPAWN_CHANCE: f32 = 0.05;
const LEADER_REINFORCEMENT_BONUS_MIN: f64 = 0.5;
const LEADER_REINFORCEMENT_BONUS_RANGE: f64 = 0.25;
const LEADER_HEALTH_BONUS_MIN: f64 = 1.0;
const LEADER_HEALTH_BONUS_RANGE: f64 = 3.0;
const HARD_IRON_EQUIPMENT_CHANCE: f32 = 0.05;
const OTHER_IRON_EQUIPMENT_CHANCE: f32 = 0.01;
const IRON_EQUIPMENT_OPTIONS: i32 = 6;
const HALLOWEEN_PUMPKIN_CHANCE: f32 = 0.25;
const HALLOWEEN_JACK_O_LANTERN_CHANCE: f32 = 0.1;
const DOOR_BREAK_CHANCE: f32 = 0.1;
const FIRE_ATTACK_CHANCE_PER_DIFFICULTY: f32 = 0.3;
const IN_WATER_TIME: i32 = 600;
const DROWNED_CONVERSION_TIME: i32 = 300;

/// Vanilla `Zombie.BABY_DIMENSIONS`, which differs only in eye height per variant.
const fn baby_dimensions(eye_height: f32) -> EntityDimensions {
    EntityDimensions::new_with_attachments(
        BABY_WIDTH,
        BABY_HEIGHT,
        eye_height,
        EntityAttachments::new(&[], &BABY_VEHICLE_ATTACHMENT, &[], &[]),
    )
}

/// State owned by the single `BreakDoorGoal` a zombie adds while it can break doors.
#[derive(Debug, Default)]
pub struct DoorBreakingState {
    can_break_doors: bool,
    goal_id: Option<GoalId>,
}

/// Vanilla `Zombie.getSpawnAsBabyOdds`.
pub(crate) fn spawn_as_baby_odds() -> bool {
    rand::random::<f32>() < BABY_SPAWN_CHANCE
}

/// Registers vanilla `Zombie.registerGoals` and `Zombie.addBehaviourGoals`.
///
/// `Drowned` replaces the behavior goals, so it only uses the shared look goals
/// from its own registration.
pub(crate) fn register_zombie_goals(mob_base: &MobBase) {
    let mut goal_selector = mob_base.goal_selector().lock();
    // TODO(turtle_egg): ZombieAttackTurtleEggGoal needs RemoveBlockGoal.
    goal_selector.add_goal(8, LookAtPlayerGoal::new(8.0));
    goal_selector.add_goal(8, RandomLookAroundGoal::new());
    // TODO(spear): SpearUseGoal is not implemented yet.
    goal_selector.add_goal(
        3,
        ZombieAttackGoal::new(ZOMBIE_ATTACK_SPEED_MODIFIER, false),
    );
    // TODO(village): MoveThroughVillageGoal needs the village/POI manager.
    goal_selector.add_goal(7, WaterAvoidingRandomStrollGoal::new(1.0));
    drop(goal_selector);

    let mut target_selector = mob_base.target_selector().lock();
    target_selector.add_goal(1, HurtByTargetGoal::new().set_alert_others(empty()));
    target_selector.add_goal(
        2,
        NearestAttackableTargetGoal::new_for_players(true, |_, _| true),
    );
    // TODO(targets): villager, iron golem, and baby turtle target goals need those
    // entities to exist first.
}

/// Vanilla `Zombie`, shared by every concrete variant.
///
/// Object safe: [`crate::entity::Entity::as_zombie`] exposes the converted mob
/// during drowning conversion. Methods that need `Self: Sized` are limited to
/// [`Zombie::tick_drowning_conversion`].
pub trait Zombie: Monster {
    /// Locks and narrows this variant's entity data to the `Zombie` layer.
    fn zombie_data(&self) -> MappedMutexGuard<'_, ZombieEntityData>;

    /// Returns this variant's root synchronized entity data.
    fn zombie_synced_data(&self) -> &dyn EntitySyncedData;

    /// Returns the door-breaking state shared with the `BreakDoorGoal`.
    fn door_breaking(&self) -> &SyncMutex<DoorBreakingState>;

    /// Returns the drowning conversion timers.
    fn conversion_tracker(&self) -> &SyncMutex<ConversionTracker>;

    /// Vanilla `BABY_DIMENSIONS`, which each variant declares separately because
    /// only the eye height differs.
    fn baby_dimensions(&self) -> EntityDimensions;

    /// Idle sound for this variant.
    fn ambient_sound_zombie(&self) -> SoundEventRef {
        &sound_events::ENTITY_ZOMBIE_AMBIENT
    }

    /// Hurt sound for this variant.
    fn hurt_sound_zombie(&self, _source: &DamageSource) -> SoundEventRef {
        &sound_events::ENTITY_ZOMBIE_HURT
    }

    /// Death sound for this variant.
    fn death_sound_zombie(&self) -> SoundEventRef {
        &sound_events::ENTITY_ZOMBIE_DEATH
    }

    /// Footstep sound for this variant.
    fn step_sound_zombie(&self) -> SoundEventRef {
        &sound_events::ENTITY_ZOMBIE_STEP
    }

    /// Swimming sound for this variant.
    fn swim_sound_zombie(&self) -> SoundEventRef {
        Monster::swim_sound_monster(self)
    }

    /// Whether this variant starts drowning conversion while its eyes are in water.
    fn converts_in_water(&self) -> bool {
        true
    }

    /// Entity type this variant turns into after drowning.
    fn converts_to_when_drowning(&self) -> EntityTypeRef {
        &vanilla_entities::DROWNED
    }

    /// Level event played when the drowning conversion finishes.
    fn conversion_level_event(&self) -> i32 {
        level_events::SOUND_ZOMBIE_TO_DROWNED
    }

    /// Returns the synced baby flag.
    fn is_baby_zombie(&self) -> bool {
        *self.zombie_data().baby.get()
    }

    /// Sets the baby flag, refreshing dimensions and the baby speed modifier.
    fn set_baby_zombie(&self, baby: bool) {
        self.zombie_data().baby.set(baby);

        if self.level().is_some() {
            let mut attributes = self.attributes().lock();
            attributes.remove_modifier(vanilla_attributes::MOVEMENT_SPEED, &BABY_SPEED_MODIFIER_ID);
            if baby {
                attributes.add_modifier(
                    vanilla_attributes::MOVEMENT_SPEED,
                    AttributeModifier {
                        id: BABY_SPEED_MODIFIER_ID,
                        amount: BABY_SPEED_MODIFIER_AMOUNT,
                        operation: AttributeModifierOperation::AddMultipliedBase,
                    },
                    false,
                );
            }
        }

        self.refresh_dimensions();
    }

    /// Returns whether this zombie may break wooden doors.
    fn can_break_doors(&self) -> bool {
        self.door_breaking().lock().can_break_doors
    }

    /// Sets door breaking and keeps the navigation and goal selector in sync.
    fn set_can_break_doors(&self, can_break_doors: bool) {
        // TODO(navigation): vanilla skips this when the navigation cannot
        // navigate ground; `PathNavigation` has no `canNavigateGround` yet.
        let removed_goal = {
            let mut state = self.door_breaking().lock();
            if state.can_break_doors == can_break_doors {
                return;
            }
            state.can_break_doors = can_break_doors;
            if can_break_doors {
                None
            } else {
                state.goal_id.take()
            }
        };

        self.mob_base()
            .navigation()
            .lock()
            .set_can_open_doors(can_break_doors);

        let mut goal_selector = self.mob_base().goal_selector().lock();
        if can_break_doors {
            let goal_id = goal_selector.add_goal(
                1,
                BreakDoorGoal::new(|difficulty| difficulty == Difficulty::Hard),
            );
            self.door_breaking().lock().goal_id = Some(goal_id);
        } else if let Some(goal_id) = removed_goal {
            let Some(pathfinder) = Entity::as_pathfinder_mob(self) else {
                return;
            };
            goal_selector.remove_goal(pathfinder, goal_id);
        }
    }

    /// Vanilla `Zombie.handleAttributes`.
    fn handle_attributes(&self, difficulty_modifier: f32, spawn_reason: EntitySpawnReason) {
        let mut attributes = self.attributes().lock();
        attributes.set_base_value(
            vanilla_attributes::SPAWN_REINFORCEMENTS,
            rand::random::<f64>() * 0.1,
        );
        attributes.set_modifier(
            vanilla_attributes::KNOCKBACK_RESISTANCE,
            AttributeModifier {
                id: RANDOM_SPAWN_BONUS_ID,
                amount: rand::random::<f64>() * KNOCKBACK_RESISTANCE_SPAWN_BONUS_MAX,
                operation: AttributeModifierOperation::AddValue,
            },
            true,
        );

        let follow_range_modifier =
            rand::random::<f64>() * FOLLOW_RANGE_SPAWN_BONUS_SCALE * f64::from(difficulty_modifier);
        if follow_range_modifier > 1.0 {
            attributes.set_modifier(
                vanilla_attributes::FOLLOW_RANGE,
                AttributeModifier {
                    id: ZOMBIE_RANDOM_SPAWN_BONUS_ID,
                    amount: follow_range_modifier,
                    operation: AttributeModifierOperation::AddMultipliedTotal,
                },
                true,
            );
        }

        if rand::random::<f32>() >= difficulty_modifier * LEADER_SPAWN_CHANCE {
            return;
        }

        attributes.set_modifier(
            vanilla_attributes::SPAWN_REINFORCEMENTS,
            AttributeModifier {
                id: LEADER_ZOMBIE_BONUS_ID,
                amount: rand::random::<f64>() * LEADER_REINFORCEMENT_BONUS_RANGE
                    + LEADER_REINFORCEMENT_BONUS_MIN,
                operation: AttributeModifierOperation::AddValue,
            },
            true,
        );
        attributes.set_modifier(
            vanilla_attributes::MAX_HEALTH,
            AttributeModifier {
                id: LEADER_ZOMBIE_BONUS_ID,
                amount: rand::random::<f64>() * LEADER_HEALTH_BONUS_RANGE + LEADER_HEALTH_BONUS_MIN,
                operation: AttributeModifierOperation::AddMultipliedTotal,
            },
            true,
        );
        drop(attributes);

        if !matches!(
            spawn_reason,
            EntitySpawnReason::Conversion
                | EntitySpawnReason::Load
                | EntitySpawnReason::DimensionTravel
        ) {
            self.set_health(self.get_max_health());
        }

        self.set_can_break_doors(true);
    }

    /// Mirrors the dirty mob-effect display state into synced entity data.
    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base().take_effects_dirty() {
            return;
        }

        let display = self.living_base().mob_effect_display_state();
        {
            let mut entity_data = self.zombie_data();
            let living = entity_data.living_entity_mut();
            living.effect_particles.set(display.particles);
            living.effect_ambience.set(display.ambient);
        }

        self.zombie_synced_data()
            .set_base_invisible_flag(display.invisible);
    }

    /// Vanilla `Zombie.tick` runs the shared mob tick first and then the
    /// drowning tracker, which may replace this mob before the tick ends.
    fn tick_drowning_conversion(&self)
    where
        Self: Sized,
    {
        if !self.converts_in_water() {
            return;
        }

        let Some(world) = self.level() else {
            return;
        };
        if !Entity::is_alive(self) || self.is_no_ai() {
            return;
        }

        let afflicted = self.is_eye_in_water();
        let status = {
            let mut tracker = self.conversion_tracker().lock();
            let status = tracker.tick(afflicted);
            let converting = tracker.is_converting();
            drop(tracker);
            let mut data = self.zombie_data();
            if *data.drowned_conversion.get() != converting {
                data.drowned_conversion.set(converting);
            }
            status
        };

        if status != ConversionStatus::Converted {
            return;
        }

        let converts_to = self.converts_to_when_drowning();
        let conversion_level_event = self.conversion_level_event();
        let converted_level = Arc::clone(&world);
        let after_conversion = move |converted: &dyn Mob| {
            let difficulty_modifier = converted_level
                .get_current_difficulty_at(converted.block_position())
                .special_multiplier();
            if let Some(zombie) = converted.as_zombie() {
                zombie.handle_attributes(difficulty_modifier, EntitySpawnReason::Conversion);
            }
            if !converted.is_silent() {
                converted_level.level_event(
                    conversion_level_event,
                    converted.block_position(),
                    0,
                    None,
                );
            }
        };

        self.convert_to(
            converts_to,
            ConversionParams::single(true, true),
            Some(&after_conversion),
        );
    }

    /// Vanilla `Zombie.doHurtTarget` fire ignition, shared with `Husk`.
    fn do_hurt_target_zombie(
        &self,
        entity: &SharedEntity,
        world: &World,
        target: &SharedEntity,
    ) -> bool {
        self.do_hurt_target_zombie_default(entity, world, target)
    }

    /// Shared `Zombie.doHurtTarget` body before variant additions.
    fn do_hurt_target_zombie_default(
        &self,
        entity: &SharedEntity,
        world: &World,
        target: &SharedEntity,
    ) -> bool {
        if !do_hurt_target_default(self, entity, world, target) {
            return false;
        }

        let difficulty = world
            .get_current_difficulty_at(self.block_position())
            .effective_difficulty();
        if self.equipment_in_slot(EquipmentSlot::MainHand).is_empty()
            && self.is_on_fire()
            && rand::random::<f32>() < difficulty * FIRE_ATTACK_CHANCE_PER_DIFFICULTY
        {
            target.ignite_for_ticks(2 * (difficulty as i32) * 20);
        }

        true
    }

    /// Whether this zombie may pick up the item, rejecting eggs for mounted babies.
    fn can_hold_item_zombie(&self, item_stack: &ItemStack) -> bool {
        !(self.is_baby_zombie()
            && self.is_passenger()
            && REGISTRY.items.is_in_tag(item_stack.item(), &ItemTag::EGGS))
    }

    /// Vanilla `Drowned.okTarget`, which every other zombie accepts unconditionally.
    fn ok_target(&self, _target: Option<&dyn LivingEntity>) -> bool {
        true
    }

    /// Whether this zombie wants to pick up the item.
    fn wants_to_pick_up_zombie(&self, item_stack: &ItemStack) -> bool {
        !item_stack.is(&vanilla_items::GLOW_INK_SAC) && self.can_hold_item_zombie(item_stack)
    }

    /// Vanilla `Zombie.populateDefaultEquipmentSlots`.
    fn populate_default_equipment_slots_zombie(&self, difficulty: DifficultyInstance) {
        populate_default_equipment_slots_default(self, difficulty);

        let iron_chance = if difficulty.difficulty() == Difficulty::Hard {
            HARD_IRON_EQUIPMENT_CHANCE
        } else {
            OTHER_IRON_EQUIPMENT_CHANCE
        };
        if rand::random::<f32>() >= iron_chance {
            return;
        }

        let item: ItemRef = match rand::random_range(0..IRON_EQUIPMENT_OPTIONS) {
            0 => &vanilla_items::IRON_SWORD,
            1 => &vanilla_items::IRON_SPEAR,
            _ => &vanilla_items::IRON_SHOVEL,
        };
        self.living_base()
            .equipment()
            .lock()
            .set(EquipmentSlot::MainHand, ItemStack::new(item));
    }

    /// Vanilla `Zombie.finalizeSpawn`, shared up to the variant-specific tail.
    fn finalize_spawn_zombie(
        &self,
        world: &Arc<World>,
        difficulty: DifficultyInstance,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_zombie_default(world, difficulty, spawn_reason, group_data)
    }

    /// Shared `Zombie.finalizeSpawn` body before variant additions.
    fn finalize_spawn_zombie_default(
        &self,
        world: &Arc<World>,
        difficulty: DifficultyInstance,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let group_data = self.finalize_spawn_mob_base(world, difficulty, spawn_reason, group_data);
        let difficulty_modifier = difficulty.special_multiplier();

        if spawn_reason != EntitySpawnReason::Conversion {
            self.set_can_pick_up_loot(
                rand::random::<f32>() < PICKUP_LOOT_CHANCE * difficulty_modifier,
            );
        }

        let group_data = match group_data {
            Some(SpawnGroupData::Zombie(group_data)) => Some(SpawnGroupData::Zombie(group_data)),
            None => Some(SpawnGroupData::Zombie(ZombieGroupData::new(
                spawn_as_baby_odds(),
                true,
            ))),
            group_data => group_data,
        };

        if let Some(SpawnGroupData::Zombie(group_data)) = group_data {
            if group_data.is_baby() {
                self.set_baby_zombie(true);
                // TODO(jockey): mount a nearby or newly created chicken once
                // spawn finalization can add entities before the zombie is added.
            }

            self.set_can_break_doors(
                rand::random::<f32>() < difficulty_modifier * DOOR_BREAK_CHANCE,
            );

            if spawn_reason != EntitySpawnReason::Conversion {
                self.populate_default_equipment_slots_zombie(difficulty);
                // TODO(enchantments): populate mob spawn equipment enchantments
                // once enchanting from a provider is available.
            }
        }

        if self.equipment_in_slot(EquipmentSlot::Head).is_empty()
            && special_dates::is_halloween()
            && rand::random::<f32>() < HALLOWEEN_PUMPKIN_CHANCE
        {
            let item: ItemRef = if rand::random::<f32>() < HALLOWEEN_JACK_O_LANTERN_CHANCE {
                &vanilla_items::JACK_O_LANTERN
            } else {
                &vanilla_items::CARVED_PUMPKIN
            };
            self.living_base()
                .equipment()
                .lock()
                .set(EquipmentSlot::Head, ItemStack::new(item));
            self.set_equipment_drop_chance(EquipmentSlot::Head, 0.0);
        }

        self.handle_attributes(difficulty_modifier, spawn_reason);

        group_data
    }

    /// Applies the vanilla pickup-loot roll to an egg-spawned offspring.
    fn on_offspring_spawned_from_egg_zombie(&self, _spawner: &Player, offspring: &SharedEntity) {
        let Some(world) = self.level() else {
            return;
        };
        let difficulty_modifier = world
            .get_current_difficulty_at(offspring.block_position())
            .special_multiplier();
        if let Some(offspring) = offspring.as_mob() {
            offspring.set_can_pick_up_loot(
                rand::random::<f32>() < PICKUP_LOOT_CHANCE * difficulty_modifier,
            );
        }
    }

    /// Vanilla `Zombie.getDefaultDimensions`, including the baby dimensions.
    fn zombie_dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.is_baby_zombie() {
            self.baby_dimensions().scale(scale)
        } else if self.entity_type().fixed {
            self.entity_type().dimensions
        } else {
            self.entity_type().dimensions.scale(scale)
        }
    }

    /// Vanilla `Zombie.getBaseExperienceReward`, which multiplies baby rewards.
    fn base_experience_reward_zombie(&self) -> i32 {
        let reward = if self.is_baby_zombie() {
            (f64::from(self.xp_reward()) * BABY_XP_MULTIPLIER) as i32
        } else {
            self.xp_reward()
        };
        Mob::base_experience_reward_mob_with(self, reward)
    }
}

/// Generates the [`Zombie`] implementation for a variant.
macro_rules! impl_zombie_state {
    ($ty:ty, $baby_dimensions:expr, { $($extra:tt)* }) => {
        impl Zombie for $ty {
            fn zombie_data(&self) -> MappedMutexGuard<'_, ZombieEntityData> {
                MutexGuard::map(self.entity_data.lock(), |data| data.zombie_mut())
            }

            fn zombie_synced_data(&self) -> &dyn EntitySyncedData {
                &self.entity_data
            }

            fn door_breaking(&self) -> &SyncMutex<DoorBreakingState> {
                &self.door_breaking
            }

            fn conversion_tracker(&self) -> &SyncMutex<ConversionTracker> {
                &self.conversion_tracker
            }

            fn baby_dimensions(&self) -> EntityDimensions {
                $baby_dimensions
            }

            $($extra)*
        }
    };
}

/// Generates the [`Entity`] implementation shared by every zombie variant.
macro_rules! impl_zombie_entity {
    ($ty:ty, { $($extra:tt)* }) => {
        impl Entity for $ty {
            fn base(&self) -> &EntityBase {
                &self.base
            }

            fn entity_type(&self) -> EntityTypeRef {
                self.entity_type
            }

            fn tick(self: Arc<Self>) {
                let entity = Arc::clone(&self).into_shared_entity();
                if let Some(living) = entity.as_living_entity() {
                    living.tick_living_entity(&entity);
                }
                Zombie::tick_drowning_conversion(self.as_ref());
            }

            fn base_tick(&self) {
                Mob::base_tick_mob(self);
            }

            fn as_zombie(&self) -> Option<&dyn Zombie> {
                Some(self)
            }

            fn dimensions_for_pose(&self, pose: EntityPose) -> EntityDimensions {
                self.zombie_dimensions_for_pose(pose)
            }

            fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
                Some(self.zombie_synced_data())
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
                Monster::sound_source_monster(self)
            }

            fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
                Monster::fall_sounds_monster(self)
            }

            fn swim_sound(&self) -> SoundEventRef {
                self.swim_sound_zombie()
            }

            fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
                self.play_sound(self.step_sound_zombie(), 0.15, 1.0);
            }

            fn save_additional(&self, nbt: &mut NbtCompound) {
                self.save_mob(nbt);
                nbt.insert("IsBaby", i8::from(self.is_baby_zombie()));
                nbt.insert("CanBreakDoors", i8::from(self.can_break_doors()));
                self.conversion_tracker().lock().save(
                    nbt,
                    self.is_eye_in_water(),
                    "InWaterTime",
                    "DrownedConversionTime",
                );
            }

            fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
                self.load_mob(nbt);
                self.set_baby_zombie(nbt.byte("IsBaby").is_some_and(|value| value != 0));
                self.set_can_break_doors(nbt.byte("CanBreakDoors").is_some_and(|value| value != 0));
                let converting = {
                    let mut tracker = self.conversion_tracker().lock();
                    tracker.load(
                        &nbt,
                        "InWaterTime",
                        "DrownedConversionTime",
                    );
                    tracker.is_converting()
                };
                self.zombie_data().drowned_conversion.set(converting);
            }

            $($extra)*
        }
    };
}

/// Generates the [`LivingEntity`] implementation shared by every zombie variant.
macro_rules! impl_zombie_living {
    ($ty:ty, { $($extra:tt)* }) => {
        impl LivingEntity for $ty {
            fn living_base(&self) -> &LivingEntityBase {
                &self.living_base
            }

            fn get_health(&self) -> f32 {
                *self.zombie_data().living_entity().health.get()
            }

            fn set_health(&self, health: f32) {
                let max_health = self.get_max_health();
                let clamped = health.clamp(0.0, max_health);
                self.zombie_data().living_entity_mut().health.set(clamped);
            }

            fn is_baby(&self) -> bool {
                Zombie::is_baby_zombie(self)
            }

            fn base_experience_reward(&self) -> i32 {
                self.base_experience_reward_zombie()
            }

            fn hurt_sound(&self, source: &DamageSource) -> Option<SoundEventRef> {
                Some(self.hurt_sound_zombie(source))
            }

            fn death_sound(&self) -> Option<SoundEventRef> {
                Some(self.death_sound_zombie())
            }

            fn should_drop_experience(&self) -> bool {
                Monster::should_drop_experience_monster(self)
            }

            fn should_drop_loot(&self, world: &World) -> bool {
                Monster::should_drop_loot_monster(self, world)
            }

            fn server_ai_step(&self, entity: &SharedEntity) {
                Mob::mob_server_ai_step(self, entity);
            }

            fn ai_step(&self, entity: &SharedEntity) -> Option<crate::physics::MoveResult> {
                Monster::monster_ai_step(self, entity)
            }

            $($extra)*
        }
    };
}

/// Generates the [`Mob`] implementation shared by every zombie variant.
macro_rules! impl_zombie_mob {
    ($ty:ty, { $($extra:tt)* }) => {
        impl Mob for $ty {
            fn mob_base(&self) -> &MobBase {
                &self.mob_base
            }

            fn mob_flags(&self) -> i8 {
                *self.zombie_data().mob().mob_flags.get()
            }

            fn set_mob_flags(&self, flags: i8) {
                self.zombie_data().mob_mut().mob_flags.set(flags);
            }

            fn set_baby(&self, baby: bool) {
                Zombie::set_baby_zombie(self, baby);
            }

            fn tick_goal_selectors(&self, entity: &SharedEntity) {
                PathfinderMob::tick_pathfinder_goal_selectors(self, entity);
            }

            fn tick_path_navigation(&self) {
                PathfinderMob::tick_pathfinder_path_navigation(self);
            }

            fn ambient_sound(&self) -> Option<SoundEventRef> {
                Some(self.ambient_sound_zombie())
            }

            fn do_hurt_target(
                &self,
                entity: &SharedEntity,
                world: &World,
                target: &SharedEntity,
            ) -> bool {
                self.do_hurt_target_zombie(entity, world, target)
            }

            fn can_hold_item(&self, item_stack: &ItemStack) -> bool {
                self.can_hold_item_zombie(item_stack)
            }

            fn wants_to_pick_up(&self, item_stack: &ItemStack) -> bool {
                self.wants_to_pick_up_zombie(item_stack)
            }

            fn finalize_spawn(
                &self,
                world: &Arc<World>,
                difficulty: DifficultyInstance,
                spawn_reason: EntitySpawnReason,
                group_data: Option<SpawnGroupData>,
            ) -> Option<SpawnGroupData> {
                self.finalize_spawn_zombie(world, difficulty, spawn_reason, group_data)
            }

            fn populate_default_equipment_slots(&self, difficulty: DifficultyInstance) {
                self.populate_default_equipment_slots_zombie(difficulty);
            }

            fn on_offspring_spawned_from_egg(
                &self,
                spawner: &Player,
                offspring: &SharedEntity,
            ) {
                self.on_offspring_spawned_from_egg_zombie(spawner, offspring);
            }

            $($extra)*
        }
    };
}

mod attack_goal;
pub mod husk;
#[expect(
    clippy::module_inception,
    reason = "the inner module is the vanilla Zombie class inside the zombie family module"
)]
pub mod zombie;

#[cfg(test)]
mod tests;

pub(crate) use attack_goal::ZombieAttackGoal;
pub use husk::HuskEntity;
pub use zombie::ZombieEntity;
