//! Vanilla-shaped mob foundations.

use std::f32::consts::PI;
use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_math::fast_floor;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::LootTableRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_rules::ENTITY_DROPS;
use steel_registry::{
    REGISTRY, RegistryExt, sound_events, vanilla_attributes, vanilla_blocks, vanilla_damage_types,
    vanilla_entities, vanilla_game_events, vanilla_items,
};
use steel_utils::UuidExt;
use steel_utils::locks::SyncMutex;
use steel_utils::types::{Difficulty, InteractionHand};
use steel_utils::{BlockPos, ChunkPos, Identifier, WorldAabb, axis::Axis};
use uuid::Uuid;

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, ITEM_BEHAVIORS, InteractionResult};
use crate::enchantment_helper::{self, EnchantmentDamageContext, EnchantmentPostAttackContext};
use crate::entity::ai::control::{
    BodyRotationInput, MobControls, MoveControlOperation, rotate_if_necessary, rotate_towards,
};
use crate::entity::ai::goal::{GoalControl, GoalSelector};
use crate::entity::ai::navigation::{
    NavigationPathRequest, NavigationRecomputeRequest, NavigationTickContext, PathNavigation,
};
use crate::entity::ai::path::{Path, PathType, PathfindingContext, PathfindingMalus};
use crate::entity::ai::sensing::Sensing;
use crate::entity::ai::walk::{MobPathSettings, WalkNodeEvaluator, WalkPathEvaluator};
use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::entity::damage::DamageSource;
use crate::entity::entities::LeashFenceKnotEntity;
use crate::entity::{
    Entity, EntitySpawnReason, LivingEntity, LivingTravelInput, RemovalReason, SharedEntity,
    SpawnGroupData, WeakEntity,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::WorldCollisionProvider;
use crate::player::Player;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, World};

const MOB_FLAG_NO_AI: i8 = 1;
const MOB_FLAG_LEFT_HANDED: i8 = 2;
const MOB_FLAG_AGGRESSIVE: i8 = 4;
const MOVE_CONTROL_MIN_SPEED_SQR: f64 = 2.500_000_3e-7;
const MOVE_CONTROL_MAX_TURN: f32 = 90.0;
const DEFAULT_EQUIPMENT_DROP_CHANCE: f32 = 0.085;
const PRESERVE_ITEM_DROP_CHANCE_THRESHOLD: f32 = 1.0;
const PRESERVE_ITEM_DROP_CHANCE: f32 = 2.0;
const LEASH_SNAP_DISTANCE: f64 = 12.0;
const LEASH_ELASTIC_DISTANCE: f64 = 6.0;
const LEASH_AXIS_SPECIFIC_ELASTICITY: DVec3 = DVec3::new(0.8, 0.2, 0.8);
const LEASH_SPRING_DAMPENING: f64 = 0.7;
const LEASH_TORSIONAL_ELASTICITY: f64 = 10.0;
const LEASH_STIFFNESS: f64 = 0.11;
const ENTITY_LEASH_ATTACHMENT_POINT: DVec3 = DVec3::new(0.0, 0.5, 0.5);
const LEASHER_ATTACHMENT_POINT: DVec3 = DVec3::new(0.0, 0.5, 0.0);
const DELAYED_LEASH_DROP_TICKS: i32 = 100;
const BODY_ROTATION_MOVING_DISTANCE_SQR: f64 = 2.500_000_3e-7;
const TARGET_REACH_DISTANCE_SQR: f64 = 2.25;
const DEFAULT_ATTACK_REACH_BASE: f32 = 2.04;
const DEFAULT_ATTACK_REACH_OFFSET: f32 = 0.6;
const RANDOM_SPAWN_BONUS_ID: Identifier = Identifier::vanilla_static("random_spawn_bonus");
const RANDOM_SPAWN_BONUS_SCALE: f64 = 0.114_850_000_000_000_01;
const LEFT_HANDED_SPAWN_CHANCE: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
struct DropChances {
    by_equipment: [f32; EquipmentSlot::ALL.len()],
}

impl DropChances {
    const DEFAULT: Self = Self {
        by_equipment: [DEFAULT_EQUIPMENT_DROP_CHANCE; EquipmentSlot::ALL.len()],
    };

    #[must_use]
    const fn by_equipment(self, slot: EquipmentSlot) -> f32 {
        self.by_equipment[slot.index()]
    }

    const fn set_guaranteed_drop(&mut self, slot: EquipmentSlot) {
        self.by_equipment[slot.index()] = PRESERVE_ITEM_DROP_CHANCE;
    }

    fn set_equipment_chance(&mut self, slot: EquipmentSlot, chance: f32) -> bool {
        if chance < 0.0 {
            return false;
        }

        self.by_equipment[slot.index()] = chance;
        true
    }

    #[must_use]
    fn is_preserved(self, slot: EquipmentSlot) -> bool {
        self.by_equipment(slot) > PRESERVE_ITEM_DROP_CHANCE_THRESHOLD
    }

    fn save(self, nbt: &mut NbtCompound) {
        if self == Self::DEFAULT {
            return;
        }

        let mut drop_chances = NbtCompound::new();
        for slot in EquipmentSlot::ALL {
            let chance = self.by_equipment(slot);
            if chance.to_bits() != DEFAULT_EQUIPMENT_DROP_CHANCE.to_bits() {
                drop_chances.insert(slot.name(), chance);
            }
        }

        nbt.insert("drop_chances", NbtTag::Compound(drop_chances));
    }

    fn load(nbt: BorrowedNbtCompoundView<'_, '_>) -> Self {
        let Some(drop_chances) = nbt.compound("drop_chances") else {
            return Self::DEFAULT;
        };

        let mut loaded = Self::DEFAULT;
        for slot in EquipmentSlot::ALL {
            let Some(chance) = drop_chances.float(slot.name()) else {
                continue;
            };
            if !loaded.set_equipment_chance(slot, chance) {
                return Self::DEFAULT;
            }
        }

        loaded
    }
}

