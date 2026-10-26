//! Vanilla Zombie entity with baby state, hostile targeting, and arm-raise
//! attack animation.

use std::iter::empty;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_attributes;
use steel_registry::vanilla_entity_data::ZombieEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{REGISTRY, TaggedRegistryExt, sound_events, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier};

use crate::entity::ai::goal::{
    BreakDoorGoal, Goal, GoalControls, GoalId, HurtByTargetGoal, LookAtPlayerGoal, MeleeAttackGoal,
    NearestAttackableTargetGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::entity::damage::DamageSource;
use crate::entity::mob::{
    RANDOM_SPAWN_BONUS_ID, do_hurt_target_default, populate_default_equipment_slots_default,
};
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason, EntitySyncedData,
    LivingEntity, LivingEntityBase, Mob, MobBase, Monster, PathfinderMob, SharedEntity,
    SpawnGroupData, ZombieGroupData,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::MoveResult;
use crate::player::Player;
use crate::special_dates;
use crate::world::{DifficultyInstance, World};

const DEFAULT_STEP_HEIGHT: f32 = 0.6;
const XP_REWARD: i32 = 5;
const BABY_XP_MULTIPLIER: f64 = 2.5;
const BABY_SPAWN_CHANCE: f32 = 0.05;
const BABY_WIDTH: f32 = 0.49;
const BABY_HEIGHT: f32 = 0.98;
const BABY_EYE_HEIGHT: f32 = 0.775;
const BABY_VEHICLE_ATTACHMENT: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.1875, 0.0)];
const BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    BABY_WIDTH,
    BABY_HEIGHT,
    BABY_EYE_HEIGHT,
    EntityAttachments::new(&[], &BABY_VEHICLE_ATTACHMENT, &[], &[]),
);
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

#[derive(Debug, Default)]
struct DoorBreakingState {
    can_break_doors: bool,
    goal_id: Option<GoalId>,
}

/// Vanilla Zombie.
#[entity_behavior(class = "Zombie")]
pub struct ZombieEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<ZombieEntityData>,
    door_breaking: SyncMutex<DoorBreakingState>,
}

// SAFETY: The owner-scoped type key uniquely identifies ZombieEntity.
unsafe impl DowncastType for ZombieEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/zombie");
}

impl ZombieEntity {
    /// Creates a new zombie entity instance.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Loads a saved zombie entity.
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
        let mut entity_data = ZombieEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            // TODO(spear): add SpearUseGoal here once Steel has one.
            goal_selector.add_goal(
                3,
                ZombieAttackGoal::new(ZOMBIE_ATTACK_SPEED_MODIFIER, false),
            );
            // TODO(turtle_egg): add ZombieAttackTurtleEggGoal here once
            // RemoveBlockGoal exists.
            // TODO(village): add MoveThroughVillageGoal here once the village/POI
            // manager exists.
            goal_selector.add_goal(7, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(8, LookAtPlayerGoal::new(8.0));
            goal_selector.add_goal(8, RandomLookAroundGoal::new());

            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(1, HurtByTargetGoal::new().set_alert_others(empty()));
            target_selector.add_goal(
                2,
                NearestAttackableTargetGoal::new_for_players(true, |_, _| true),
            );
            // TODO(targets): add villager, iron golem, and baby turtle target goals
            // here once those entities exist.
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            door_breaking: SyncMutex::new(DoorBreakingState::default()),
        }
    }

    /// Returns true if this zombie is a baby.
    #[must_use]
    pub fn is_baby(&self) -> bool {
        *self.entity_data.lock().zombie().baby.get()
    }

    /// Sets the vanilla baby state and its movement-speed modifier.
    pub fn set_baby(&self, baby: bool) {
        self.entity_data.lock().zombie_mut().baby.set(baby);

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

    /// Rolls vanilla `Zombie.getSpawnAsBabyOdds`.
    #[must_use]
    pub fn spawn_as_baby_odds() -> bool {
        rand::random::<f32>() < BABY_SPAWN_CHANCE
    }

    /// Returns whether this zombie can break wooden doors.
    #[must_use]
    pub fn can_break_doors(&self) -> bool {
        self.door_breaking.lock().can_break_doors
    }

    /// Sets whether this zombie can break wooden doors and keeps the goal
    /// selector and navigation in sync.
    pub fn set_can_break_doors(&self, can_break_doors: bool) {
        let removed_goal = {
            let mut state = self.door_breaking.lock();
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

        self.mob_base
            .navigation()
            .lock()
            .set_can_open_doors(can_break_doors);

        let mut goal_selector = self.mob_base.goal_selector().lock();
        if can_break_doors {
            let goal_id = goal_selector.add_goal(
                1,
                BreakDoorGoal::new(|difficulty| difficulty == Difficulty::Hard),
            );
            self.door_breaking.lock().goal_id = Some(goal_id);
        } else if let Some(goal_id) = removed_goal {
            goal_selector.remove_goal(self, goal_id);
        }
    }

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
    }
}