#[derive(Debug)]
pub struct MobBase {
    goal_selector: SyncMutex<GoalSelector>,
    target_selector: SyncMutex<GoalSelector>,
    target: SyncMutex<Option<WeakEntity>>,
    sensing: SyncMutex<Sensing>,
    controls: SyncMutex<MobControls>,
    navigation: SyncMutex<PathNavigation>,
    pathfinding_malus: SyncMutex<PathfindingMalus>,
    persistence_required: SyncMutex<bool>,
    can_pick_up_loot: SyncMutex<bool>,
    drop_chances: SyncMutex<DropChances>,
    home_restriction: SyncMutex<MobHomeRestriction>,
    death_loot_table: SyncMutex<Option<Identifier>>,
    death_loot_table_seed: SyncMutex<i64>,
    leash_data: SyncMutex<Option<LeashData>>,
    ambient_sound_time: SyncMutex<i32>,
    xp_reward: SyncMutex<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MobHomeRestriction {
    position: BlockPos,
    radius: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeashAttachment {
    Entity(Uuid),
    FenceKnot(BlockPos),
}

#[derive(Debug, Clone)]
struct LeashData {
    attachment: LeashAttachment,
    holder: Option<WeakEntity>,
    angular_momentum: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LeashWrench {
    force: DVec3,
    torque: f64,
}

impl LeashWrench {
    const fn new(force: DVec3, torque: f64) -> Self {
        Self { force, torque }
    }
}

impl MobHomeRestriction {
    const fn none() -> Self {
        Self {
            position: BlockPos::ZERO,
            radius: -1,
        }
    }
}

impl LeashData {
    fn from_entity(holder: &SharedEntity) -> Self {
        let attachment = holder.as_leash_fence_knot().map_or_else(
            || LeashAttachment::Entity(holder.uuid()),
            |knot| LeashAttachment::FenceKnot(knot.leash_fence_pos()),
        );
        Self {
            attachment,
            holder: Some(Arc::downgrade(holder)),
            angular_momentum: 0.0,
        }
    }

    const fn from_delayed_attachment(attachment: LeashAttachment) -> Self {
        Self {
            attachment,
            holder: None,
            angular_momentum: 0.0,
        }
    }

    fn holder(&self) -> Option<SharedEntity> {
        self.holder.as_ref().and_then(WeakEntity::upgrade)
    }

    fn saved_attachment(&self) -> LeashAttachment {
        self.holder().map_or(self.attachment, |holder| {
            holder.as_leash_fence_knot().map_or_else(
                || LeashAttachment::Entity(holder.uuid()),
                |knot| LeashAttachment::FenceKnot(knot.leash_fence_pos()),
            )
        })
    }

    fn set_holder(&mut self, holder: &SharedEntity) {
        self.attachment = holder.as_leash_fence_knot().map_or_else(
            || LeashAttachment::Entity(holder.uuid()),
            |knot| LeashAttachment::FenceKnot(knot.leash_fence_pos()),
        );
        self.holder = Some(Arc::downgrade(holder));
        self.angular_momentum = 0.0;
    }

    fn save(&self, nbt: &mut NbtCompound) {
        match self.saved_attachment() {
            LeashAttachment::Entity(uuid) => {
                let mut leash = NbtCompound::new();
                leash.insert("UUID", NbtTag::IntArray(uuid.to_int_array().to_vec()));
                nbt.insert("leash", NbtTag::Compound(leash));
            }
            LeashAttachment::FenceKnot(pos) => {
                nbt.insert("leash", NbtTag::IntArray(vec![pos.x(), pos.y(), pos.z()]));
            }
        }
    }

    fn load(nbt: BorrowedNbtCompoundView<'_, '_>) -> Option<Self> {
        if let Some(leash) = nbt.compound("leash")
            && let Some(uuid_array) = leash.int_array("UUID")
            && let Some(uuid) = Uuid::from_int_array(&uuid_array)
        {
            return Some(Self::from_delayed_attachment(LeashAttachment::Entity(uuid)));
        }

        nbt.int_array("leash")
            .filter(|position| position.len() == 3)
            .map(|position| {
                Self::from_delayed_attachment(LeashAttachment::FenceKnot(BlockPos::new(
                    position[0],
                    position[1],
                    position[2],
                )))
            })
    }
}

fn leash_dimensions(entity: &dyn Entity) -> DVec3 {
    let dimensions = entity.base().dimensions();
    DVec3::new(
        f64::from(dimensions.width),
        f64::from(dimensions.height),
        f64::from(dimensions.width),
    )
}

fn leash_bounding_box_center(entity: &dyn Entity) -> DVec3 {
    let bounding_box = entity.bounding_box();
    DVec3::new(
        f64::midpoint(bounding_box.min_x(), bounding_box.max_x()),
        f64::midpoint(bounding_box.min_y(), bounding_box.max_y()),
        f64::midpoint(bounding_box.min_z(), bounding_box.max_z()),
    )
}

fn leash_holder_movement(entity: &dyn Entity) -> DVec3 {
    if entity.as_mob().is_some_and(Mob::is_no_ai) {
        return DVec3::ZERO;
    }

    entity.known_movement()
}

fn rotate_y(vector: DVec3, radians: f32) -> DVec3 {
    let cos = f64::from(radians.cos());
    let sin = f64::from(radians.sin());
    DVec3::new(
        vector.x * cos + vector.z * sin,
        vector.y,
        vector.z * cos - vector.x * sin,
    )
}

fn axis_specific_leash_elasticity(force: DVec3) -> DVec3 {
    force * LEASH_AXIS_SPECIFIC_ELASTICITY
}

fn compute_elastic_interaction(
    entity: &dyn Entity,
    holder: &dyn Entity,
    slack_distance: f64,
) -> Option<LeashWrench> {
    let entity_y_rot = entity.rotation().0 * PI / 180.0;
    let entity_attach_vector = rotate_y(
        ENTITY_LEASH_ATTACHMENT_POINT * leash_dimensions(entity),
        -entity_y_rot,
    );
    let entity_attach_pos = entity.position() + entity_attach_vector;

    let holder_y_rot = holder.rotation().0 * PI / 180.0;
    let holder_attach_vector = rotate_y(
        LEASHER_ATTACHMENT_POINT * leash_dimensions(holder),
        -holder_y_rot,
    );
    let holder_attach_pos = holder.position() + holder_attach_vector;

    compute_dampened_spring_interaction(
        holder_attach_pos,
        entity_attach_pos,
        slack_distance,
        leash_holder_movement(entity),
        entity_attach_vector,
    )
}

fn compute_dampened_spring_interaction(
    pivot_point: DVec3,
    object_position: DVec3,
    spring_slack: f64,
    object_motion: DVec3,
    lever_arm: DVec3,
) -> Option<LeashWrench> {
    let distance = object_position.distance(pivot_point);
    if distance < spring_slack {
        return None;
    }

    let mut displacement = (pivot_point - object_position).normalize() * (distance - spring_slack);
    let torque = torque_from_force(lever_arm, displacement);
    if object_motion.dot(displacement) >= 0.0 {
        displacement *= 1.0 - LEASH_SPRING_DAMPENING;
    }

    Some(LeashWrench::new(displacement, torque))
}

fn torque_from_force(lever_arm: DVec3, force: DVec3) -> f64 {
    lever_arm.z * force.x - lever_arm.x * force.z
}

impl MobBase {
    #[must_use]
    pub fn new() -> Self {
        Self {
            goal_selector: SyncMutex::new(GoalSelector::new()),
            target_selector: SyncMutex::new(GoalSelector::new()),
            target: SyncMutex::new(None),
            sensing: SyncMutex::new(Sensing::new()),
            controls: SyncMutex::new(MobControls::new()),
            navigation: SyncMutex::new(PathNavigation::new()),
            pathfinding_malus: SyncMutex::new(PathfindingMalus::new()),
            persistence_required: SyncMutex::new(false),
            can_pick_up_loot: SyncMutex::new(false),
            drop_chances: SyncMutex::new(DropChances::DEFAULT),
            home_restriction: SyncMutex::new(MobHomeRestriction::none()),
            death_loot_table: SyncMutex::new(None),
            death_loot_table_seed: SyncMutex::new(0),
            leash_data: SyncMutex::new(None),
            ambient_sound_time: SyncMutex::new(0),
            xp_reward: SyncMutex::new(0),
        }
    }

    #[must_use]
    pub const fn goal_selector(&self) -> &SyncMutex<GoalSelector> {
        &self.goal_selector
    }

    #[must_use]
    pub const fn target_selector(&self) -> &SyncMutex<GoalSelector> {
        &self.target_selector
    }

    #[must_use]
    pub(crate) const fn sensing(&self) -> &SyncMutex<Sensing> {
        &self.sensing
    }

    #[must_use]
    pub fn target(&self, is_valid: impl Fn(&dyn LivingEntity) -> bool) -> Option<SharedEntity> {
        let mut target = self.target.lock();
        let Some(upgraded) = target.as_ref().and_then(WeakEntity::upgrade) else {
            *target = None;
            return None;
        };
        let living_target = upgraded.as_living_entity()?;
        if !is_valid(living_target) {
            return None;
        }
        Some(upgraded)
    }

    pub fn set_target(
        &self,
        target: Option<&SharedEntity>,
        is_valid: impl Fn(&dyn LivingEntity) -> bool,
    ) -> bool {
        let Some(target) = target else {
            *self.target.lock() = None;
            return true;
        };
        if !target.is_living_entity() {
            return false;
        }
        let Some(living_target) = target.as_living_entity() else {
            return false;
        };
        if !is_valid(living_target) {
            *self.target.lock() = None;
            return false;
        }

        *self.target.lock() = Some(Arc::downgrade(target));
        true
    }

    #[must_use]
    pub const fn controls(&self) -> &SyncMutex<MobControls> {
        &self.controls
    }

    #[must_use]
    pub const fn navigation(&self) -> &SyncMutex<PathNavigation> {
        &self.navigation
    }

    #[must_use]
    pub const fn pathfinding_malus(&self) -> &SyncMutex<PathfindingMalus> {
        &self.pathfinding_malus
    }

    #[must_use]
    pub const fn persistence_required(&self) -> &SyncMutex<bool> {
        &self.persistence_required
    }

    pub const fn can_pick_up_loot(&self) -> &SyncMutex<bool> {
        &self.can_pick_up_loot
    }

    const fn drop_chances(&self) -> &SyncMutex<DropChances> {
        &self.drop_chances
    }

    const fn home_restriction(&self) -> &SyncMutex<MobHomeRestriction> {
        &self.home_restriction
    }

    const fn death_loot_table(&self) -> &SyncMutex<Option<Identifier>> {
        &self.death_loot_table
    }

    const fn death_loot_table_seed(&self) -> &SyncMutex<i64> {
        &self.death_loot_table_seed
    }

    const fn leash_data(&self) -> &SyncMutex<Option<LeashData>> {
        &self.leash_data
    }

    #[must_use]
    pub fn ambient_sound_time(&self) -> i32 {
        *self.ambient_sound_time.lock()
    }

    pub fn set_ambient_sound_time(&self, ambient_sound_time: i32) {
        *self.ambient_sound_time.lock() = ambient_sound_time;
    }

    fn get_and_increment_ambient_sound_time(&self) -> i32 {
        let mut ambient_sound_time = self.ambient_sound_time.lock();
        let previous = *ambient_sound_time;
        *ambient_sound_time += 1;
        previous
    }

    #[must_use]
    pub fn xp_reward(&self) -> i32 {
        *self.xp_reward.lock()
    }

    pub fn set_xp_reward(&self, xp_reward: i32) {
        *self.xp_reward.lock() = xp_reward;
    }
}

impl Default for MobBase {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Mob: LivingEntity {
    fn mob_base(&self) -> &MobBase;

    fn mob_flags(&self) -> i8;

    fn set_mob_flags(&self, flags: i8);

    fn custom_server_ai_step(&self) {}

    fn tick_goal_selectors(&self) {}

    fn xp_reward(&self) -> i32 {
        self.mob_base().xp_reward()
    }

    fn set_xp_reward(&self, xp_reward: i32) {
        self.mob_base().set_xp_reward(xp_reward);
    }

    /// Returns vanilla `Mob.getTarget`.
    fn target(&self) -> Option<SharedEntity> {
        self.mob_base()
            .target(|target| self.is_valid_target(target))
    }

    /// Sets vanilla `Mob.target`.
    ///
    /// Returns `false` when the supplied entity is not a living entity.
    fn set_target(&self, target: Option<&SharedEntity>) -> bool {
        self.mob_base()
            .set_target(target, |target| self.is_valid_target(target))
    }

    fn is_valid_target(&self, target: &dyn LivingEntity) -> bool {
        if target
            .as_player()
            .is_some_and(|player| player.has_infinite_materials() || player.is_spectator())
        {
            return false;
        }

        Mob::can_attack(self, target)
    }

    /// Returns vanilla `Mob.canAttack`.
    fn can_attack(&self, target: &dyn LivingEntity) -> bool {
        target.entity_type() != &vanilla_entities::GHAST && LivingEntity::can_attack(self, target)
    }

    fn base_experience_reward_mob(&self) -> i32 {
        let xp_reward = self.xp_reward();
        if xp_reward <= 0 {
            return xp_reward;
        }

        let mut result = xp_reward;
        for slot in EquipmentSlot::ALL {
            if !slot.can_increase_experience() {
                continue;
            }

            let should_increase = {
                let equipment = self.living_base().equipment().lock();
                !equipment.get_ref(slot).is_empty() && self.equipment_drop_chance(slot) <= 1.0
            };
            if should_increase {
                result += 1 + rand::random_range(0..3);
            }
        }
        result
    }

    fn ambient_sound_interval(&self) -> i32 {
        if let Some(animal) = self.as_animal() {
            return animal.ambient_sound_interval_animal();
        }

        80
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        None
    }

    fn play_ambient_sound(&self) {
        self.make_sound(self.ambient_sound());
    }

    fn reset_ambient_sound_time(&self) {
        self.mob_base()
            .set_ambient_sound_time(-self.ambient_sound_interval());
    }

    /// Runs vanilla `Mob.baseTick`.
    fn base_tick_mob(&self) {
        self.base_tick_living_entity();
        self.mob_base_tick();
    }

    /// Runs the mob-owned portion of vanilla `Mob.baseTick`.
    fn mob_base_tick(&self) {
        if !LivingEntity::is_alive(self) {
            return;
        }

        let ambient_sound_time = self.mob_base().get_and_increment_ambient_sound_time();
        if rand::random_range(0..1000) < ambient_sound_time {
            self.reset_ambient_sound_time();
            self.play_ambient_sound();
        }
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_mob_base(world, spawn_reason, group_data)
    }

    fn finalize_spawn_mob_base(
        &self,
        _world: &Arc<World>,
        _spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let needs_random_spawn_bonus = !self
            .attributes()
            .lock()
            .has_modifier(vanilla_attributes::FOLLOW_RANGE, &RANDOM_SPAWN_BONUS_ID);
        let random_spawn_bonus = needs_random_spawn_bonus
            .then(|| RANDOM_SPAWN_BONUS_SCALE * (rand::random::<f64>() - rand::random::<f64>()));
        let left_handed = rand::random::<f32>() < LEFT_HANDED_SPAWN_CHANCE;

        if let Some(amount) = random_spawn_bonus {
            self.attributes().lock().add_modifier(
                vanilla_attributes::FOLLOW_RANGE,
                AttributeModifier {
                    id: RANDOM_SPAWN_BONUS_ID,
                    amount,
                    operation: AttributeModifierOperation::AddMultipliedBase,
                },
                true,
            );
        }
        self.set_left_handed(left_handed);
        group_data
    }

    /// Handles vanilla `Mob.interact`.
    fn interact_mob(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        if !LivingEntity::is_alive(self) {
            return InteractionResult::Pass;
        }

        // TODO: Handle name tags and spawn eggs once item-on-entity behavior exists.
        let interaction_result = self.interact_entity(player, hand, location);
        if interaction_result != InteractionResult::Pass {
            return interaction_result;
        }

        let interaction_result = self.mob_interact(player, hand);
        if interaction_result.consumes_action()
            && let Some(world) = self.level()
        {
            world.game_event(
                &vanilla_game_events::ENTITY_INTERACT,
                self.block_position(),
                &GameEventContext::new(Some(player), None),
            );
        }

        interaction_result
    }

    /// Handles vanilla `Mob.mobInteract`.
    fn mob_interact(&self, _player: &Player, _hand: InteractionHand) -> InteractionResult {
        InteractionResult::Pass
    }

    /// Returns vanilla `Mob.canShearEquipment`.
    fn can_shear_equipment(&self, _player: &Player) -> bool {
        !self.is_vehicle()
    }

    /// Applies vanilla `Mob.usePlayerItem`.
    fn use_player_item(&self, player: &Player, hand: InteractionHand) {
        player.inventory.lock().shrink_item_in_hand(hand, 1);
        // TODO: Apply USE_REMAINDER components once item use-remainder support exists.
    }

    fn remove_when_far_away(&self, _dist_sqr: f64) -> bool {
        true
    }

    fn requires_custom_persistence(&self) -> bool {
        self.is_passenger() || self.is_leashed()
    }

    fn is_persistence_required(&self) -> bool {
        *self.mob_base().persistence_required().lock()
    }

    fn set_persistence_required(&self) {
        *self.mob_base().persistence_required().lock() = true;
    }

    /// Returns vanilla `Mob.canPickUpLoot`.
    fn can_pick_up_loot(&self) -> bool {
        *self.mob_base().can_pick_up_loot().lock()
    }

    fn set_can_pick_up_loot(&self, can_pick_up_loot: bool) {
        *self.mob_base().can_pick_up_loot().lock() = can_pick_up_loot;
    }

    fn equipment_drop_chance(&self, slot: EquipmentSlot) -> f32 {
        self.mob_base().drop_chances().lock().by_equipment(slot)
    }

    fn is_equipment_drop_preserved(&self, slot: EquipmentSlot) -> bool {
        self.mob_base().drop_chances().lock().is_preserved(slot)
    }

    fn set_guaranteed_drop(&self, slot: EquipmentSlot) {
        self.mob_base()
            .drop_chances()
            .lock()
            .set_guaranteed_drop(slot);
    }

    fn drop_custom_death_loot_mob(&self, _source: &DamageSource, killed_by_player: bool) {
        if self.level().is_none() {
            return;
        }

        for slot in EquipmentSlot::ALL {
            let drop_chance = self.equipment_drop_chance(slot);
            let preserve = self.is_equipment_drop_preserved(slot);
            if !can_attempt_equipment_drop(drop_chance, preserve, killed_by_player) {
                continue;
            }

            let can_drop_item = {
                let equipment = self.living_base().equipment().lock();
                let item_stack = equipment.get_ref(slot);
                !item_stack.is_empty()
                    && !item_stack
                        .has_enchantment_effect(EnchantmentEffectComponent::PreventEquipmentDrop)
            };
            if !can_drop_item {
                continue;
            }

            // TODO: Apply EquipmentDrops enchantment value effects once damage
            // sources can resolve their living attacker context.
            let random_roll = rand::random::<f32>();
            if random_roll >= drop_chance {
                continue;
            }

            let mut item_stack = {
                let mut equipment = self.living_base().equipment().lock();
                let item_stack = equipment.get_ref(slot);
                if item_stack.is_empty()
                    || item_stack
                        .has_enchantment_effect(EnchantmentEffectComponent::PreventEquipmentDrop)
                {
                    continue;
                }

                equipment.take(slot)
            };
            if !preserve && item_stack.is_damageable_item() {
                let max_damage = item_stack.get_max_damage();
                let inner = rand::random_range(0..(max_damage - 3).max(1));
                let damage = max_damage - rand::random_range(0..=inner);
                item_stack.set_damage_value(damage);
            }

            self.spawn_at_location(item_stack, 0.0);
        }
    }

    fn save_mob(&self, nbt: &mut NbtCompound) {
        nbt.insert("CanPickUpLoot", i8::from(self.can_pick_up_loot()));
        nbt.insert(
            "PersistenceRequired",
            i8::from(self.is_persistence_required()),
        );
        self.mob_base().drop_chances().lock().save(nbt);
        if let Some(leash_data) = self.mob_base().leash_data().lock().as_ref() {
            leash_data.save(nbt);
        }

        if self.has_home() {
            let home = *self.mob_base().home_restriction().lock();
            nbt.insert("home_radius", home.radius);
            nbt.insert(
                "home_pos",
                NbtTag::IntArray(vec![
                    home.position.x(),
                    home.position.y(),
                    home.position.z(),
                ]),
            );
        }

        nbt.insert("LeftHanded", i8::from(self.is_left_handed()));
        if let Some(loot_table) = self.mob_base().death_loot_table().lock().as_ref() {
            nbt.insert("DeathLootTable", loot_table.to_string());
        }
        let loot_table_seed = *self.mob_base().death_loot_table_seed().lock();
        if loot_table_seed != 0 {
            nbt.insert("DeathLootTableSeed", loot_table_seed);
        }
        if self.is_no_ai() {
            nbt.insert("NoAI", i8::from(true));
        }
    }

    fn load_mob(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_can_pick_up_loot(nbt.byte("CanPickUpLoot").is_some_and(|value| value != 0));
        *self.mob_base().persistence_required().lock() = nbt
            .byte("PersistenceRequired")
            .is_some_and(|value| value != 0);
        *self.mob_base().drop_chances().lock() = DropChances::load(nbt);
        *self.mob_base().leash_data().lock() = LeashData::load(nbt);
        let home_radius = nbt.int("home_radius").unwrap_or(-1);
        if home_radius >= 0 {
            let home_position = nbt
                .int_array("home_pos")
                .filter(|position| position.len() == 3)
                .map_or(BlockPos::ZERO, |position| {
                    BlockPos::new(position[0], position[1], position[2])
                });
            self.set_home_to(home_position, home_radius);
        } else {
            self.clear_home();
        }

        self.set_left_handed(nbt.byte("LeftHanded").is_some_and(|value| value != 0));
        let death_loot_table = nbt
            .string("DeathLootTable")
            .and_then(|loot_table| loot_table.to_str().as_ref().parse().ok());
        *self.mob_base().death_loot_table().lock() = death_loot_table;
        *self.mob_base().death_loot_table_seed().lock() =
            nbt.long("DeathLootTableSeed").unwrap_or(0);
        self.set_no_ai(nbt.byte("NoAI").is_some_and(|value| value != 0));
    }

    fn set_death_loot_table(&self, loot_table: Option<Identifier>) {
        *self.mob_base().death_loot_table().lock() = loot_table;
    }

    fn set_death_loot_table_seed(&self, seed: i64) {
        *self.mob_base().death_loot_table_seed().lock() = seed;
    }

    fn custom_death_loot_table(&self) -> Option<LootTableRef> {
        self.mob_base()
            .death_loot_table()
            .lock()
            .as_ref()
            .and_then(|key| REGISTRY.loot_tables.by_key(key))
    }

    fn has_custom_death_loot_table(&self) -> bool {
        self.mob_base().death_loot_table().lock().is_some()
    }

    fn death_loot_table_seed(&self) -> i64 {
        *self.mob_base().death_loot_table_seed().lock()
    }

    fn clear_custom_death_loot_table(&self) {
        *self.mob_base().death_loot_table().lock() = None;
    }

    fn is_leashed(&self) -> bool {
        self.leash_holder().is_some()
    }

    fn may_be_leashed(&self) -> bool {
        self.mob_base().leash_data().lock().is_some()
    }

    fn leash_holder(&self) -> Option<SharedEntity> {
        self.mob_base()
            .leash_data()
            .lock()
            .as_ref()
            .and_then(LeashData::holder)
    }

    fn leash_attachment(&self) -> Option<LeashAttachment> {
        self.mob_base()
            .leash_data()
            .lock()
            .as_ref()
            .map(LeashData::saved_attachment)
    }

    fn set_delayed_leash_attachment(&self, attachment: LeashAttachment) {
        *self.mob_base().leash_data().lock() = Some(LeashData::from_delayed_attachment(attachment));
    }

    fn can_be_leashed(&self) -> bool {
        // TODO: Return false for enemy mobs once hostile mob foundations exist.
        true
    }

    fn leash_distance_to(&self, holder: &dyn Entity) -> f64 {
        leash_bounding_box_center(self.as_entity_event_source())
            .distance(leash_bounding_box_center(holder))
    }

    fn leash_snap_distance(&self) -> f64 {
        LEASH_SNAP_DISTANCE
    }

    fn leash_elastic_distance(&self) -> f64 {
        LEASH_ELASTIC_DISTANCE
    }

    fn when_leashed_to(&self, holder: &dyn Entity) {
        holder.notify_leash_holder(self.as_entity_event_source());
    }

    fn leash_too_far_behaviour(&self) {
        self.drop_leash();
    }

    fn on_elastic_leash_pull(&self) {
        self.check_fall_distance_accumulation();
    }

    fn close_range_leash_behaviour(&self, _holder: &dyn Entity) {}

    fn check_elastic_interactions(&self, holder: &dyn Entity) -> bool {
        let Some(wrench) = compute_elastic_interaction(
            self.as_entity_event_source(),
            holder,
            self.leash_elastic_distance(),
        ) else {
            return false;
        };

        {
            let mut leash_data = self.mob_base().leash_data().lock();
            let Some(leash_data) = leash_data.as_mut() else {
                return false;
            };
            leash_data.angular_momentum += LEASH_TORSIONAL_ELASTICITY * wrench.torque;
        }

        let relative_velocity_to_leasher =
            leash_holder_movement(holder) - leash_holder_movement(self.as_entity_event_source());
        self.push_impulse(
            axis_specific_leash_elasticity(wrench.force)
                + relative_velocity_to_leasher * LEASH_STIFFNESS,
        );
        true
    }

    fn apply_leash_angular_momentum(&self) -> bool {
        let angular_friction = self.leash_angular_friction();
        let angular_momentum = {
            let mut leash_data = self.mob_base().leash_data().lock();
            let Some(leash_data) = leash_data.as_mut() else {
                return false;
            };
            let angular_momentum = leash_data.angular_momentum;
            leash_data.angular_momentum *= angular_friction;
            angular_momentum
        };
        self.rotate_by_leash_angular_momentum(angular_momentum);
        true
    }

    fn rotate_by_leash_angular_momentum(&self, angular_momentum: f64) {
        let (yaw, pitch) = self.rotation();
        self.set_rotation((yaw - angular_momentum as f32, pitch));
    }

    fn leash_angular_momentum(&self) -> Option<f64> {
        self.mob_base()
            .leash_data()
            .lock()
            .as_ref()
            .map(|leash_data| leash_data.angular_momentum)
    }

    fn leash_angular_friction(&self) -> f64 {
        if self.on_ground() {
            let Some(world) = self.level() else {
                return 0.91;
            };
            let Some(pos) = self.block_pos_below_that_affects_movement() else {
                return 0.91;
            };
            return f64::from(world.get_block_state(pos).get_block().config.friction * 0.91);
        }

        if self.is_in_water() || self.is_in_lava() {
            return 0.8;
        }

        0.91
    }

    fn can_have_a_leash_attached_to(&self, holder: &dyn Entity) -> bool {
        self.id() != holder.id()
            && self.leash_distance_to(holder) <= self.leash_snap_distance()
            && self.can_be_leashed()
    }

    fn set_leashed_to(&self, holder: &SharedEntity) -> bool {
        if self.id() == holder.id() {
            return false;
        }

        let old_holder = self.leash_holder();
        {
            let mut leash_data = self.mob_base().leash_data().lock();
            if let Some(leash_data) = leash_data.as_mut() {
                leash_data.set_holder(holder);
            } else {
                *leash_data = Some(LeashData::from_entity(holder));
            }
        }

        if self.is_passenger() {
            self.stop_riding();
        }
        if let Some(old_holder) = old_holder
            && old_holder.id() != holder.id()
        {
            old_holder.notify_leashee_removed(self.as_entity_event_source());
        }
        true
    }

    fn tick_leash(&self) {
        if let Some(holder) = self.leash_holder() {
            if !self.can_interact_with_level() || !holder.can_interact_with_level() {
                if let Some(world) = self.level()
                    && world.get_game_rule(&ENTITY_DROPS).as_bool() == Some(true)
                {
                    self.drop_leash();
                } else {
                    self.remove_leash();
                }
                return;
            }

            let distance_to = self.leash_distance_to(holder.as_ref());
            self.when_leashed_to(holder.as_ref());
            let angular_momentum_before_distance_action = self.leash_angular_momentum();
            if distance_to > self.leash_snap_distance() {
                if let Some(world) = self.level() {
                    world.play_sound_at(
                        &sound_events::ITEM_LEAD_BREAK,
                        SoundSource::Neutral,
                        holder.position(),
                        1.0,
                        1.0,
                        None,
                    );
                }
                self.leash_too_far_behaviour();
            } else if distance_to
                > self.leash_elastic_distance()
                    - f64::from(holder.base().dimensions().width)
                    - f64::from(self.base().dimensions().width)
                && self.check_elastic_interactions(holder.as_ref())
            {
                self.on_elastic_leash_pull();
            } else {
                self.close_range_leash_behaviour(holder.as_ref());
            }
            if !self.apply_leash_angular_momentum()
                && let Some(angular_momentum) = angular_momentum_before_distance_action
            {
                self.rotate_by_leash_angular_momentum(angular_momentum);
            }
            return;
        }

        let Some(attachment) = self.leash_attachment() else {
            return;
        };

        let Some(world) = self.level() else {
            return;
        };

        match attachment {
            LeashAttachment::Entity(uuid) => {
                if let Some(holder) = world.get_entity_by_uuid(&uuid) {
                    let _ = self.set_leashed_to(&holder);
                    return;
                }

                if self.tick_count() > DELAYED_LEASH_DROP_TICKS {
                    let _ = self.spawn_at_location(ItemStack::new(&vanilla_items::ITEMS.lead), 0.0);
                    self.remove_leash_state();
                }
            }
            LeashAttachment::FenceKnot(pos) => {
                if let Some(holder) = LeashFenceKnotEntity::get_or_create_knot(&world, pos) {
                    let _ = self.set_leashed_to(&holder);
                    return;
                }

                if self.tick_count() > DELAYED_LEASH_DROP_TICKS {
                    let _ = self.spawn_at_location(ItemStack::new(&vanilla_items::ITEMS.lead), 0.0);
                    self.remove_leash_state();
                }
            }
        }
    }

    fn drop_leash(&self) {
        if self.leash_holder().is_none() {
            return;
        }

        let holder = self.remove_leash_state();
        let _ = self.spawn_at_location(ItemStack::new(&vanilla_items::ITEMS.lead), 0.0);
        if let Some(holder) = holder {
            holder.notify_leashee_removed(self.as_entity_event_source());
        }
    }

    fn remove_leash(&self) {
        if self.leash_holder().is_some()
            && let Some(holder) = self.remove_leash_state()
        {
            holder.notify_leashee_removed(self.as_entity_event_source());
        }
    }

    fn remove_leash_state(&self) -> Option<SharedEntity> {
        self.mob_base()
            .leash_data()
            .lock()
            .take()
            .and_then(|leash_data| leash_data.holder())
    }

    fn is_within_home(&self) -> bool {
        self.is_within_home_pos(self.block_position())
    }

    fn is_within_home_pos(&self, pos: BlockPos) -> bool {
        let home = *self.mob_base().home_restriction().lock();
        home.radius == -1
            || block_pos_distance_sqr(home.position, pos) < home_radius_sqr(home.radius)
    }

    fn is_within_home_vec(&self, pos: DVec3) -> bool {
        let home = *self.mob_base().home_restriction().lock();
        home.radius == -1
            || block_center_distance_sqr(home.position, pos) < home_radius_sqr(home.radius)
    }

    fn set_home_to(&self, position: BlockPos, radius: i32) {
        *self.mob_base().home_restriction().lock() = MobHomeRestriction { position, radius };
    }

    fn home_position(&self) -> BlockPos {
        self.mob_base().home_restriction().lock().position
    }

    fn home_radius(&self) -> i32 {
        self.mob_base().home_restriction().lock().radius
    }

    fn clear_home(&self) {
        self.mob_base().home_restriction().lock().radius = -1;
    }

    fn has_home(&self) -> bool {
        self.home_radius() != -1
    }

    fn check_mob_despawn(&self) {
        if self
            .level()
            .is_some_and(|world| world.difficulty() == Difficulty::Peaceful)
            && !self.entity_type().allowed_in_peaceful
        {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        if self.is_persistence_required() || self.requires_custom_persistence() {
            self.set_no_action_time(0);
            return;
        }

        let Some(nearest_player_dist_sqr) = self.nearest_player_distance_sqr() else {
            return;
        };

        let mob_category = self.entity_type().mob_category;
        let despawn_distance = mob_category.despawn_distance();
        let despawn_distance_sqr = despawn_distance * despawn_distance;
        if nearest_player_dist_sqr > f64::from(despawn_distance_sqr)
            && self.remove_when_far_away(nearest_player_dist_sqr)
        {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        let no_despawn_distance = mob_category.no_despawn_distance();
        let no_despawn_distance_sqr = no_despawn_distance * no_despawn_distance;
        if self.no_action_time() > 600
            && nearest_player_dist_sqr > f64::from(no_despawn_distance_sqr)
            && self.remove_when_far_away(nearest_player_dist_sqr)
        {
            let should_discard = rand::random_range(0..800) == 0;
            if should_discard {
                self.set_removed(RemovalReason::Discarded);
            }
        } else if nearest_player_dist_sqr < f64::from(no_despawn_distance_sqr) {
            self.set_no_action_time(0);
        }
    }

    fn nearest_player_distance_sqr(&self) -> Option<f64> {
        let world = self.level()?;
        world.nearest_player_distance_sqr(self.position())
    }

    fn controlling_passenger_mob(&self) -> Option<SharedEntity> {
        let first_passenger = self.first_passenger()?;
        if self.is_no_ai() || !first_passenger.is_mob() || !first_passenger.can_control_vehicle() {
            return None;
        }

        Some(first_passenger)
    }

    fn get_pathfinding_malus(&self, path_type: PathType) -> f32 {
        self.mob_base().pathfinding_malus().lock().get(path_type)
    }

    /// Vanilla `Entity.getMaxFallDistance` baseline.
    fn max_fall_distance(&self) -> i32 {
        3
    }

    fn set_pathfinding_malus(&self, path_type: PathType, malus: f32) {
        self.mob_base()
            .pathfinding_malus()
            .lock()
            .set(path_type, malus);
    }

    fn is_no_ai(&self) -> bool {
        self.mob_flags() & MOB_FLAG_NO_AI != 0
    }

    fn set_no_ai(&self, no_ai: bool) {
        self.set_mob_flag(MOB_FLAG_NO_AI, no_ai);
    }

    fn is_left_handed(&self) -> bool {
        self.mob_flags() & MOB_FLAG_LEFT_HANDED != 0
    }

    fn set_left_handed(&self, left_handed: bool) {
        self.set_mob_flag(MOB_FLAG_LEFT_HANDED, left_handed);
    }

    fn is_aggressive(&self) -> bool {
        self.mob_flags() & MOB_FLAG_AGGRESSIVE != 0
    }

    /// Returns vanilla `Mob.getMaxHeadXRot`.
    fn max_head_x_rot(&self) -> f32 {
        40.0
    }

    /// Returns vanilla `Mob.getMaxHeadYRot`.
    fn max_head_y_rot(&self) -> f32 {
        75.0
    }

    /// Handles vanilla `Mob.doHurtTarget`.
    #[must_use]
    fn do_hurt_target(&self, target: &SharedEntity) -> bool {
        let Some(attacker) = self.as_entity_event_source().as_living_entity() else {
            return false;
        };
        LivingEntity::refresh_equipment_attribute_modifiers(self, EquipmentSlot::MainHand);
        let weapon_item = {
            let mut main_hand = ItemStack::empty();
            self.with_equipment_slot(EquipmentSlot::MainHand, &mut |item_stack| {
                main_hand = item_stack.copy_with_count(item_stack.count());
            });
            main_hand
        };
        let attack_damage = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::ATTACK_DAMAGE) as f32;
        let damage_source = self.mob_attack_damage_source(&weapon_item, attacker);
        let enchantment_context = EnchantmentDamageContext::new(
            target.entity_type(),
            Some(self.entity_type()),
            Some(self.entity_type()),
            &damage_source,
        );
        let mut damage =
            enchantment_helper::modify_damage(&weapon_item, &enchantment_context, attack_damage);
        damage += ITEM_BEHAVIORS
            .get_behavior(weapon_item.item())
            .get_attack_damage_bonus(attacker, target.as_ref(), damage, &damage_source);

        let old_movement = target.velocity();
        let was_hurt = target.hurt(&damage_source, damage);
        if was_hurt {
            self.cause_extra_knockback(
                target.as_ref(),
                self.get_attack_knockback(target.as_ref(), &weapon_item, &damage_source),
                old_movement,
            );
            self.with_equipment_slot_mut(EquipmentSlot::MainHand, &mut |stack| {
                if stack.is_empty() {
                    return;
                }
                if let Some(living_target) = target.as_living_entity() {
                    ITEM_BEHAVIORS.get_behavior(stack.item()).hurt_enemy(
                        stack,
                        living_target,
                        attacker,
                    );
                }
            });
            let post_attack_context = EnchantmentPostAttackContext::new(
                target.as_ref(),
                Some(self.as_entity_event_source()),
                Some(self.as_entity_event_source()),
                &damage_source,
            );
            enchantment_helper::do_post_attack_effects_from_item(
                &weapon_item,
                &post_attack_context,
            );
            self.set_last_hurt_mob(Some(target));
            self.play_attack_sound();
        }

        if let Some(user) = self.as_entity_event_source().as_living_entity() {
            enchantment_helper::do_post_piercing_attack_effects(user);
        }
        was_hurt
    }

    /// Returns the damage source used by vanilla `ItemStack.getDamageSource`.
    fn mob_attack_damage_source(
        &self,
        weapon_item: &ItemStack,
        attacker: &dyn LivingEntity,
    ) -> DamageSource {
        let damage_source = if let Some(damage_type) = weapon_item.get_damage_type() {
            DamageSource::environment(damage_type)
        } else {
            ITEM_BEHAVIORS
                .get_behavior(weapon_item.item())
                .get_item_damage_source(attacker)
                .unwrap_or_else(|| DamageSource::environment(&vanilla_damage_types::MOB_ATTACK))
        };

        damage_source
            .with_causing_entity(self.id())
            .with_direct_entity(self.id())
            .with_source_position(self.position())
    }

    /// Returns vanilla `LivingEntity.getKnockback` for mob attacks.
    fn get_attack_knockback(
        &self,
        target: &dyn Entity,
        weapon_item: &ItemStack,
        damage_source: &DamageSource,
    ) -> f64 {
        let attack_knockback = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::ATTACK_KNOCKBACK);
        let enchantment_context = EnchantmentDamageContext::new(
            target.entity_type(),
            Some(self.entity_type()),
            Some(self.entity_type()),
            damage_source,
        );
        let modified = enchantment_helper::modify_knockback(
            weapon_item,
            &enchantment_context,
            attack_knockback as f32,
        );
        f64::from(modified) / 2.0
    }

    /// Applies vanilla `LivingEntity.causeExtraKnockback`.
    fn cause_extra_knockback(
        &self,
        target: &dyn Entity,
        knockback_amount: f64,
        _old_movement: DVec3,
    ) {
        if knockback_amount <= 0.0 {
            return;
        }
        let Some(living_target) = target.as_living_entity() else {
            return;
        };

        let yaw_radians = self.rotation().0.to_radians();
        let yaw_sin = f64::from(yaw_radians.sin());
        let yaw_cos = f64::from(yaw_radians.cos());
        living_target.knockback(knockback_amount, yaw_sin, -yaw_cos);

        let velocity = self.velocity();
        self.set_velocity(DVec3::new(velocity.x * 0.6, velocity.y, velocity.z * 0.6));
    }

    /// Plays vanilla `LivingEntity.playAttackSound`.
    fn play_attack_sound(&self) {}

    /// Returns vanilla `Mob.isWithinMeleeAttackRange`.
    fn is_within_melee_attack_range(&self, target: &dyn LivingEntity) -> bool {
        // TODO: Use the held item's ATTACK_RANGE component once it has typed component data.
        let max_range = default_attack_reach();
        let min_range = 0.0;
        let target_hitbox = target.bounding_box();
        self.attack_bounding_box(max_range)
            .intersects(target_hitbox)
            && (min_range <= 0.0
                || !self
                    .attack_bounding_box(min_range)
                    .intersects(target_hitbox))
    }

    /// Returns vanilla `Mob.getAttackBoundingBox`.
    fn attack_bounding_box(&self, horizontal_expansion: f64) -> WorldAabb {
        let own_aabb = self.bounding_box();
        let base = if let Some(vehicle) = self.vehicle() {
            let mount_aabb = vehicle.bounding_box();
            WorldAabb::new(
                own_aabb.min_x().min(mount_aabb.min_x()),
                own_aabb.min_y(),
                own_aabb.min_z().min(mount_aabb.min_z()),
                own_aabb.max_x().max(mount_aabb.max_x()),
                own_aabb.max_y(),
                own_aabb.max_z().max(mount_aabb.max_z()),
            )
        } else {
            own_aabb
        };

        base.inflate_xyz(horizontal_expansion, 0.0, horizontal_expansion)
    }

    fn set_aggressive(&self, aggressive: bool) {
        self.set_mob_flag(MOB_FLAG_AGGRESSIVE, aggressive);
    }

    fn set_mob_flag(&self, flag: i8, enabled: bool) {
        let flags = self.mob_flags();
        let next = if enabled { flags | flag } else { flags & !flag };
        self.set_mob_flags(next);
    }

    fn controlled_mob_vehicle(&self) -> Option<SharedEntity> {
        let vehicle = self.vehicle()?;
        if vehicle
            .controlling_passenger()
            .is_none_or(|passenger| passenger.id() != self.id())
        {
            return None;
        }
        vehicle.as_mob()?;
        Some(vehicle)
    }

    fn set_wanted_position(&self, position: DVec3, speed_modifier: f64) {
        if let Some(vehicle) = self.controlled_mob_vehicle()
            && let Some(mob) = vehicle.as_mob()
        {
            mob.set_wanted_position(position, speed_modifier);
            return;
        }

        self.mob_base()
            .controls()
            .lock()
            .move_control
            .set_wanted_position(position, speed_modifier);
    }

    fn jump_control_jump(&self) {
        self.mob_base().controls().lock().jump_control.jump();
    }

    /// Mirrors vanilla `Mob.setSpeed`: update cached speed and forward AI input.
    fn set_mob_speed(&self, speed: f32) {
        self.set_speed(speed);
        let input = self.travel_input();
        self.set_travel_input(LivingTravelInput::new(
            input.sideways(),
            input.vertical(),
            speed,
        ));
    }

    fn mob_server_ai_step(&self) {
        self.increment_no_action_time();
        self.mob_base().sensing().lock().tick();
        if self.tick_count() % 5 == 0 {
            self.update_control_flags();
        }
        self.tick_goal_selectors();
        self.tick_path_navigation();
        self.custom_server_ai_step();
        self.tick_move_control();
        self.tick_look_control();
        self.tick_jump_control();
    }

    fn tick_path_navigation(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let game_time = world.game_time();
        self.mob_base().navigation().lock().tick();
        tick_path_navigation_target(self, &world, game_time, true);
    }

    fn tick_move_control(&self) {
        let move_control = {
            let mut controls = self.mob_base().controls().lock();
            let move_control = controls.move_control;
            if matches!(move_control.operation(), MoveControlOperation::MoveTo) {
                controls.move_control.set_wait();
            }
            move_control
        };

        match move_control.operation() {
            MoveControlOperation::Wait => {
                let input = self.travel_input();
                self.set_travel_input(LivingTravelInput::new(
                    input.sideways(),
                    input.vertical(),
                    0.0,
                ));
            }
            MoveControlOperation::MoveTo => self.tick_move_to_control(
                move_control.wanted_position(),
                move_control.speed_modifier(),
            ),
            MoveControlOperation::Strafe => {
                self.tick_strafe_control(
                    move_control.strafe_forward(),
                    move_control.strafe_right(),
                );
            }
            MoveControlOperation::Jumping => {
                self.tick_jumping_control(move_control.speed_modifier());
            }
        }
    }

    fn tick_move_to_control(&self, wanted_position: DVec3, speed_modifier: f64) {
        let position = self.position();
        let xd = wanted_position.x - position.x;
        let yd = wanted_position.y - position.y;
        let zd = wanted_position.z - position.z;
        let dd = xd * xd + yd * yd + zd * zd;
        if dd < MOVE_CONTROL_MIN_SPEED_SQR {
            let input = self.travel_input();
            self.set_travel_input(LivingTravelInput::new(
                input.sideways(),
                input.vertical(),
                0.0,
            ));
            return;
        }

        let y_rot = (zd.atan2(xd) as f32 * 180.0 / PI) - 90.0;
        let (_, pitch) = self.rotation();
        self.set_rotation((
            rotlerp(self.rotation().0, y_rot, MOVE_CONTROL_MAX_TURN),
            pitch,
        ));
        let movement_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED);
        self.set_mob_speed((speed_modifier * movement_speed) as f32);

        if should_jump_to_wanted_position(self, xd, yd, zd) {
            self.jump_control_jump();
            self.mob_base().controls().lock().move_control.set_jumping();
        }
    }

    fn tick_strafe_control(&self, forward: f32, right: f32) {
        let movement_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED) as f32;
        let speed = movement_speed * 0.25;
        let mut strafe_forward = forward;
        let mut strafe_right = right;

        let mut distance = strafe_forward
            .mul_add(strafe_forward, strafe_right * strafe_right)
            .sqrt();
        if distance < 1.0 {
            distance = 1.0;
        }
        distance = speed / distance;
        let xa = strafe_forward * distance;
        let za = strafe_right * distance;
        let yaw_radians = self.rotation().0 * PI / 180.0;
        let sin = yaw_radians.sin();
        let cos = yaw_radians.cos();
        let dx = xa.mul_add(cos, -(za * sin));
        let dz = za.mul_add(cos, xa * sin);
        if !self.is_strafe_walkable(dx, dz) {
            strafe_forward = 1.0;
            strafe_right = 0.0;
        }

        self.set_speed(speed);
        self.set_travel_input(LivingTravelInput::new(strafe_right, 0.0, strafe_forward));
        self.mob_base().controls().lock().move_control.set_wait();
    }

    fn is_strafe_walkable(&self, dx: f32, dz: f32) -> bool {
        let Some(world) = self.level() else {
            return true;
        };
        let position = self.position();
        let pos = BlockPos::new(
            fast_floor(position.x + f64::from(dx)),
            fast_floor(position.y),
            fast_floor(position.z + f64::from(dz)),
        );
        let mut context = PathfindingContext::new(world.as_ref(), self.block_position());
        WalkPathEvaluator::path_type_static(&mut context, pos) == PathType::Walkable
    }

    fn tick_jumping_control(&self, speed_modifier: f64) {
        let movement_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED);
        self.set_mob_speed((speed_modifier * movement_speed) as f32);
        if self.on_ground()
            || (self.is_in_water() || self.is_in_lava()) && self.is_affected_by_fluids()
        {
            self.mob_base().controls().lock().move_control.set_wait();
        }
    }

    fn tick_look_control(&self) {
        let look_control = {
            let mut controls = self.mob_base().controls().lock();
            let look_control = controls.look_control;
            controls.look_control.tick_cooldown();
            look_control
        };

        let mut rotation = self.rotation();
        rotation.1 = 0.0;
        if look_control.is_looking_at_target() {
            let position = self.position();
            let wanted_position = look_control.wanted_position();
            let xd = wanted_position.x - position.x;
            let yd = wanted_position.y - self.get_eye_y();
            let zd = wanted_position.z - position.z;
            let horizontal = xd.hypot(zd);
            if horizontal.abs() > 1.0e-5 || yd.abs() > 1.0e-5 {
                let target_pitch = -(yd.atan2(horizontal)) as f32 * 180.0 / PI;
                rotation.1 =
                    rotate_towards(rotation.1, target_pitch, look_control.x_max_rot_angle());
            }
            if zd.abs() > 1.0e-5 || xd.abs() > 1.0e-5 {
                let target_yaw = (zd.atan2(xd) as f32 * 180.0 / PI) - 90.0;
                self.set_y_head_rot(rotate_towards(
                    self.y_head_rot(),
                    target_yaw,
                    look_control.y_max_rot_speed(),
                ));
            }
        } else {
            self.set_y_head_rot(rotate_towards(self.y_head_rot(), self.y_body_rot(), 10.0));
        }

        self.set_rotation(rotation);
        self.clamp_head_rotation_to_body_when_pathing();
    }

    fn clamp_head_rotation_to_body_when_pathing(&self) {
        if self.mob_base().navigation().lock().is_done() {
            return;
        }

        self.set_y_head_rot(rotate_if_necessary(
            self.y_head_rot(),
            self.y_body_rot(),
            self.max_head_y_rot(),
        ));
    }

    fn tick_jump_control(&self) {
        let jumping = self.mob_base().controls().lock().jump_control.tick();
        self.set_jumping(jumping);
    }

    fn update_control_flags(&self) {
        let no_controller = self
            .controlling_passenger()
            .is_none_or(|passenger| !passenger.is_mob());
        let not_in_boat = self
            .vehicle()
            .is_none_or(|vehicle| !vehicle.entity_type().is_abstract_boat);

        let mut selector = self.mob_base().goal_selector().lock();
        selector.set_control(GoalControl::Move, no_controller);
        selector.set_control(GoalControl::Jump, no_controller && not_in_boat);
        selector.set_control(GoalControl::Look, no_controller);
    }

    fn tick_body_rotation_control(&self) {
        let moving = {
            let delta = self.position() - self.old_position();
            delta.x.mul_add(delta.x, delta.z * delta.z) > BODY_ROTATION_MOVING_DISTANCE_SQR
        };
        let carrying_mob_passenger = self
            .first_passenger()
            .is_some_and(|passenger| passenger.is_mob());
        let input = BodyRotationInput::new(
            moving,
            carrying_mob_passenger,
            self.rotation().0,
            self.y_body_rot(),
            self.y_head_rot(),
            self.max_head_y_rot(),
        );
        let update = self
            .mob_base()
            .controls()
            .lock()
            .body_rotation_control
            .tick(input);
        self.set_y_body_rot(update.y_body_rot());
        self.set_y_head_rot(update.y_head_rot());
    }
}

fn can_attempt_equipment_drop(drop_chance: f32, preserve: bool, killed_by_player: bool) -> bool {
    drop_chance != 0.0 && (killed_by_player || preserve)
}

fn default_attack_reach() -> f64 {
    f64::from(DEFAULT_ATTACK_REACH_BASE).sqrt() - f64::from(DEFAULT_ATTACK_REACH_OFFSET)
}

fn tick_path_navigation_target<M: Mob + ?Sized>(
    mob: &M,
    world: &Arc<World>,
    game_time: i64,
    can_update_path: bool,
) {
    let (target, speed_modifier) = {
        let mut navigation = mob.mob_base().navigation().lock();
        let mob_position =
            ground_navigation_temp_mob_pos(mob, world.as_ref(), navigation.can_float());
        let context = NavigationTickContext {
            mob_position,
            mob_bounding_box_width: mob.bounding_box().width(),
            mob_speed: mob.get_speed(),
            game_time,
        };
        let next_target = if can_update_path {
            navigation.next_move_target(context)
        } else {
            navigation.next_move_target_without_path_update(context, mob.on_ground())
        };
        let Some(target) = next_target else {
            return;
        };
        target
    };

    let target_pos = BlockPos::containing(target.x, target.y, target.z);
    let ground_y = if world.get_block_state(target_pos.below()).is_air() {
        target.y
    } else {
        WalkNodeEvaluator::floor_level(world.as_ref(), target_pos)
    };
    mob.set_wanted_position(DVec3::new(target.x, ground_y, target.z), speed_modifier);
}

fn ground_navigation_temp_mob_pos<M: Mob + ?Sized>(
    mob: &M,
    world: &World,
    can_float: bool,
) -> DVec3 {
    let position = mob.position();
    DVec3::new(
        position.x,
        f64::from(ground_navigation_surface_y(mob, world, can_float)),
        position.z,
    )
}

fn ground_navigation_surface_y<M: Mob + ?Sized>(mob: &M, world: &World, can_float: bool) -> i32 {
    if !mob.is_in_water() || !can_float {
        return fast_floor(mob.position().y + 0.5);
    }

    let position = mob.position();
    let block_y = mob.block_position().y();
    let mut surface = block_y;
    let mut state = world.get_block_state(BlockPos::containing(
        position.x,
        f64::from(surface),
        position.z,
    ));
    let mut steps = 0;
    while state.get_block() == &vanilla_blocks::WATER {
        surface += 1;
        state = world.get_block_state(BlockPos::containing(
            position.x,
            f64::from(surface),
            position.z,
        ));
        steps += 1;
        if steps > 16 {
            return block_y;
        }
    }

    surface
}

pub trait PathfinderMob: Mob {
    fn controlled_pathfinder_vehicle(&self) -> Option<SharedEntity> {
        let vehicle = self.controlled_mob_vehicle()?;
        vehicle.as_pathfinder_mob()?;
        Some(vehicle)
    }