// TODO(villager_infection): override killed_entity to convert a killed Villager
// into a Zombie Villager on Normal/Hard once both entities exist.
impl Entity for ZombieEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
        // TODO(drowned_conversion): tick a conversion tracker here once
        // DrownedEntity exists; persist InWaterTime and DrownedConversionTime
        // and play level event 1040 on conversion.
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.is_baby() {
            BABY_DIMENSIONS.scale(scale)
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
        Monster::sound_source_monster(self)
    }

    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        Monster::fall_sounds_monster(self)
    }

    fn swim_sound(&self) -> SoundEventRef {
        Monster::swim_sound_monster(self)
    }

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(&sound_events::ENTITY_ZOMBIE_STEP, 0.15, 1.0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        nbt.insert("IsBaby", i8::from(self.is_baby()));
        nbt.insert("CanBreakDoors", i8::from(self.can_break_doors()));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.set_baby(nbt.byte("IsBaby").is_some_and(|value| value != 0));
        self.set_can_break_doors(nbt.byte("CanBreakDoors").is_some_and(|value| value != 0));
    }
}

// TODO(reinforcements): override hurt_server to spawn backup zombies on Hard
// once SpawnPlacements and the entity AABB/liquid helpers exist.
impl LivingEntity for ZombieEntity {
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

    fn is_baby(&self) -> bool {
        ZombieEntity::is_baby(self)
    }

    fn base_experience_reward(&self) -> i32 {
        let reward = if self.is_baby() {
            (f64::from(self.xp_reward()) * BABY_XP_MULTIPLIER) as i32
        } else {
            self.xp_reward()
        };
        Mob::base_experience_reward_mob_with(self, reward)
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ZOMBIE_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ZOMBIE_DEATH)
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

    fn ai_step(&self, entity: &SharedEntity) -> Option<MoveResult> {
        Monster::monster_ai_step(self, entity)
    }
}