    fn get_walk_target_value(&self, pos: BlockPos) -> f32 {
        self.as_animal()
            .map_or(0.0, |animal| animal.animal_walk_target_value(pos))
    }

    fn has_line_of_sight_cached(&self, target: &dyn Entity) -> bool {
        self.mob_base()
            .sensing()
            .lock()
            .has_line_of_sight(target.id(), || self.has_line_of_sight(target))
    }

    fn can_update_path(&self) -> bool {
        self.on_ground() || self.is_in_water() || self.is_in_lava() || self.is_passenger()
    }

    fn can_path_to_targets_below_surface(&self) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.can_path_to_targets_below_surface();
        }

        self.mob_base()
            .navigation()
            .lock()
            .can_path_to_targets_below_surface()
    }

    fn can_reach_living_target(&self, target: &dyn LivingEntity) -> bool {
        let target_pos = target.block_position();
        self.create_path_to(target_pos, 0)
            .is_some_and(|path| path_end_node_can_reach_target(&path, target_pos))
    }

    fn tick_pathfinder_path_navigation(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let game_time = world.game_time();
        let recompute_request = {
            let mut navigation = self.mob_base().navigation().lock();
            navigation.tick();
            navigation.take_delayed_recompute_request(game_time, self.can_update_path())
        };
        if let Some(request) = recompute_request {
            self.recompute_path(request);
        }

        tick_path_navigation_target(self, &world, game_time, self.can_update_path());
    }

    fn tick_pathfinder_goal_selectors(&self)
    where
        Self: Sized,
    {
        let id_based_tick_count = self.tick_count().wrapping_add(self.id());
        if id_based_tick_count % 2 != 0 && self.tick_count() > 1 {
            self.mob_base()
                .target_selector()
                .lock()
                .tick_running_goals(self, false);
            self.mob_base()
                .goal_selector()
                .lock()
                .tick_running_goals(self, false);
        } else {
            self.mob_base().target_selector().lock().tick(self);
            self.mob_base().goal_selector().lock().tick(self);
        }
    }

    fn is_stable_destination(&self, pos: BlockPos) -> bool {
        self.level()
            .is_some_and(|world| world.get_block_state(pos.below()).is_solid_render())
    }

    fn create_path_to(&self, target: BlockPos, reach_range: i32) -> Option<Path> {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.create_path_to(target, reach_range);
        }

        let world = self.level()?;
        if !world.has_full_chunk(ChunkPos::from_block_pos(target)) {
            return None;
        }

        let target = path_target_for_mob(self, world.as_ref(), target);
        let targets = [target];
        self.create_path_to_targets(&world, &targets, reach_range)
    }

    fn recompute_path(&self, request: NavigationRecomputeRequest) {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            pathfinder.recompute_path(request);
            return;
        }

        let path = self.create_path_to(request.target_pos, request.reach_range);
        self.mob_base()
            .navigation()
            .lock()
            .complete_recompute_path(path, request.game_time);
    }

    fn move_to_pos(&self, target: DVec3, speed_modifier: f64) -> bool {
        self.move_to_pos_with_reach(target, 1, speed_modifier)
    }

    fn move_to_pos_with_reach(&self, target: DVec3, reach_range: i32, speed_modifier: f64) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.move_to_pos_with_reach(target, reach_range, speed_modifier);
        }

        let target_pos = BlockPos::containing(target.x, target.y, target.z);
        let Some(world) = self.level() else {
            self.mob_base().navigation().lock().stop();
            return false;
        };
        if !world.has_full_chunk(ChunkPos::from_block_pos(target_pos)) {
            self.mob_base().navigation().lock().stop();
            return false;
        }

        let target_pos = path_target_for_mob(self, world.as_ref(), target_pos);
        let targets = [target_pos];
        if self
            .mob_base()
            .navigation()
            .lock()
            .reuse_current_path_to_targets(
                world.as_ref(),
                &targets,
                speed_modifier,
                self.position(),
            )
        {
            return true;
        }

        let path = self.create_path_to_targets(&world, &targets, reach_range);
        self.move_to_path(path, speed_modifier)
    }

    fn move_to_path(&self, path: Option<Path>, speed_modifier: f64) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.move_to_path(path, speed_modifier);
        }

        let Some(world) = self.level() else {
            self.mob_base().navigation().lock().stop();
            return false;
        };
        let mut navigation = self.mob_base().navigation().lock();
        let Some(path) = path else {
            navigation.stop();
            return false;
        };

        navigation.move_to(world.as_ref(), path, speed_modifier, self.position())
    }

    fn is_path_finding(&self) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.is_path_finding();
        }

        !self.mob_base().navigation().lock().is_done()
    }

    fn is_panicking(&self) -> bool {
        self.mob_base()
            .goal_selector()
            .lock()
            .has_running_panic_goal()
    }

    fn create_path_to_targets(
        &self,
        world: &Arc<World>,
        targets: &[BlockPos],
        reach_range: i32,
    ) -> Option<Path> {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(pathfinder) = vehicle.as_pathfinder_mob()
        {
            return pathfinder.create_path_to_targets(world, targets, reach_range);
        }

        if targets.is_empty()
            || self.position().y < f64::from(world.min_y())
            || !self.can_update_path()
        {
            return None;
        }

        let follow_range = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::FOLLOW_RANGE);
        let max_path_length = {
            let mut navigation = self.mob_base().navigation().lock();
            navigation.update_pathfinder_max_visited_nodes(follow_range);
            navigation.max_path_length(follow_range)
        };

        let mob_position = self.block_position();
        let settings = MobPathSettings::from_mob(self);
        let mut evaluator = WalkNodeEvaluator::new(settings);
        let collision_world =
            WorldCollisionProvider::for_path_navigation(world, self.as_entity_event_source());
        let mut collision = |aabb| {
            collision_world.has_entity_context_collision(
                aabb,
                self.position().y,
                self.is_descending(),
            )
        };

        self.mob_base().navigation().lock().create_path(
            &mut evaluator,
            world.as_ref(),
            &mut collision,
            NavigationPathRequest {
                mob_position,
                targets,
                max_path_length,
                reach_range,
            },
        )
    }
}

fn path_end_node_can_reach_target(path: &Path, target: BlockPos) -> bool {
    let Some(end_node) = path.end_node() else {
        return false;
    };
    let dx = end_node.x - target.x();
    let dz = end_node.z - target.z();
    f64::from(dx * dx + dz * dz) <= TARGET_REACH_DISTANCE_SQR
}

fn path_target_for_mob<M: PathfinderMob + ?Sized>(
    mob: &M,
    level: &dyn LevelReader,
    target: BlockPos,
) -> BlockPos {
    if mob.can_path_to_targets_below_surface() {
        target
    } else {
        find_ground_path_target_surface(level, target)
    }
}

fn find_ground_path_target_surface(level: &dyn LevelReader, mut pos: BlockPos) -> BlockPos {
    if level.get_block_state(pos).is_air() {
        let mut column_pos = pos.below();
        while column_pos.y() >= level.min_y() && level.get_block_state(column_pos).is_air() {
            column_pos = column_pos.below();
        }
        if column_pos.y() >= level.min_y() {
            return column_pos.above();
        }

        column_pos = pos.at_y(pos.y() + 1);
        while column_pos.y() < level.max_y_exclusive() && level.get_block_state(column_pos).is_air()
        {
            column_pos = column_pos.above();
        }
        pos = column_pos;
    }

    if !level.get_block_state(pos).is_solid() {
        return pos;
    }

    let mut column_pos = pos.above();
    while column_pos.y() < level.max_y_exclusive() && level.get_block_state(column_pos).is_solid() {
        column_pos = column_pos.above();
    }
    column_pos
}