impl Mob for ZombieEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn set_baby(&self, baby: bool) {
        ZombieEntity::set_baby(self, baby);
    }

    fn tick_goal_selectors(&self, entity: &SharedEntity) {
        PathfinderMob::tick_pathfinder_goal_selectors(self, entity);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ZOMBIE_AMBIENT)
    }

    fn do_hurt_target(&self, entity: &SharedEntity, world: &World, target: &SharedEntity) -> bool {
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

    fn can_hold_item(&self, item_stack: &ItemStack) -> bool {
        !(self.is_baby()
            && self.is_passenger()
            && REGISTRY.items.is_in_tag(item_stack.item(), &ItemTag::EGGS))
    }

    fn wants_to_pick_up(&self, item_stack: &ItemStack) -> bool {
        !item_stack.is(&vanilla_items::GLOW_INK_SAC) && Mob::can_hold_item(self, item_stack)
    }

    fn finalize_spawn(
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
                ZombieEntity::spawn_as_baby_odds(),
                true,
            ))),
            group_data => group_data,
        };

        if let Some(SpawnGroupData::Zombie(group_data)) = group_data {
            if group_data.is_baby() {
                self.set_baby(true);
                // TODO(jockey): mount a nearby or newly created chicken once
                // spawn finalization can add entities before the zombie is added.
            }

            self.set_can_break_doors(
                rand::random::<f32>() < difficulty_modifier * DOOR_BREAK_CHANCE,
            );

            if spawn_reason != EntitySpawnReason::Conversion {
                self.populate_default_equipment_slots(difficulty);
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

    fn on_offspring_spawned_from_egg(&self, _spawner: &Player, offspring: &SharedEntity) {
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

    fn populate_default_equipment_slots(&self, difficulty: DifficultyInstance) {
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

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl Monster for ZombieEntity {}

impl PathfinderMob for ZombieEntity {}

/// Vanilla `ZombieAttackGoal`: raises the arms while winding up an attack.
struct ZombieAttackGoal {
    melee: MeleeAttackGoal,
    raise_arm_ticks: i32,
}

impl ZombieAttackGoal {
    const fn new(speed_modifier: f64, following_target_even_if_not_seen: bool) -> Self {
        Self {
            melee: MeleeAttackGoal::new(speed_modifier, following_target_even_if_not_seen),
            raise_arm_ticks: 0,
        }
    }
}

impl Goal for ZombieAttackGoal {
    fn controls(&self) -> GoalControls {
        self.melee.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.melee.can_use(mob)
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.melee.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.melee.start(mob);
        mob.set_aggressive(true);
        self.raise_arm_ticks = 0;
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.melee.stop(mob);
        mob.set_aggressive(false);
    }

    fn requires_update_every_tick(&self) -> bool {
        self.melee.requires_update_every_tick()
    }

    fn tick(&mut self, mob: &dyn PathfinderMob, entity: &SharedEntity) {
        self.melee.tick(mob, entity);
        self.raise_arm_ticks += 1;
        let winding_up = self.raise_arm_ticks >= ZOMBIE_RAISE_ARM_TICKS_BEFORE_ATTACK
            && self.melee.get_ticks_until_next_attack() < MeleeAttackGoal::attack_interval() / 2;
        mob.set_aggressive(winding_up);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use simdnbt::borrow::read_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::entity_data::EntityPose;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::vanilla_attributes;
    use steel_registry::{init_vanilla_registry, vanilla_entities, vanilla_items};
    use steel_utils::types::Difficulty;

    use super::{LEADER_ZOMBIE_BONUS_ID, ZOMBIE_RANDOM_SPAWN_BONUS_ID, ZombieEntity};
    use crate::entity::{
        AgeableMobGroupData, Entity, EntitySpawnReason, LivingEntity, Mob, SharedEntity,
        SpawnGroupData, ZombieGroupData,
    };
    use crate::inventory::equipment::EquipmentSlot;
    use crate::special_dates;
    use crate::test_support::fresh_test_world;
    use crate::world::{DifficultyInstance, World};

    fn make_zombie(id: i32) -> ZombieEntity {
        ZombieEntity::new(&vanilla_entities::ZOMBIE, id, DVec3::ZERO, Weak::new())
    }

    fn peaceful_difficulty() -> DifficultyInstance {
        DifficultyInstance::new(Difficulty::Peaceful, 0, 0, 0.0)
    }

    fn hard_difficulty() -> DifficultyInstance {
        DifficultyInstance::new(Difficulty::Hard, 1_512_000, 3_600_000, 0.0)
    }

    fn assert_head_has_only_halloween_gear(zombie: &ZombieEntity) {
        let head = zombie.equipment_in_slot(EquipmentSlot::Head);
        if special_dates::is_halloween() {
            assert!(
                head.is_empty()
                    || head.is(&vanilla_items::CARVED_PUMPKIN)
                    || head.is(&vanilla_items::JACK_O_LANTERN),
                "Halloween can replace an empty head slot, but nothing else: {head:?}"
            );
            if !head.is_empty() {
                assert_eq!(zombie.equipment_drop_chance(EquipmentSlot::Head), 0.0);
            }
        } else {
            assert!(head.is_empty(), "head slot should be empty: {head:?}");
        }
    }

    fn assert_no_armor(zombie: &ZombieEntity) {
        for slot in [
            EquipmentSlot::Chest,
            EquipmentSlot::Legs,
            EquipmentSlot::Feet,
        ] {
            assert!(
                zombie.equipment_in_slot(slot).is_empty(),
                "armor slot {slot:?} should be empty"
            );
        }
        assert_head_has_only_halloween_gear(zombie);
    }

    #[test]
    fn zombie_baby_nbt_round_trip() {
        init_vanilla_registry();

        let zombie = make_zombie(1);
        assert!(!zombie.is_baby());

        zombie.set_baby(true);
        zombie.set_can_break_doors(true);
        assert!(zombie.is_baby());

        let mut nbt = NbtCompound::new();
        zombie.save_additional(&mut nbt);
        assert_eq!(nbt.byte("IsBaby"), Some(1));
        assert_eq!(nbt.byte("CanBreakDoors"), Some(1));

        let loaded = make_zombie(2);
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("reborrow failed: {error}"));
        loaded.load_additional((&borrowed).into());

        assert!(loaded.is_baby());
        assert!(loaded.can_break_doors());
    }

    #[test]
    fn zombie_can_break_doors_toggles_goal_and_navigation() {
        init_vanilla_registry();
        let zombie = make_zombie(1);
        let initial_goals = zombie
            .mob_base()
            .goal_selector()
            .lock()
            .available_goal_count();

        assert!(!zombie.can_break_doors());
        assert!(!zombie.mob_base().navigation().lock().can_open_doors());

        zombie.set_can_break_doors(true);
        assert!(zombie.can_break_doors());
        assert!(zombie.mob_base().navigation().lock().can_open_doors());
        assert_eq!(
            zombie
                .mob_base()
                .goal_selector()
                .lock()
                .available_goal_count(),
            initial_goals + 1
        );

        zombie.set_can_break_doors(true);
        assert_eq!(
            zombie
                .mob_base()
                .goal_selector()
                .lock()
                .available_goal_count(),
            initial_goals + 1
        );

        zombie.set_can_break_doors(false);
        assert!(!zombie.can_break_doors());
        assert!(!zombie.mob_base().navigation().lock().can_open_doors());
        assert_eq!(
            zombie
                .mob_base()
                .goal_selector()
                .lock()
                .available_goal_count(),
            initial_goals
        );
    }

    #[test]
    fn zombie_baby_dimensions_match_vanilla() {
        init_vanilla_registry();

        let zombie = make_zombie(1);
        let adult = zombie.dimensions_for_pose(EntityPose::Standing);
        assert_eq!(
            (adult.width, adult.height),
            (
                vanilla_entities::ZOMBIE.dimensions.width,
                vanilla_entities::ZOMBIE.dimensions.height
            )
        );

        zombie.set_baby(true);
        let baby = zombie.dimensions_for_pose(EntityPose::Standing);
        assert_eq!((baby.width, baby.height), (0.49, 0.98));
    }

    #[test]
    fn zombie_baby_speed_modifier_requires_world_and_applies_once() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("zombie_baby_speed");
        let world: &Arc<World> = &world_fixture.world;

        let without_world = make_zombie(1);
        without_world.set_baby(true);
        let speed = without_world
            .attributes()
            .lock()
            .get_value(vanilla_attributes::MOVEMENT_SPEED)
            .expect("zombies have movement speed");
        assert!((speed - 0.23).abs() < 1.0e-6);

        let with_world = ZombieEntity::new(
            &vanilla_entities::ZOMBIE,
            2,
            DVec3::new(8.0, 65.0, 8.0),
            Arc::downgrade(world),
        );
        with_world.set_baby(true);
        let speed = with_world
            .attributes()
            .lock()
            .get_value(vanilla_attributes::MOVEMENT_SPEED)
            .expect("zombies have movement speed");
        assert!((speed - 0.345).abs() < 1.0e-6);

        with_world.set_baby(true);
        let speed = with_world
            .attributes()
            .lock()
            .get_value(vanilla_attributes::MOVEMENT_SPEED)
            .expect("zombies have movement speed");
        assert!((speed - 0.345).abs() < 1.0e-6);

        with_world.set_baby(false);
        let speed = with_world
            .attributes()
            .lock()
            .get_value(vanilla_attributes::MOVEMENT_SPEED)
            .expect("zombies have movement speed");
        assert!((speed - 0.23).abs() < 1.0e-6);
    }

    #[test]
    fn zombie_baby_experience_reward_matches_vanilla() {
        init_vanilla_registry();

        let zombie = make_zombie(1);
        assert_eq!(LivingEntity::base_experience_reward(&zombie), 5);

        zombie.set_baby(true);
        assert_eq!(LivingEntity::base_experience_reward(&zombie), 12);
    }

    #[test]
    fn zombie_group_data_finalize_sets_baby() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("zombie_group_data");
        let world = &world_fixture.world;

        let adult = make_zombie(1);
        let result = adult.finalize_spawn(
            world,
            peaceful_difficulty(),
            EntitySpawnReason::Natural,
            Some(SpawnGroupData::Zombie(ZombieGroupData::new(false, false))),
        );
        assert!(!adult.is_baby());
        assert_eq!(
            result,
            Some(SpawnGroupData::Zombie(ZombieGroupData::new(false, false)))
        );

        let baby = make_zombie(2);
        baby.finalize_spawn(
            world,
            peaceful_difficulty(),
            EntitySpawnReason::Natural,
            Some(SpawnGroupData::Zombie(ZombieGroupData::new(true, false))),
        );
        assert!(baby.is_baby());
    }

    #[test]
    fn zombie_passes_through_foreign_group_data() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("zombie_foreign_group_data");
        let world = &world_fixture.world;
        let zombie = make_zombie(1);
        let group_data = AgeableMobGroupData::with_should_spawn_baby(true);

        let result = zombie.finalize_spawn(
            world,
            peaceful_difficulty(),
            EntitySpawnReason::Natural,
            Some(SpawnGroupData::AgeableMob(group_data)),
        );

        assert_eq!(result, Some(SpawnGroupData::AgeableMob(group_data)));
        assert!(!zombie.is_baby());
    }

    #[test]
    fn zombie_peaceful_spawn_has_no_armor_or_spawn_bonuses() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("zombie_peaceful_spawn");
        let world = &world_fixture.world;
        let zombie = make_zombie(1);
        zombie.set_can_pick_up_loot(true);

        zombie.finalize_spawn(
            world,
            peaceful_difficulty(),
            EntitySpawnReason::Natural,
            None,
        );

        assert!(!zombie.can_pick_up_loot());
        assert_no_armor(&zombie);
        assert!(
            zombie.equipment_in_slot(EquipmentSlot::OffHand).is_empty(),
            "offhand should be empty"
        );
        assert!(
            zombie.equipment_in_slot(EquipmentSlot::MainHand).is_empty()
                || zombie
                    .equipment_in_slot(EquipmentSlot::MainHand)
                    .is(&vanilla_items::IRON_SWORD)
                || zombie
                    .equipment_in_slot(EquipmentSlot::MainHand)
                    .is(&vanilla_items::IRON_SPEAR)
                || zombie
                    .equipment_in_slot(EquipmentSlot::MainHand)
                    .is(&vanilla_items::IRON_SHOVEL),
            "main hand may only contain the vanilla iron weapon roll"
        );

        let attributes = zombie.attributes().lock();
        let reinforcement = attributes.required_value(vanilla_attributes::SPAWN_REINFORCEMENTS);
        assert!((0.0..0.1).contains(&reinforcement));
        let knockback = attributes.required_value(vanilla_attributes::KNOCKBACK_RESISTANCE);
        assert!((0.0..0.05).contains(&knockback));
        assert!(!attributes.has_modifier(
            vanilla_attributes::SPAWN_REINFORCEMENTS,
            &LEADER_ZOMBIE_BONUS_ID
        ));
        assert!(!attributes.has_modifier(vanilla_attributes::MAX_HEALTH, &LEADER_ZOMBIE_BONUS_ID));
        assert!(!attributes.has_modifier(
            vanilla_attributes::FOLLOW_RANGE,
            &ZOMBIE_RANDOM_SPAWN_BONUS_ID
        ));
        assert_eq!(
            attributes.required_value(vanilla_attributes::MAX_HEALTH),
            20.0
        );
    }

    #[test]
    fn zombie_conversion_spawn_keeps_pickup_and_does_not_equip() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("zombie_conversion_spawn");
        let world = &world_fixture.world;

        for id in 0..200 {
            let zombie = make_zombie(id);
            zombie.set_can_pick_up_loot(true);
            zombie.living_base().equipment().lock().set(
                EquipmentSlot::MainHand,
                ItemStack::new(&vanilla_items::STONE),
            );

            zombie.finalize_spawn(
                world,
                hard_difficulty(),
                EntitySpawnReason::Conversion,
                None,
            );

            assert!(zombie.can_pick_up_loot());
            assert!(
                zombie
                    .equipment_in_slot(EquipmentSlot::MainHand)
                    .is(&vanilla_items::STONE)
            );
            for slot in [
                EquipmentSlot::Chest,
                EquipmentSlot::Legs,
                EquipmentSlot::Feet,
            ] {
                assert!(
                    zombie.equipment_in_slot(slot).is_empty(),
                    "conversion spawn must not equip {slot:?}"
                );
            }
            assert_head_has_only_halloween_gear(&zombie);
        }
    }

    #[test]
    fn zombie_leader_spawn_applies_vanilla_bonuses() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("zombie_leader_spawn");
        let world = &world_fixture.world;
        let mut leaders = 0;

        for id in 0..500 {
            let zombie = make_zombie(id);
            zombie.finalize_spawn(world, hard_difficulty(), EntitySpawnReason::Natural, None);

            let attributes = zombie.attributes().lock();
            let reinforcement = attributes.required_value(vanilla_attributes::SPAWN_REINFORCEMENTS);
            let knockback = attributes.required_value(vanilla_attributes::KNOCKBACK_RESISTANCE);
            assert!((0.0..0.05).contains(&knockback));

            let follow_range = attributes.required_value(vanilla_attributes::FOLLOW_RANGE);
            if attributes.has_modifier(
                vanilla_attributes::FOLLOW_RANGE,
                &ZOMBIE_RANDOM_SPAWN_BONUS_ID,
            ) {
                assert!(
                    follow_range > 61.0,
                    "follow range bonus should at least double the base: {follow_range}"
                );
            } else {
                assert!(
                    (30.9..=39.1).contains(&follow_range),
                    "base follow range should stay near 35: {follow_range}"
                );
            }

            if attributes.has_modifier(
                vanilla_attributes::SPAWN_REINFORCEMENTS,
                &LEADER_ZOMBIE_BONUS_ID,
            ) {
                leaders += 1;
                assert!((0.5..0.85).contains(&reinforcement));
                assert!(
                    attributes
                        .has_modifier(vanilla_attributes::MAX_HEALTH, &LEADER_ZOMBIE_BONUS_ID)
                );
                let max_health = attributes.required_value(vanilla_attributes::MAX_HEALTH);
                assert!((40.0..100.0).contains(&max_health));
                drop(attributes);
                assert_eq!(zombie.get_health(), zombie.get_max_health());
            } else {
                assert!((0.0..0.1).contains(&reinforcement));
                drop(attributes);
                assert_eq!(zombie.get_health(), 20.0);
            }
        }

        assert!(
            leaders > 0,
            "expected a leader among 500 hard-difficulty spawns"
        );
    }

    #[test]
    fn zombie_rejects_eggs_for_riding_babies_and_glow_ink_sac() {
        init_vanilla_registry();

        let zombie = make_zombie(1);
        let egg = ItemStack::new(&vanilla_items::EGG);
        let glow_ink_sac = ItemStack::new(&vanilla_items::GLOW_INK_SAC);
        let stone = ItemStack::new(&vanilla_items::STONE);

        assert!(zombie.can_hold_item(&egg));
        assert!(zombie.wants_to_pick_up(&egg));
        assert!(!zombie.wants_to_pick_up(&glow_ink_sac));
        assert!(zombie.can_hold_item(&stone));

        zombie.set_baby(true);
        assert!(zombie.can_hold_item(&egg));
    }

    #[test]
    fn zombie_spawn_uses_vanilla_hostile_defaults() {
        init_vanilla_registry();

        let zombie: SharedEntity = Arc::new(make_zombie(1));
        let mob = zombie.as_mob().expect("zombie is a mob");
        assert_eq!(mob.xp_reward(), 5);
    }

    #[test]
    fn zombie_baby_odds_are_five_percent_boundary() {
        init_vanilla_registry();

        let mut babies = 0;
        for _ in 0..10_000 {
            if ZombieEntity::spawn_as_baby_odds() {
                babies += 1;
            }
        }

        assert!(
            (300..=700).contains(&babies),
            "baby rolls should cluster around 5%: {babies}"
        );
    }
}