fn should_jump_to_wanted_position<M: Mob + ?Sized>(mob: &M, xd: f64, yd: f64, zd: f64) -> bool {
    let max_up_step = f64::from(mob.max_up_step());
    if yd > max_up_step && xd * xd + zd * zd < mob.bounding_box().width().max(1.0) {
        return true;
    }

    let Some(world) = mob.level() else {
        return false;
    };
    let pos = mob.block_position();
    let block_state = world.get_block_state(pos);
    let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
    let shape = behavior.get_collision_shape(
        block_state,
        world.as_ref(),
        pos,
        BlockCollisionContext::empty(),
    );
    let shape_top = position_shape_top(pos, shape.max(Axis::Y));
    let block = block_state.get_block();
    !shape.is_empty()
        && mob.position().y < shape_top
        && !block.has_tag(&BlockTag::DOORS)
        && !block.has_tag(&BlockTag::FENCES)
}

fn position_shape_top(pos: BlockPos, local_y: f64) -> f64 {
    f64::from(pos.y()) + local_y
}

fn block_pos_distance_sqr(a: BlockPos, b: BlockPos) -> f64 {
    let dx = f64::from(a.x() - b.x());
    let dy = f64::from(a.y() - b.y());
    let dz = f64::from(a.z() - b.z());
    dx.mul_add(dx, dy.mul_add(dy, dz * dz))
}

fn block_center_distance_sqr(pos: BlockPos, target: DVec3) -> f64 {
    let (x, y, z) = pos.get_center();
    DVec3::new(x, y, z).distance_squared(target)
}

fn home_radius_sqr(radius: i32) -> f64 {
    let radius = f64::from(radius);
    radius * radius
}

fn rotlerp(a: f32, b: f32, max: f32) -> f32 {
    let mut diff = wrap_degrees(b - a);
    if diff > max {
        diff = max;
    }
    if diff < -max {
        diff = -max;
    }

    let mut result = a + diff;
    if result < 0.0 {
        result += 360.0;
    } else if result > 360.0 {
        result -= 360.0;
    }
    result
}

fn wrap_degrees(mut degrees: f32) -> f32 {
    degrees %= 360.0;
    if degrees >= 180.0 {
        degrees -= 360.0;
    }
    if degrees < -180.0 {
        degrees += 360.0;
    }
    degrees
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::vanilla_entities;
    use steel_registry::vanilla_items::ITEMS;
    use steel_registry::{
        REGISTRY, test_support::init_test_registry, vanilla_attributes, vanilla_blocks,
        vanilla_damage_types,
    };
    use steel_utils::locks::SyncMutex;
    use steel_utils::{BlockPos, BlockStateId};

    use super::{
        can_attempt_equipment_drop, find_ground_path_target_surface, path_end_node_can_reach_target,
    };
    use crate::behavior::init_behaviors;
    use crate::entity::ai::control::{DEFAULT_LOOK_X_MAX_ROT_ANGLE, DEFAULT_LOOK_Y_MAX_ROT_SPEED};
    use crate::entity::ai::goal::GoalControl;
    use crate::entity::ai::node::Node;
    use crate::entity::ai::path::{Path, PathType};
    use crate::entity::damage::DamageSource;
    use crate::entity::mob::{Mob, MobBase};
    use crate::entity::{
        Entity, EntityBase, LivingEntity, LivingEntityBase, PathfinderMob, SharedEntity,
    };
    use crate::world::LevelReader;

    #[test]
    fn equipment_drop_attempt_gate_matches_vanilla_conditions() {
        assert!(!can_attempt_equipment_drop(0.0, true, true));
        assert!(!can_attempt_equipment_drop(0.085, false, false));
        assert!(can_attempt_equipment_drop(0.085, false, true));
        assert!(can_attempt_equipment_drop(2.0, true, false));
    }

    struct SurfaceLevel {
        default_state: BlockStateId,
        states: Vec<(BlockPos, BlockStateId)>,
    }

    impl SurfaceLevel {
        fn new(default_state: BlockStateId) -> Self {
            Self {
                default_state,
                states: Vec::new(),
            }
        }

        fn with(mut self, pos: BlockPos, state: BlockStateId) -> Self {
            self.states.push((pos, state));
            self
        }
    }

    impl LevelReader for SurfaceLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            self.states
                .iter()
                .find_map(|(state_pos, state)| (*state_pos == pos).then_some(*state))
                .unwrap_or(self.default_state)
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    struct DespawnTestMob {
        base: EntityBase,
        entity_type: EntityTypeRef,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        flags: SyncMutex<i8>,
        health: SyncMutex<f32>,
        nearest_player_distance_sqr: Option<f64>,
        remove_when_far_away: bool,
        controlling_passenger: SyncMutex<Option<SharedEntity>>,
    }

    impl DespawnTestMob {
        fn new(nearest_player_distance_sqr: Option<f64>, remove_when_far_away: bool) -> Self {
            Self::with_position(
                1,
                DVec3::ZERO,
                nearest_player_distance_sqr,
                remove_when_far_away,
            )
        }

        fn with_position(
            id: i32,
            position: DVec3,
            nearest_player_distance_sqr: Option<f64>,
            remove_when_far_away: bool,
        ) -> Self {
            Self::with_entity_type(
                id,
                position,
                &vanilla_entities::PIG,
                nearest_player_distance_sqr,
                remove_when_far_away,
            )
        }

        fn with_entity_type(
            id: i32,
            position: DVec3,
            entity_type: EntityTypeRef,
            nearest_player_distance_sqr: Option<f64>,
            remove_when_far_away: bool,
        ) -> Self {
            init_test_registry();

            Self {
                base: EntityBase::new(id, position, entity_type.dimensions, Weak::new()),
                entity_type,
                living_base: LivingEntityBase::new(entity_type),
                mob_base: MobBase::new(),
                flags: SyncMutex::new(0),
                health: SyncMutex::new(10.0),
                nearest_player_distance_sqr,
                remove_when_far_away,
                controlling_passenger: SyncMutex::new(None),
            }
        }

        fn set_controlling_passenger(&self, passenger: SharedEntity) {
            *self.controlling_passenger.lock() = Some(passenger);
        }
    }

    impl Entity for DespawnTestMob {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
            Some(self)
        }

        fn as_mob(&self) -> Option<&dyn Mob> {
            Some(self)
        }

        fn controlling_passenger(&self) -> Option<SharedEntity> {
            self.controlling_passenger.lock().clone()
        }

        fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
            LivingEntity::hurt_server(self, source, amount)
        }
    }

    impl LivingEntity for DespawnTestMob {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health;
        }
    }

    struct HiddenTarget {
        base: EntityBase,
        living_base: LivingEntityBase,
        health: SyncMutex<f32>,
    }

    impl HiddenTarget {
        fn shared(id: i32) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::new(
                    id,
                    DVec3::ZERO,
                    vanilla_entities::PIG.dimensions,
                    Weak::new(),
                ),
                living_base: LivingEntityBase::new(&vanilla_entities::PIG),
                health: SyncMutex::new(10.0),
            })
        }
    }

    impl Entity for HiddenTarget {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }

        fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
            Some(self)
        }
    }

    impl LivingEntity for HiddenTarget {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health;
        }

        fn can_be_seen_as_enemy(&self) -> bool {
            false
        }
    }

    impl Mob for DespawnTestMob {
        fn mob_base(&self) -> &MobBase {
            &self.mob_base
        }

        fn mob_flags(&self) -> i8 {
            *self.flags.lock()
        }

        fn set_mob_flags(&self, flags: i8) {
            *self.flags.lock() = flags;
        }

        fn remove_when_far_away(&self, _dist_sqr: f64) -> bool {
            self.remove_when_far_away
        }

        fn nearest_player_distance_sqr(&self) -> Option<f64> {
            self.nearest_player_distance_sqr
        }
    }

    impl PathfinderMob for DespawnTestMob {}

    struct MobControlVehicleEntity {
        base: EntityBase,
        entity_type: EntityTypeRef,
    }

    impl MobControlVehicleEntity {
        fn new(id: i32, entity_type: EntityTypeRef) -> Self {
            Self {
                base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
                entity_type,
            }
        }
    }

    impl Entity for MobControlVehicleEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }
    }

    #[test]
    fn mob_base_uses_vanilla_fire_path_malus_defaults() {
        let base = MobBase::new();
        let malus = base.pathfinding_malus().lock();

        assert_eq!(
            malus.get(PathType::FireInNeighbor).to_bits(),
            8.0_f32.to_bits()
        );
        assert_eq!(malus.get(PathType::Fire).to_bits(), 16.0_f32.to_bits());
        assert_eq!(malus.get(PathType::Water).to_bits(), 8.0_f32.to_bits());
    }

    #[test]
    fn pathfinder_mob_reads_below_surface_capability_from_navigation() {
        let mob = DespawnTestMob::new(None, false);

        assert!(!mob.can_path_to_targets_below_surface());

        mob.mob_base()
            .navigation()
            .lock()
            .set_can_path_to_targets_below_surface(true);

        assert!(mob.can_path_to_targets_below_surface());
    }

    #[test]
    fn mob_server_ai_step_increments_no_action_time() {
        let mob = DespawnTestMob::new(None, false);

        mob.set_no_action_time(12);
        mob.mob_server_ai_step();

        assert_eq!(mob.no_action_time(), 13);
    }

    #[test]
    fn mob_control_flags_enable_goals_without_controller_or_boat() {
        let mob = DespawnTestMob::new(None, false);
        {
            let mut selector = mob.mob_base().goal_selector().lock();
            selector.disable_control(GoalControl::Move);
            selector.disable_control(GoalControl::Jump);
            selector.disable_control(GoalControl::Look);
        }

        mob.update_control_flags();

        let selector = mob.mob_base().goal_selector().lock();
        assert!(!selector.is_control_disabled(GoalControl::Move));
        assert!(!selector.is_control_disabled(GoalControl::Jump));
        assert!(!selector.is_control_disabled(GoalControl::Look));
    }

    #[test]
    fn mob_control_flags_disable_goals_for_mob_controller() {
        let mob = DespawnTestMob::new(None, false);
        let controller: SharedEntity =
            Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));
        mob.set_controlling_passenger(controller);

        mob.update_control_flags();

        let selector = mob.mob_base().goal_selector().lock();
        assert!(selector.is_control_disabled(GoalControl::Move));
        assert!(selector.is_control_disabled(GoalControl::Jump));
        assert!(selector.is_control_disabled(GoalControl::Look));
    }

    #[test]
    fn mob_control_flags_disable_jump_when_riding_boat() {
        let mob = Arc::new(DespawnTestMob::new(None, false));
        let mob_entity: SharedEntity = mob.clone();
        let boat: SharedEntity =
            Arc::new(MobControlVehicleEntity::new(2, &vanilla_entities::OAK_BOAT));
        EntityBase::restore_passenger_relationship(&boat, &mob_entity);

        mob.update_control_flags();

        let selector = mob.mob_base().goal_selector().lock();
        assert!(!selector.is_control_disabled(GoalControl::Move));
        assert!(selector.is_control_disabled(GoalControl::Jump));
        assert!(!selector.is_control_disabled(GoalControl::Look));
    }

    #[test]
    fn mob_attack_damage_source_uses_item_damage_type_component() {
        let mob = DespawnTestMob::new(None, false);
        let spear = ItemStack::new(&ITEMS.wooden_spear);

        let source = mob.mob_attack_damage_source(&spear, &mob);

        assert_eq!(source.damage_type.key, vanilla_damage_types::SPEAR.key);
        assert_eq!(source.causing_entity_id, Some(mob.id()));
        assert_eq!(source.direct_entity_id, Some(mob.id()));
    }

    #[test]
    fn mob_target_stores_living_target_weakly() {
        let mob = DespawnTestMob::new(None, false);
        let target: SharedEntity =
            Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));

        assert!(mob.set_target(Some(&target)));

        let stored = mob.target().expect("living target should be stored");
        assert!(Arc::ptr_eq(&stored, &target));
    }

    #[test]
    fn mob_target_can_be_cleared() {
        let mob = DespawnTestMob::new(None, false);
        let target: SharedEntity =
            Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));
        assert!(mob.set_target(Some(&target)));

        assert!(mob.set_target(None));

        assert!(mob.target().is_none());
    }

    #[test]
    fn mob_target_expires_with_target_entity() {
        let mob = DespawnTestMob::new(None, false);
        {
            let target: SharedEntity =
                Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));
            assert!(mob.set_target(Some(&target)));
        }

        assert!(mob.target().is_none());
    }

    #[test]
    fn mob_target_rejects_non_living_entities() {
        let mob = DespawnTestMob::new(None, false);
        let target: SharedEntity =
            Arc::new(MobControlVehicleEntity::new(2, &vanilla_entities::OAK_BOAT));

        assert!(!mob.set_target(Some(&target)));

        assert!(mob.target().is_none());
    }

    #[test]
    fn mob_target_rejects_targets_it_cannot_attack() {
        let mob = DespawnTestMob::new(None, false);
        let target = HiddenTarget::shared(2);

        assert!(!mob.set_target(Some(&target)));

        assert!(mob.target().is_none());
    }

    #[test]
    fn mob_target_filters_invalid_target_without_clearing_stored_target() {
        let mob = DespawnTestMob::new(None, false);
        let target: SharedEntity =
            Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));

        assert!(mob.mob_base().set_target(Some(&target), |_| true));

        assert!(mob.mob_base().target(|_| false).is_none());

        let stored = mob
            .mob_base()
            .target(|_| true)
            .expect("temporary invalidity must not clear the stored target");
        assert!(Arc::ptr_eq(&stored, &target));
    }

    #[test]
    fn mob_target_clears_previous_target_when_new_target_is_invalid() {
        let mob = DespawnTestMob::new(None, false);
        let previous: SharedEntity =
            Arc::new(DespawnTestMob::with_position(2, DVec3::ZERO, None, false));
        let invalid = HiddenTarget::shared(3);

        assert!(mob.set_target(Some(&previous)));
        assert!(!mob.set_target(Some(&invalid)));

        assert!(mob.target().is_none());
    }

    #[test]
    fn melee_attack_range_uses_vanilla_default_reach() {
        let mob = DespawnTestMob::new(None, false);
        let close_target = DespawnTestMob::with_position(2, DVec3::new(1.7, 0.0, 0.0), None, false);
        let far_target = DespawnTestMob::with_position(3, DVec3::new(1.8, 0.0, 0.0), None, false);

        assert!(mob.is_within_melee_attack_range(&close_target));
        assert!(!mob.is_within_melee_attack_range(&far_target));
    }

    #[test]
    fn melee_attack_range_uses_vehicle_expanded_attack_box() {
        let mob = Arc::new(DespawnTestMob::with_position(
            1,
            DVec3::new(4.0, 0.0, 0.0),
            None,
            false,
        ));
        let target = DespawnTestMob::with_position(2, DVec3::new(1.1, 0.0, 0.0), None, false);

        assert!(!mob.is_within_melee_attack_range(&target));

        let mob_entity: SharedEntity = mob.clone();
        let vehicle: SharedEntity =
            Arc::new(MobControlVehicleEntity::new(3, &vanilla_entities::PIG));
        EntityBase::restore_passenger_relationship(&vehicle, &mob_entity);

        assert!(mob.is_within_melee_attack_range(&target));
    }

    #[test]
    fn target_reach_uses_vanilla_horizontal_endpoint_distance() {
        let reachable = Path::new(vec![Node::new(1, 0, 1)], BlockPos::new(2, 64, 2), false);
        let too_far = Path::new(vec![Node::new(3, 64, 0)], BlockPos::new(0, 64, 0), false);

        assert!(path_end_node_can_reach_target(
            &reachable,
            BlockPos::new(2, 64, 2)
        ));
        assert!(!path_end_node_can_reach_target(
            &too_far,
            BlockPos::new(0, 64, 0)
        ));
    }

    #[test]
    fn mob_can_attack_excludes_ghasts() {
        let mob = DespawnTestMob::with_entity_type(
            1,
            DVec3::ZERO,
            &vanilla_entities::ZOMBIE,
            None,
            false,
        );
        let ghast = DespawnTestMob::with_entity_type(
            2,
            DVec3::new(1.0, 0.0, 0.0),
            &vanilla_entities::GHAST,
            None,
            false,
        );

        assert!(LivingEntity::can_attack(&mob, &ghast));
        assert!(!Mob::can_attack(&mob, &ghast));
    }

    #[test]
    fn mob_base_tick_increments_ambient_sound_time_when_roll_fails() {
        let mob = DespawnTestMob::new(None, false);

        mob.mob_base_tick();

        assert_eq!(mob.mob_base().ambient_sound_time(), 1);
    }

    #[test]
    fn mob_base_tick_resets_ambient_sound_time_after_vanilla_roll() {
        let mob = DespawnTestMob::new(None, false);

        mob.mob_base().set_ambient_sound_time(1000);
        mob.mob_base_tick();

        assert_eq!(mob.mob_base().ambient_sound_time(), -80);
    }

    #[test]
    fn mob_hurt_sound_resets_ambient_sound_time() {
        let mob = DespawnTestMob::new(None, false);
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        mob.mob_base().set_ambient_sound_time(12);
        LivingEntity::play_hurt_sound(&mob, &source);

        assert_eq!(mob.mob_base().ambient_sound_time(), -80);
    }

    #[test]
    fn mob_do_hurt_target_applies_attack_damage_and_records_target() {
        init_test_registry();
        init_behaviors();

        let mob = DespawnTestMob::with_entity_type(
            1,
            DVec3::ZERO,
            &vanilla_entities::ZOMBIE,
            None,
            false,
        );
        mob.attributes()
            .lock()
            .set_base_value(vanilla_attributes::ATTACK_DAMAGE, 4.0);
        let target = Arc::new(DespawnTestMob::with_position(
            2,
            DVec3::new(1.0, 0.0, 0.0),
            None,
            false,
        ));
        let target_entity: SharedEntity = target.clone();

        assert!(mob.do_hurt_target(&target_entity));

        assert_eq!(target.get_health().to_bits(), 6.0_f32.to_bits());
        let stored_target = mob
            .last_hurt_mob()
            .expect("successful mob attack should record target");
        assert!(Arc::ptr_eq(&stored_target, &target_entity));
    }

    #[test]
    fn mob_do_hurt_target_applies_vanilla_extra_knockback() {
        init_test_registry();
        init_behaviors();

        let mob = DespawnTestMob::with_entity_type(
            1,
            DVec3::ZERO,
            &vanilla_entities::ZOMBIE,
            None,
            false,
        );
        {
            let mut attributes = mob.attributes().lock();
            attributes.set_base_value(vanilla_attributes::ATTACK_DAMAGE, 4.0);
            attributes.set_base_value(vanilla_attributes::ATTACK_KNOCKBACK, 2.0);
        }
        mob.set_velocity(DVec3::new(1.0, 0.0, 1.0));
        let target = Arc::new(DespawnTestMob::with_position(
            2,
            DVec3::new(1.0, 0.0, 0.0),
            None,
            false,
        ));
        let target_entity: SharedEntity = target.clone();

        assert!(mob.do_hurt_target(&target_entity));

        assert_eq!(mob.velocity().x.to_bits(), 0.6_f64.to_bits());
        assert_eq!(mob.velocity().z.to_bits(), 0.6_f64.to_bits());
        assert!(target.velocity().length_squared() > 0.0);
        assert!(target.needs_velocity_sync());
    }

    #[test]
    fn mob_look_control_rotates_head_yaw_without_turning_body_yaw() {
        let mob = DespawnTestMob::new(None, false);
        mob.set_rotation((0.0, 0.0));
        mob.set_y_body_rot(0.0);
        mob.set_y_head_rot(0.0);
        let position = mob.position();
        mob.mob_base().controls().lock().look_control.set_look_at(
            DVec3::new(position.x + 1.0, mob.get_eye_y(), position.z),
            DEFAULT_LOOK_Y_MAX_ROT_SPEED,
            DEFAULT_LOOK_X_MAX_ROT_ANGLE,
        );

        Mob::tick_look_control(&mob);

        assert_eq!(mob.rotation(), (0.0, 0.0));
        assert_eq!(mob.y_body_rot().to_bits(), 0.0_f32.to_bits());
        assert_eq!(mob.y_head_rot().to_bits(), (-10.0_f32).to_bits());
    }

    #[test]
    fn mob_look_control_returns_head_yaw_toward_body_when_idle() {
        let mob = DespawnTestMob::new(None, false);
        mob.set_rotation((0.0, 20.0));
        mob.set_y_body_rot(90.0);
        mob.set_y_head_rot(0.0);

        Mob::tick_look_control(&mob);

        assert_eq!(mob.rotation(), (0.0, 0.0));
        assert_eq!(mob.y_body_rot().to_bits(), 90.0_f32.to_bits());
        assert_eq!(mob.y_head_rot().to_bits(), 10.0_f32.to_bits());
    }

    #[test]
    fn mob_body_rotation_control_uses_tick_position_delta() {
        let mob = DespawnTestMob::new(None, false);
        mob.set_old_position(DVec3::ZERO);
        mob.base.set_position_local(DVec3::new(1.0, 0.0, 0.0));
        mob.set_rotation((90.0, 0.0));
        mob.set_y_body_rot(0.0);
        mob.set_y_head_rot(200.0);

        Mob::tick_body_rotation_control(&mob);

        assert_eq!(mob.y_body_rot().to_bits(), 90.0_f32.to_bits());
        assert_eq!(mob.y_head_rot().to_bits(), 165.0_f32.to_bits());
    }

    #[test]
    fn mob_tick_leash_applies_default_elastic_pull() {
        let mob = Arc::new(DespawnTestMob::with_position(1, DVec3::ZERO, None, false));
        let holder = Arc::new(DespawnTestMob::with_position(
            2,
            DVec3::new(7.0, 0.0, 0.0),
            None,
            false,
        ));
        let holder_entity: SharedEntity = holder.clone();
        assert!(mob.set_leashed_to(&holder_entity));

        mob.tick_leash();

        assert!(mob.velocity().x > 0.0);
        assert!(mob.velocity().z < 0.0);
        assert!(mob.needs_velocity_sync());
        assert!(mob.rotation().0 < 0.0);
        assert!(mob.is_leashed());
    }

    #[test]
    fn mob_despawn_resets_no_action_time_near_player() {
        let mob = DespawnTestMob::new(Some(31.0 * 31.0), false);

        mob.set_no_action_time(42);
        mob.check_mob_despawn();

        assert_eq!(mob.no_action_time(), 0);
        assert!(!mob.is_removed());
    }

    #[test]
    fn mob_despawn_discards_far_removable_mob() {
        let mob = DespawnTestMob::new(Some(129.0 * 129.0), true);

        mob.check_mob_despawn();

        assert!(mob.is_removed());
    }

    #[test]
    fn mob_persistence_resets_no_action_time_and_blocks_removal() {
        let mob = DespawnTestMob::new(Some(129.0 * 129.0), true);

        mob.set_no_action_time(42);
        mob.set_persistence_required();
        mob.check_mob_despawn();

        assert_eq!(mob.no_action_time(), 0);
        assert!(!mob.is_removed());
    }

    #[test]
    fn mob_home_restriction_uses_vanilla_radius() {
        let mob = DespawnTestMob::new(None, false);

        assert!(mob.is_within_home_pos(BlockPos::new(1000, 64, 1000)));

        mob.set_home_to(BlockPos::ZERO, 4);
        assert!(mob.has_home());
        assert!(mob.is_within_home_pos(BlockPos::new(3, 0, 0)));
        assert!(!mob.is_within_home_pos(BlockPos::new(4, 0, 0)));

        mob.clear_home();
        assert!(!mob.has_home());
        assert!(mob.is_within_home_pos(BlockPos::new(1000, 64, 1000)));
    }

    #[test]
    fn ground_path_target_air_rewrites_to_surface_above_ground() {
        init_test_registry();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = SurfaceLevel::new(air).with(BlockPos::new(4, 63, 4), stone);

        assert_eq!(
            find_ground_path_target_surface(&level, BlockPos::new(4, 70, 4)),
            BlockPos::new(4, 64, 4)
        );
    }

    #[test]
    fn ground_path_target_solid_rewrites_to_first_open_block_above() {
        init_test_registry();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let level = SurfaceLevel::new(air)
            .with(BlockPos::new(4, 64, 4), stone)
            .with(BlockPos::new(4, 65, 4), stone);

        assert_eq!(
            find_ground_path_target_surface(&level, BlockPos::new(4, 64, 4)),
            BlockPos::new(4, 66, 4)
        );
    }
}
