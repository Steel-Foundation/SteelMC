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
    vanilla_game_events, vanilla_items,
};
use steel_utils::UuidExt;
use steel_utils::types::{Difficulty, InteractionHand};
use steel_utils::{BlockPos, ChunkPos, Identifier, WorldAabb, axis::Axis};
use uuid::Uuid;

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, InteractionResult};
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
    fn by_equipment(self, slot: EquipmentSlot) -> f32 {
        self.by_equipment[slot.index()]
    }

    fn set_guaranteed_drop(&mut self, slot: EquipmentSlot) {
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
    pub goal_selector: GoalSelector,
    pub target_selector: GoalSelector,
    target: Option<WeakEntity>,
    sensing: Sensing,
    pub controls: MobControls,
    pub navigation: PathNavigation,
    pub pathfinding_malus: PathfindingMalus,
    persistence_required: bool,
    can_pick_up_loot: bool,
    drop_chances: DropChances,
    home_restriction: MobHomeRestriction,
    death_loot_table: Option<Identifier>,
    death_loot_table_seed: i64,
    leash_data: Option<LeashData>,
    ambient_sound_time: i32,
    xp_reward: i32,
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
    /// Computes the vanilla leash attachment for a holder handle: fence knots
    /// attach by block position, everything else by UUID.
    fn holder_attachment(holder: &SharedEntity) -> LeashAttachment {
        holder
            .with_entity_as::<LeashFenceKnotEntity, _>(|knot| {
                LeashAttachment::FenceKnot(knot.block_pos)
            })
            .unwrap_or_else(|| LeashAttachment::Entity(holder.uuid()))
    }

    fn from_entity(holder: &SharedEntity) -> Self {
        Self {
            attachment: Self::holder_attachment(holder),
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
        self.holder()
            .map_or(self.attachment, |holder| Self::holder_attachment(&holder))
    }

    fn set_holder(&mut self, holder: &SharedEntity) {
        self.attachment = Self::holder_attachment(holder);
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
        (bounding_box.min_x() + bounding_box.max_x()) / 2.0,
        (bounding_box.min_y() + bounding_box.max_y()) / 2.0,
        (bounding_box.min_z() + bounding_box.max_z()) / 2.0,
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
            goal_selector: GoalSelector::new(),
            target_selector: GoalSelector::new(),
            target: None,
            sensing: Sensing::new(),
            controls: MobControls::new(),
            navigation: PathNavigation::new(),
            pathfinding_malus: PathfindingMalus::new(),
            persistence_required: false,
            can_pick_up_loot: false,
            drop_chances: DropChances::DEFAULT,
            home_restriction: MobHomeRestriction::none(),
            death_loot_table: None,
            death_loot_table_seed: 0,
            leash_data: None,
            ambient_sound_time: 0,
            xp_reward: 0,
        }
    }

    #[must_use]
    pub fn target(&mut self, is_valid: impl Fn(&dyn LivingEntity) -> bool) -> Option<SharedEntity> {
        let Some(upgraded) = self.target.as_ref().and_then(WeakEntity::upgrade) else {
            self.target = None;
            return None;
        };

        {
            let upgraded = upgraded.lock_entity();
            let Some(living_target) = upgraded.get().as_living_entity() else {
                return None;
            };
            if !is_valid(living_target) {
                return None;
            }
        }
        Some(upgraded)
    }

    pub fn set_target(
        &mut self,
        target: Option<&SharedEntity>,
        is_valid: impl Fn(&dyn LivingEntity) -> bool,
    ) -> bool {
        let Some(target) = target else {
            self.target = None;
            return true;
        };
        let Some(valid) = target.with_living(|living_target| is_valid(living_target)) else {
            // Not a living entity.
            return false;
        };
        if !valid {
            self.target = None;
            return false;
        }

        self.target = Some(Arc::downgrade(target));
        true
    }

    fn get_and_increment_ambient_sound_time(&mut self) -> i32 {
        let previous = self.ambient_sound_time;
        self.ambient_sound_time += 1;
        previous
    }
}

impl Default for MobBase {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Mob: LivingEntity {
    fn mob_base(&mut self) -> &mut MobBase;
    fn mob_base_ref(&self) -> &MobBase;

    fn mob_flags(&self) -> i8;

    fn set_mob_flags(&mut self, flags: i8);

    fn custom_server_ai_step(&mut self) {}

    fn tick_goal_selectors(&mut self) {}

    fn xp_reward(&mut self) -> i32 {
        self.mob_base().xp_reward
    }

    fn set_xp_reward(&mut self, xp_reward: i32) {
        self.mob_base().xp_reward = xp_reward;
    }

    /// Returns vanilla `Mob.getTarget`.
    fn target(&mut self) -> Option<SharedEntity> {
        let Some(upgraded) = self
            .mob_base()
            .target
            .as_ref()
            .and_then(WeakEntity::upgrade)
        else {
            self.mob_base().target = None;
            return None;
        };

        let valid = {
            let locked = upgraded.lock_entity();
            match locked.get().as_living_entity() {
                Some(living_target) => self.is_valid_target(living_target),
                None => false,
            }
        };

        valid.then_some(upgraded)
    }

    /// Sets vanilla `Mob.target`.
    ///
    /// Returns `false` when the supplied entity is not a living entity.
    fn set_target(&mut self, target: Option<&SharedEntity>) -> bool {
        let valid = match target {
            None => true,
            Some(entity) => match entity.with_living(|living| self.is_valid_target(living)) {
                Some(valid) => valid,
                None => return false, // not a living entity
            },
        };

        self.mob_base().set_target(target, |_| valid)
    }

    fn is_valid_target(&self, target: &dyn LivingEntity) -> bool {
        if target
            .as_player_ref()
            .is_some_and(|player| player.has_infinite_materials() || player.is_spectator())
        {
            return false;
        }

        self.can_attack(target)
    }

    fn base_experience_reward_mob(&mut self) -> i32 {
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
                let equipment = self.living_base().equipment();
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

    fn reset_ambient_sound_time(&mut self) {
        self.mob_base().ambient_sound_time = -self.ambient_sound_interval();
    }

    fn mob_base_tick(&mut self) {
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
        &mut self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_mob_base(world, spawn_reason, group_data)
    }

    fn finalize_spawn_mob_base(
        &mut self,
        world: &Arc<World>,
        _spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let needs_random_spawn_bonus = !self
            .attributes()
            .has_modifier(vanilla_attributes::FOLLOW_RANGE, &RANDOM_SPAWN_BONUS_ID);
        let random_spawn_bonus = needs_random_spawn_bonus
            .then(|| RANDOM_SPAWN_BONUS_SCALE * (rand::random::<f64>() - rand::random::<f64>()));
        let left_handed = rand::random::<f32>() < LEFT_HANDED_SPAWN_CHANCE;

        if let Some(amount) = random_spawn_bonus {
            self.attributes_mut().add_modifier(
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
        &mut self,
        player: &mut Player,
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
    fn mob_interact(&mut self, _player: &mut Player, _hand: InteractionHand) -> InteractionResult {
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
        self.mob_base_ref().persistence_required
    }

    fn set_persistence_required(&mut self) {
        self.mob_base().persistence_required = true;
    }

    /// Returns vanilla `Mob.canPickUpLoot`.
    fn can_pick_up_loot(&self) -> bool {
        self.mob_base_ref().can_pick_up_loot
    }

    fn set_can_pick_up_loot(&mut self, can_pick_up_loot: bool) {
        self.mob_base().can_pick_up_loot = can_pick_up_loot;
    }

    fn equipment_drop_chance(&self, slot: EquipmentSlot) -> f32 {
        self.mob_base_ref().drop_chances.by_equipment(slot)
    }

    fn is_equipment_drop_preserved(&self, slot: EquipmentSlot) -> bool {
        self.mob_base_ref().drop_chances.is_preserved(slot)
    }

    fn set_guaranteed_drop(&mut self, slot: EquipmentSlot) {
        self.mob_base().drop_chances.set_guaranteed_drop(slot);
    }

    fn drop_custom_death_loot_mob(&mut self, _source: &DamageSource, killed_by_player: bool) {
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
                let equipment = self.living_base().equipment();
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
                let equipment = self.living_base().equipment();
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
        self.mob_base_ref().drop_chances.save(nbt);
        if let Some(leash_data) = self.mob_base_ref().leash_data.as_ref() {
            leash_data.save(nbt);
        }

        if self.has_home() {
            let home = self.mob_base_ref().home_restriction;
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
        if let Some(loot_table) = self.mob_base_ref().death_loot_table.as_ref() {
            nbt.insert("DeathLootTable", loot_table.to_string());
        }
        let loot_table_seed = self.mob_base_ref().death_loot_table_seed;
        if loot_table_seed != 0 {
            nbt.insert("DeathLootTableSeed", loot_table_seed);
        }
        if self.is_no_ai() {
            nbt.insert("NoAI", i8::from(true));
        }
    }

    fn load_mob(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_can_pick_up_loot(nbt.byte("CanPickUpLoot").is_some_and(|value| value != 0));
        self.mob_base().persistence_required = nbt
            .byte("PersistenceRequired")
            .is_some_and(|value| value != 0);
        self.mob_base().drop_chances = DropChances::load(nbt);
        self.mob_base().leash_data = LeashData::load(nbt);
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
        self.mob_base().death_loot_table = death_loot_table;
        self.mob_base().death_loot_table_seed = nbt.long("DeathLootTableSeed").unwrap_or(0);
        self.set_no_ai(nbt.byte("NoAI").is_some_and(|value| value != 0));
    }

    fn set_death_loot_table(&mut self, loot_table: Option<Identifier>) {
        self.mob_base().death_loot_table = loot_table;
    }

    fn set_death_loot_table_seed(&mut self, seed: i64) {
        self.mob_base().death_loot_table_seed = seed;
    }

    fn custom_death_loot_table(&self) -> Option<LootTableRef> {
        self.mob_base_ref()
            .death_loot_table
            .as_ref()
            .and_then(|key| REGISTRY.loot_tables.by_key(key))
    }

    fn has_custom_death_loot_table(&self) -> bool {
        self.mob_base_ref().death_loot_table.is_some()
    }

    fn death_loot_table_seed(&self) -> i64 {
        self.mob_base_ref().death_loot_table_seed
    }

    fn clear_custom_death_loot_table(&mut self) {
        self.mob_base().death_loot_table = None;
    }

    fn is_leashed(&self) -> bool {
        self.leash_holder().is_some()
    }

    fn may_be_leashed(&self) -> bool {
        self.mob_base_ref().leash_data.is_some()
    }

    fn leash_holder(&self) -> Option<SharedEntity> {
        self.mob_base_ref()
            .leash_data
            .as_ref()
            .and_then(LeashData::holder)
    }

    fn leash_attachment(&self) -> Option<LeashAttachment> {
        self.mob_base_ref()
            .leash_data
            .as_ref()
            .map(LeashData::saved_attachment)
    }

    fn set_delayed_leash_attachment(&mut self, attachment: LeashAttachment) {
        self.mob_base().leash_data = Some(LeashData::from_delayed_attachment(attachment));
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

    fn leash_too_far_behaviour(&mut self) {
        self.drop_leash(None);
    }

    fn on_elastic_leash_pull(&self) {
        self.check_fall_distance_accumulation();
    }

    fn close_range_leash_behaviour(&self, _holder: &dyn Entity) {}

    fn check_elastic_interactions(&mut self, holder: &dyn Entity) -> bool {
        let Some(wrench) = compute_elastic_interaction(
            self.as_entity_event_source(),
            holder,
            self.leash_elastic_distance(),
        ) else {
            return false;
        };

        {
            let Some(leash_data) = self.mob_base().leash_data.as_mut() else {
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

    fn apply_leash_angular_momentum(&mut self) -> bool {
        let angular_friction = self.leash_angular_friction();
        let angular_momentum = {
            let Some(leash_data) = self.mob_base().leash_data.as_mut() else {
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
        self.mob_base_ref()
            .leash_data
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

    fn set_leashed_to(&mut self, holder: &SharedEntity, caller: Option<&dyn Entity>) -> bool {
        if self.id() == holder.id() {
            return false;
        }

        let old_holder = self.leash_holder();
        {
            let leash_data = &mut self.mob_base().leash_data;
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
            self.notify_leash_holder_removed(&old_holder, caller);
        }
        true
    }

    fn tick_leash(&mut self) {
        if let Some(holder) = self.leash_holder() {
            // Hold the holder's behavior lock once for all holder-dependent
            // reads. Branches that end the leash re-lock the holder to notify
            // it (`drop_leash`/`remove_leash`), so the guard is dropped first.
            let mut locked = holder.lock_entity();
            let h = locked.get_mut();

            if !self.can_interact_with_level() || !h.can_interact_with_level() {
                drop(locked);
                if let Some(world) = self.level()
                    && world.get_game_rule(&ENTITY_DROPS).as_bool() == Some(true)
                {
                    self.drop_leash(None);
                } else {
                    self.remove_leash(None);
                }
                return;
            }

            let distance_to = self.leash_distance_to(h);
            self.when_leashed_to(h);
            let angular_momentum_before_distance_action = self.leash_angular_momentum();
            if distance_to > self.leash_snap_distance() {
                let holder_position = h.position();
                drop(locked);
                if let Some(world) = self.level() {
                    world.play_sound_at(
                        &sound_events::ITEM_LEAD_BREAK,
                        SoundSource::Neutral,
                        holder_position,
                        1.0,
                        1.0,
                        None,
                    );
                }
                self.leash_too_far_behaviour();
            } else if distance_to
                > self.leash_elastic_distance()
                    - f64::from(h.base().dimensions().width)
                    - f64::from(self.base().dimensions().width)
                && self.check_elastic_interactions(h)
            {
                self.on_elastic_leash_pull();
            } else {
                self.close_range_leash_behaviour(h);
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
                    let _ = self.set_leashed_to(&holder, None);
                    return;
                }

                if self.tick_count() > DELAYED_LEASH_DROP_TICKS {
                    let _ = self.spawn_at_location(ItemStack::new(&vanilla_items::ITEMS.lead), 0.0);
                    self.remove_leash_state();
                }
            }
            LeashAttachment::FenceKnot(pos) => {
                if let Some(holder) = LeashFenceKnotEntity::get_or_create_knot(&world, pos) {
                    let _ = self.set_leashed_to(&holder, None);
                    return;
                }

                if self.tick_count() > DELAYED_LEASH_DROP_TICKS {
                    let _ = self.spawn_at_location(ItemStack::new(&vanilla_items::ITEMS.lead), 0.0);
                    self.remove_leash_state();
                }
            }
        }
    }

    fn drop_leash(&mut self, caller: Option<&dyn Entity>) {
        if self.leash_holder().is_none() {
            return;
        }

        let holder = self.remove_leash_state();
        let _ = self.spawn_at_location(ItemStack::new(&vanilla_items::ITEMS.lead), 0.0);
        if let Some(holder) = holder {
            self.notify_leash_holder_removed(&holder, caller);
        }
    }

    fn remove_leash(&mut self, caller: Option<&dyn Entity>) {
        if self.leash_holder().is_some() {
            if let Some(holder) = self.remove_leash_state() {
                self.notify_leash_holder_removed(&holder, caller);
            }
        }
    }

    fn remove_leash_state(&mut self) -> Option<SharedEntity> {
        self.mob_base()
            .leash_data
            .take()
            .and_then(|leash_data| leash_data.holder())
    }

    /// Notifies a former leash holder that this mob is no longer leashed to it.
    ///
    /// `caller`, when present, is an entity whose behavior lock is already held
    /// higher up the stack (the one driving this removal). If it is the holder,
    /// we notify it through that borrow instead of taking its lock again —
    /// `parking_lot` mutexes are not reentrant, so re-locking would deadlock
    /// when e.g. a player un-leashes a mob tied to itself, or transfers its own
    /// leashables, while its `Player` mutex is held during inbound-packet
    /// handling. Any other holder is locked normally.
    fn notify_leash_holder_removed(&self, holder: &SharedEntity, caller: Option<&dyn Entity>) {
        if let Some(caller) = caller
            && caller.id() == holder.id()
        {
            caller.notify_leashee_removed(self.as_entity_event_source());
        } else {
            holder.with_entity(|e| e.notify_leashee_removed(self.as_entity_event_source()));
        }
    }

    fn is_within_home(&self) -> bool {
        self.is_within_home_pos(self.block_position())
    }

    fn is_within_home_pos(&self, pos: BlockPos) -> bool {
        let home = self.mob_base_ref().home_restriction;
        home.radius == -1
            || block_pos_distance_sqr(home.position, pos) < home_radius_sqr(home.radius)
    }

    fn is_within_home_vec(&self, pos: DVec3) -> bool {
        let home = &self.mob_base_ref().home_restriction;
        home.radius == -1
            || block_center_distance_sqr(home.position, pos) < home_radius_sqr(home.radius)
    }

    fn set_home_to(&mut self, position: BlockPos, radius: i32) {
        self.mob_base().home_restriction = MobHomeRestriction { position, radius };
    }

    fn home_position(&self) -> BlockPos {
        self.mob_base_ref().home_restriction.position
    }

    fn home_radius(&self) -> i32 {
        self.mob_base_ref().home_restriction.radius
    }

    fn clear_home(&mut self) {
        self.mob_base().home_restriction.radius = -1;
    }

    fn has_home(&self) -> bool {
        self.home_radius() != -1
    }

    fn check_mob_despawn(&mut self) {
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
        // Resolve both checks lock-free: a passenger driving this vehicle holds
        // its own behavior lock while this runs (the vehicle's
        // `controlling_passenger` is queried from the passenger's move control),
        // so locking the passenger here would re-lock it and self-deadlock.
        if self.is_no_ai() || !first_passenger.is_mob() || !first_passenger.can_control_vehicle() {
            return None;
        }

        Some(first_passenger)
    }

    fn get_pathfinding_malus(&self, path_type: PathType) -> f32 {
        self.mob_base_ref().pathfinding_malus.get(path_type)
    }

    /// Vanilla `Entity.getMaxFallDistance` baseline.
    fn max_fall_distance(&self) -> i32 {
        3
    }

    fn set_pathfinding_malus(&mut self, path_type: PathType, malus: f32) {
        self.mob_base().pathfinding_malus.set(path_type, malus);
    }

    fn is_no_ai(&self) -> bool {
        self.mob_flags() & MOB_FLAG_NO_AI != 0
    }

    fn set_no_ai(&mut self, no_ai: bool) {
        self.set_mob_flag(MOB_FLAG_NO_AI, no_ai);
    }

    fn is_left_handed(&self) -> bool {
        self.mob_flags() & MOB_FLAG_LEFT_HANDED != 0
    }

    fn set_left_handed(&mut self, left_handed: bool) {
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
    fn do_hurt_target(&mut self, target: &SharedEntity) -> bool {
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
            .required_value(vanilla_attributes::ATTACK_DAMAGE) as f32;
        let damage_source = self.mob_attack_damage_source();
        let enchantment_context = EnchantmentDamageContext::new(
            target.entity_type(),
            Some(self.entity_type()),
            Some(self.entity_type()),
            &damage_source,
        );
        let damage =
            enchantment_helper::modify_damage(&weapon_item, &enchantment_context, attack_damage);
        // TODO: Apply item attack damage bonuses once item combat behavior exposes them.

        let old_movement = target.velocity();
        let was_hurt = target.with_entity(|target| {
            let was_hurt = target.hurt(&damage_source, damage);
            if was_hurt {
                self.cause_extra_knockback(
                    target,
                    self.get_attack_knockback(target, &weapon_item, &damage_source),
                    old_movement,
                );
                let mut post_attack_context = EnchantmentPostAttackContext::new(
                    target,
                    Some(self.as_entity_event_source_mut()),
                    None,
                    &damage_source,
                    true,
                );
                enchantment_helper::do_post_attack_effects_from_item(
                    &weapon_item,
                    &mut post_attack_context,
                );
            }
            was_hurt
        });
        self.set_last_hurt_mob(Some(target));
        self.play_attack_sound();

        if let Some(user) = self.as_entity_event_source_mut().as_living_entity_mut() {
            enchantment_helper::do_post_piercing_attack_effects(user);
        }
        was_hurt
    }

    /// Returns the damage source used by vanilla `DamageSources.mobAttack`.
    fn mob_attack_damage_source(&self) -> DamageSource {
        // TODO: Use the held item's DAMAGE_TYPE component once it has typed component data.
        DamageSource::environment(&vanilla_damage_types::MOB_ATTACK)
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

    fn set_aggressive(&mut self, aggressive: bool) {
        self.set_mob_flag(MOB_FLAG_AGGRESSIVE, aggressive);
    }

    fn set_mob_flag(&mut self, flag: i8, enabled: bool) {
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
        vehicle.with_mob(|_| ())?;
        Some(vehicle)
    }

    fn set_wanted_position(&mut self, position: DVec3, speed_modifier: f64) {
        if let Some(vehicle) = self.controlled_mob_vehicle()
            && vehicle
                .with_mob(|mob| mob.set_wanted_position(position, speed_modifier))
                .is_some()
        {
            return;
        }

        self.mob_base()
            .controls
            .move_control
            .set_wanted_position(position, speed_modifier);
    }

    fn jump_control_jump(&mut self) {
        self.mob_base().controls.jump_control.jump();
    }

    /// Mirrors vanilla `Mob.setSpeed`: update cached speed and forward AI input.
    fn set_mob_speed(&mut self, speed: f32) {
        self.set_speed(speed);
        let input = self.travel_input();
        self.set_travel_input(LivingTravelInput::new(
            input.sideways(),
            input.vertical(),
            speed,
        ));
    }

    fn mob_server_ai_step(&mut self) {
        self.increment_no_action_time();
        self.mob_base().sensing.tick();
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

    fn tick_path_navigation(&mut self) {
        let Some(world) = self.level() else {
            return;
        };
        let game_time = world.game_time();
        self.mob_base().navigation.tick();
        tick_path_navigation_target(self, &world, game_time, true);
    }

    fn tick_move_control(&mut self) {
        let move_control = {
            let controls = &mut self.mob_base().controls;
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

    fn tick_move_to_control(&mut self, wanted_position: DVec3, speed_modifier: f64) {
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
            .required_value(vanilla_attributes::MOVEMENT_SPEED);
        self.set_mob_speed((speed_modifier * movement_speed) as f32);

        if should_jump_to_wanted_position(self, xd, yd, zd) {
            self.jump_control_jump();
            self.mob_base().controls.move_control.set_jumping();
        }
    }

    fn tick_strafe_control(&mut self, forward: f32, right: f32) {
        let movement_speed = self
            .attributes()
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
        self.mob_base().controls.move_control.set_wait();
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

    fn tick_jumping_control(&mut self, speed_modifier: f64) {
        let movement_speed = self
            .attributes()
            .required_value(vanilla_attributes::MOVEMENT_SPEED);
        self.set_mob_speed((speed_modifier * movement_speed) as f32);
        if self.on_ground()
            || (self.is_in_water() || self.is_in_lava()) && self.is_affected_by_fluids()
        {
            self.mob_base().controls.move_control.set_wait();
        }
    }

    fn tick_look_control(&mut self) {
        let look_control = {
            let mut controls = self.mob_base().controls;
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

    fn clamp_head_rotation_to_body_when_pathing(&mut self) {
        if self.mob_base_ref().navigation.is_done() {
            return;
        }

        self.set_y_head_rot(rotate_if_necessary(
            self.y_head_rot(),
            self.y_body_rot(),
            self.max_head_y_rot(),
        ));
    }

    fn tick_jump_control(&mut self) {
        let jumping = self.mob_base().controls.jump_control.tick();
        self.set_jumping(jumping);
    }

    fn update_control_flags(&mut self) {
        let no_controller = self
            .controlling_passenger()
            .is_none_or(|passenger| !passenger.is_mob());
        let not_in_boat = self
            .vehicle()
            .is_none_or(|vehicle| !vehicle.entity_type().is_abstract_boat);

        let selector = &mut self.mob_base().goal_selector;
        selector.set_control(GoalControl::Move, no_controller);
        selector.set_control(GoalControl::Jump, no_controller && not_in_boat);
        selector.set_control(GoalControl::Look, no_controller);
    }

    fn tick_body_rotation_control(&mut self) {
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
        let update = self.mob_base().controls.body_rotation_control.tick(input);
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
    mob: &mut M,
    world: &Arc<World>,
    game_time: i64,
    can_update_path: bool,
) {
    let (target, speed_modifier) = {
        let can_float = mob.mob_base_ref().navigation.can_float();
        let mob_position = ground_navigation_temp_mob_pos(mob, world.as_ref(), can_float);
        let context = NavigationTickContext {
            mob_position,
            mob_bounding_box_width: mob.bounding_box().width(),
            mob_speed: mob.get_speed(),
            game_time,
        };
        let next_target = if can_update_path {
            mob.mob_base().navigation.next_move_target(context)
        } else {
            let on_ground = mob.on_ground();
            mob.mob_base()
                .navigation
                .next_move_target_without_path_update(context, on_ground)
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
        vehicle.with_pathfinder(|_| ())?;
        Some(vehicle)
    }

    fn get_walk_target_value(&self, pos: BlockPos) -> f32 {
        self.as_animal()
            .map_or(0.0, |animal| animal.animal_walk_target_value(pos))
    }

    fn has_line_of_sight_cached(&mut self, target: &dyn Entity) -> bool {
        let id = target.id();
        // Read the cache first, then run the (immutable-`self`) line-of-sight
        // test outside the `&mut` base borrow, then record the result.
        if let Some(cached) = self.mob_base_ref().sensing.cached_line_of_sight(id) {
            return cached;
        }
        let has_line_of_sight = self.has_line_of_sight(target);
        self.mob_base()
            .sensing
            .record_line_of_sight(id, has_line_of_sight);
        has_line_of_sight
    }

    fn can_update_path(&self) -> bool {
        self.on_ground() || self.is_in_water() || self.is_in_lava() || self.is_passenger()
    }

    fn can_path_to_targets_below_surface(&self) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(result) =
                vehicle.with_pathfinder(|pathfinder| pathfinder.can_path_to_targets_below_surface())
        {
            return result;
        }

        self.mob_base_ref()
            .navigation
            .can_path_to_targets_below_surface()
    }

    fn can_reach_living_target(&mut self, target: &dyn LivingEntity) -> bool {
        let target_pos = target.block_position();
        self.create_path_to(target_pos, 0)
            .is_some_and(|path| path_end_node_can_reach_target(&path, target_pos))
    }

    fn tick_pathfinder_path_navigation(&mut self) {
        let Some(world) = self.level() else {
            return;
        };
        let game_time = world.game_time();
        let recompute_request = {
            let can_update_path = self.can_update_path();
            let navigation = &mut self.mob_base().navigation;
            navigation.tick();
            navigation.take_delayed_recompute_request(game_time, can_update_path)
        };
        if let Some(request) = recompute_request {
            self.recompute_path(request);
        }

        tick_path_navigation_target(self, &world, game_time, self.can_update_path());
    }

    fn tick_pathfinder_goal_selectors(&mut self)
    where
        Self: Sized,
    {
        let id_based_tick_count = self.tick_count().wrapping_add(self.id());
        let running_goals_only = id_based_tick_count % 2 != 0 && self.tick_count() > 1;

        self.tick_goal_selector(|m| &mut m.mob_base().target_selector, running_goals_only);
        self.tick_goal_selector(|m| &mut m.mob_base().goal_selector, running_goals_only);
    }

    /// Ticks one of this mob's goal selectors against the mob itself.
    ///
    /// `select` returns the selector mutex to tick; it lives *inside* `self`
    /// (e.g. `self.mob_base().goal_selector()`). This indirection exists for a
    /// borrow-checker reason: the naive
    /// `self.mob_base().goal_selector().lock().tick(self)` is rejected because
    /// the lock guard borrows `self` immutably for as long as it lives, which
    /// conflicts with handing `&mut self` to the goals. We grab the selector as
    /// a raw pointer (the cast ends the shared borrow), then lock through it so
    /// the guard's lifetime is detached from `self`.
    ///
    /// While the tick runs the selector mutex is held and the very same `self`
    /// is handed to each goal as `&mut self`. **A goal invoked this way must not
    /// touch the goal selectors of the `self` it receives** — neither this
    /// selector nor any other on the same mob (e.g.
    /// `self.mob_base().goal_selector()` / `target_selector()`). Doing so is a
    /// re-entrant access of state that is already borrowed/locked by this very
    /// tick: the held mutex means re-locking deadlocks, and reaching the
    /// selector's goals through `&mut self` while we are iterating them aliases
    /// the same data. In short, goals drive the mob, they do not re-tick or
    /// mutate the mob's goal selectors. Locking unrelated mob state is fine.
    fn tick_goal_selector(
        &mut self,
        select: impl FnOnce(&mut Self) -> &mut GoalSelector,
        running_goals_only: bool,
    ) where
        Self: Sized,
    {
        let selector = select(self) as *mut GoalSelector;
        // SAFETY: `selector` points into `self`, which is borrowed mutably for
        // the whole call and so stays alive and pinned. Going through the raw
        // pointer detaches the guard's lifetime from `self`, which is what lets
        // the guard and the `&mut self` passed to the goals coexist.
        let guard = unsafe { &mut *selector };
        if running_goals_only {
            guard.tick_running_goals(self, false);
        } else {
            guard.tick(self);
        }
    }

    fn is_stable_destination(&self, pos: BlockPos) -> bool {
        self.level()
            .is_some_and(|world| world.get_block_state(pos.below()).is_solid_render())
    }

    fn create_path_to(&mut self, target: BlockPos, reach_range: i32) -> Option<Path> {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(result) =
                vehicle.with_pathfinder(|pathfinder| pathfinder.create_path_to(target, reach_range))
        {
            return result;
        }

        let world = self.level()?;
        if !world.has_full_chunk(ChunkPos::from_block_pos(target)) {
            return None;
        }

        let target = path_target_for_mob(self, world.as_ref(), target);
        let targets = [target];
        self.create_path_to_targets(&world, &targets, reach_range)
    }

    fn recompute_path(&mut self, request: NavigationRecomputeRequest) {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && vehicle
                .with_pathfinder(|pathfinder| pathfinder.recompute_path(request))
                .is_some()
        {
            return;
        }

        let path = self.create_path_to(request.target_pos, request.reach_range);
        self.mob_base()
            .navigation
            .complete_recompute_path(path, request.game_time);
    }

    fn move_to_pos(&mut self, target: DVec3, speed_modifier: f64) -> bool {
        self.move_to_pos_with_reach(target, 1, speed_modifier)
    }

    fn move_to_pos_with_reach(
        &mut self,
        target: DVec3,
        reach_range: i32,
        speed_modifier: f64,
    ) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(result) = vehicle.with_pathfinder(|pathfinder| {
                pathfinder.move_to_pos_with_reach(target, reach_range, speed_modifier)
            })
        {
            return result;
        }

        let target_pos = BlockPos::containing(target.x, target.y, target.z);
        let Some(world) = self.level() else {
            self.mob_base().navigation.stop();
            return false;
        };
        if !world.has_full_chunk(ChunkPos::from_block_pos(target_pos)) {
            self.mob_base().navigation.stop();
            return false;
        }

        let target_pos = path_target_for_mob(self, world.as_ref(), target_pos);
        let targets = [target_pos];

        let mob_position = self.position();
        if self.mob_base().navigation.reuse_current_path_to_targets(
            world.as_ref(),
            &targets,
            speed_modifier,
            mob_position,
        ) {
            return true;
        }

        let path = self.create_path_to_targets(&world, &targets, reach_range);
        self.move_to_path(path, speed_modifier)
    }

    fn move_to_path(&mut self, path: Option<Path>, speed_modifier: f64) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle() {
            // The closure consumes `path`; only enter it when there is a
            // pathfinder vehicle so we don't lose `path` on the fallthrough.
            if let Some(result) =
                vehicle.with_pathfinder(|pathfinder| pathfinder.move_to_path(path, speed_modifier))
            {
                return result;
            }
            return false;
        }

        let Some(world) = self.level() else {
            self.mob_base().navigation.stop();
            return false;
        };

        let position = self.position();
        let navigation = &mut self.mob_base().navigation;
        let Some(path) = path else {
            navigation.stop();
            return false;
        };

        navigation.move_to(world.as_ref(), path, speed_modifier, position)
    }

    fn is_path_finding(&self) -> bool {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(result) = vehicle.with_pathfinder(|pathfinder| pathfinder.is_path_finding())
        {
            return result;
        }

        !self.mob_base_ref().navigation.is_done()
    }

    fn is_panicking(&self) -> bool {
        self.mob_base_ref().goal_selector.has_running_panic_goal()
    }

    fn create_path_to_targets(
        &mut self,
        world: &Arc<World>,
        targets: &[BlockPos],
        reach_range: i32,
    ) -> Option<Path> {
        if let Some(vehicle) = self.controlled_pathfinder_vehicle()
            && let Some(result) = vehicle.with_pathfinder(|pathfinder| {
                pathfinder.create_path_to_targets(world, targets, reach_range)
            })
        {
            return result;
        }

        if targets.is_empty()
            || self.position().y < f64::from(world.min_y())
            || !self.can_update_path()
        {
            return None;
        }

        let follow_range = self
            .attributes()
            .required_value(vanilla_attributes::FOLLOW_RANGE);
        let max_path_length = {
            let navigation = &mut self.mob_base().navigation;
            navigation.update_pathfinder_max_visited_nodes(follow_range);
            navigation.max_path_length(follow_range)
        };

        let mob_position = self.block_position();
        let settings = MobPathSettings::from_mob(self);
        let mut evaluator = WalkNodeEvaluator::new(settings);

        // Move the navigation out of the mob so it can be borrowed mutably while
        // the collision closure borrows the rest of `self` immutably; restore it
        // afterwards.
        let mut navigation = std::mem::take(&mut self.mob_base().navigation);
        let path = {
            let collision_world =
                WorldCollisionProvider::for_path_navigation(world, self.as_entity_event_source());
            let mut collision = |aabb| {
                collision_world.has_entity_context_collision(
                    aabb,
                    self.position().y,
                    self.is_descending(),
                )
            };

            navigation.create_path(
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
        };
        self.mob_base().navigation = navigation;
        path
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
    use steel_registry::vanilla_entities;
    use steel_registry::{
        REGISTRY, test_support::init_test_registry, vanilla_attributes, vanilla_blocks,
        vanilla_damage_types,
    };
    use steel_utils::locks::SyncMutex;
    use steel_utils::{BlockPos, BlockStateId};

    use super::{
        can_attempt_equipment_drop, find_ground_path_target_surface, path_end_node_can_reach_target,
    };
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
        base: Weak<EntityBase>,
        base_strong: Option<Arc<EntityBase>>,
        entity_type: EntityTypeRef,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        flags: i8,
        health: f32,
        nearest_player_distance_sqr: Option<f64>,
        remove_when_far_away: bool,
        controlling_passenger: Option<SharedEntity>,
    }

    impl DespawnTestMob {
        /// Returns the shared handle backing this plain test mob.
        fn entity(&self) -> SharedEntity {
            self.base_strong.clone().expect("entity already shared")
        }

        fn new(nearest_player_distance_sqr: Option<f64>, remove_when_far_away: bool) -> Self {
            // Build a real (strongly held) base so `base()`/base-backed state
            // works and the registry is initialized — mirrors `with_position_self`.
            Self::with_position_self(
                0,
                DVec3::ZERO,
                nearest_player_distance_sqr,
                remove_when_far_away,
            )
        }

        fn shared(
            nearest_player_distance_sqr: Option<f64>,
            remove_when_far_away: bool,
        ) -> SharedEntity {
            let entity_type = &vanilla_entities::PIG;

            EntityBase::pack_with(
                0,
                DVec3::ZERO,
                entity_type.dimensions,
                Weak::new(),
                |base| Self {
                    base,
                    base_strong: None,
                    entity_type,
                    living_base: LivingEntityBase::new(entity_type),
                    mob_base: MobBase::new(),
                    flags: 0,
                    health: 10.0,
                    nearest_player_distance_sqr,
                    remove_when_far_away,
                    controlling_passenger: None,
                },
            )
        }

        fn with_position_self(
            id: i32,
            position: DVec3,
            nearest_player_distance_sqr: Option<f64>,
            remove_when_far_away: bool,
        ) -> Self {
            let entity_type = &vanilla_entities::PIG;

            let base = Arc::new(EntityBase::new(
                id,
                position,
                entity_type.dimensions,
                Weak::new(),
            ));
            Self {
                base: Arc::downgrade(&base),
                base_strong: Some(base),
                entity_type,
                living_base: LivingEntityBase::new(entity_type),
                mob_base: MobBase::new(),
                flags: 0,
                health: 10.0,
                nearest_player_distance_sqr,
                remove_when_far_away,
                controlling_passenger: None,
            }
        }

        fn with_position(
            id: i32,
            position: DVec3,
            nearest_player_distance_sqr: Option<f64>,
            remove_when_far_away: bool,
        ) -> SharedEntity {
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
        ) -> SharedEntity {
            init_test_registry();

            EntityBase::pack_with(id, position, entity_type.dimensions, Weak::new(), |base| {
                Self {
                    // The entity is attached to the base by `pack_with`; the
                    // returned `Arc` owns it, so no self-held strong ref (and
                    // the weak isn't upgradable yet inside `new_cyclic`).
                    base_strong: None,
                    base,
                    entity_type,
                    living_base: LivingEntityBase::new(entity_type),
                    mob_base: MobBase::new(),
                    flags: 0,
                    health: 10.0,
                    nearest_player_distance_sqr,
                    remove_when_far_away,
                    controlling_passenger: None,
                }
            })
        }

        fn set_controlling_passenger(&mut self, passenger: SharedEntity) {
            self.controlling_passenger = Some(passenger);
        }
    }

    crate::entity::impl_test_downcast_type!(DespawnTestMob);

    impl Entity for DespawnTestMob {
        fn base_weak(&self) -> &Weak<EntityBase> {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn is_living_entity(&self) -> bool {
            true
        }

        fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
            Some(self)
        }

        fn as_living_entity_mut(&mut self) -> Option<&mut dyn LivingEntity> {
            Some(self)
        }

        fn is_mob(&self) -> bool {
            true
        }

        fn as_mob(&self) -> Option<&dyn Mob> {
            Some(self)
        }

        fn as_mob_mut(&mut self) -> Option<&mut dyn Mob> {
            Some(self)
        }

        fn controlling_passenger(&self) -> Option<SharedEntity> {
            self.controlling_passenger.clone()
        }

        fn hurt(&mut self, source: &DamageSource, amount: f32) -> bool {
            LivingEntity::hurt_server(self, source, amount)
        }
    }

    impl LivingEntity for DespawnTestMob {
        fn living_base(&mut self) -> &mut LivingEntityBase {
            &mut self.living_base
        }

        fn living_base_ref(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            self.health
        }

        fn set_health(&mut self, health: f32) {
            self.health = health;
        }
    }

    struct HiddenTarget {
        base: Weak<EntityBase>,
        living_base: LivingEntityBase,
        health: SyncMutex<f32>,
    }

    impl HiddenTarget {
        fn shared(id: i32) -> SharedEntity {
            EntityBase::pack_with(
                id,
                DVec3::ZERO,
                vanilla_entities::PIG.dimensions,
                Weak::new(),
                |base| Self {
                    base,
                    living_base: LivingEntityBase::new(&vanilla_entities::PIG),
                    health: SyncMutex::new(10.0),
                },
            )
        }
    }

    crate::entity::impl_test_downcast_type!(HiddenTarget);

    impl Entity for HiddenTarget {
        fn base_weak(&self) -> &Weak<EntityBase> {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }

        fn is_living_entity(&self) -> bool {
            true
        }

        fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
            Some(self)
        }

        fn as_living_entity_mut(&mut self) -> Option<&mut dyn LivingEntity> {
            Some(self)
        }
    }

    impl LivingEntity for HiddenTarget {
        fn living_base(&mut self) -> &mut LivingEntityBase {
            &mut self.living_base
        }
        fn living_base_ref(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&mut self, health: f32) {
            *self.health.lock() = health;
        }

        fn can_be_seen_as_enemy(&self) -> bool {
            false
        }
    }

    impl Mob for DespawnTestMob {
        fn mob_base(&mut self) -> &mut MobBase {
            &mut self.mob_base
        }

        fn mob_base_ref(&self) -> &MobBase {
            &self.mob_base
        }

        fn mob_flags(&self) -> i8 {
            self.flags
        }

        fn set_mob_flags(&mut self, flags: i8) {
            self.flags = flags;
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
        base: Weak<EntityBase>,
        entity_type: EntityTypeRef,
    }

    impl MobControlVehicleEntity {
        fn new(id: i32, entity_type: EntityTypeRef) -> SharedEntity {
            EntityBase::pack_with(
                id,
                DVec3::ZERO,
                entity_type.dimensions,
                Weak::new(),
                |base| Self { base, entity_type },
            )
        }
    }

    crate::entity::impl_test_downcast_type!(MobControlVehicleEntity);

    impl Entity for MobControlVehicleEntity {
        fn base_weak(&self) -> &Weak<EntityBase> {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }
    }

    #[test]
    fn mob_base_uses_vanilla_fire_path_malus_defaults() {
        let base = MobBase::new();
        let malus = &base.pathfinding_malus;

        assert_eq!(
            malus.get(PathType::FireInNeighbor).to_bits(),
            8.0_f32.to_bits()
        );
        assert_eq!(malus.get(PathType::Fire).to_bits(), 16.0_f32.to_bits());
        assert_eq!(malus.get(PathType::Water).to_bits(), 8.0_f32.to_bits());
    }

    #[test]
    fn pathfinder_mob_reads_below_surface_capability_from_navigation() {
        let mut mob = DespawnTestMob::new(None, false);

        assert!(!mob.can_path_to_targets_below_surface());

        mob.mob_base()
            .navigation
            .set_can_path_to_targets_below_surface(true);

        assert!(mob.can_path_to_targets_below_surface());
    }

    #[test]
    fn mob_server_ai_step_increments_no_action_time() {
        let mut mob = DespawnTestMob::new(None, false);

        mob.set_no_action_time(12);
        mob.mob_server_ai_step();

        assert_eq!(mob.no_action_time(), 13);
    }

    #[test]
    fn mob_control_flags_enable_goals_without_controller_or_boat() {
        let mut mob = DespawnTestMob::new(None, false);
        {
            let selector = &mut mob.mob_base().goal_selector;
            selector.disable_control(GoalControl::Move);
            selector.disable_control(GoalControl::Jump);
            selector.disable_control(GoalControl::Look);
        }

        mob.update_control_flags();

        let selector = &mut mob.mob_base().goal_selector;
        assert!(!selector.is_control_disabled(GoalControl::Move));
        assert!(!selector.is_control_disabled(GoalControl::Jump));
        assert!(!selector.is_control_disabled(GoalControl::Look));
    }

    #[test]
    fn mob_control_flags_disable_goals_for_mob_controller() {
        let mut mob = DespawnTestMob::new(None, false);
        let controller: SharedEntity = DespawnTestMob::with_position(2, DVec3::ZERO, None, false);
        mob.set_controlling_passenger(controller);

        mob.update_control_flags();

        let selector = &mut mob.mob_base().goal_selector;
        assert!(selector.is_control_disabled(GoalControl::Move));
        assert!(selector.is_control_disabled(GoalControl::Jump));
        assert!(selector.is_control_disabled(GoalControl::Look));
    }

    #[test]
    fn mob_control_flags_disable_jump_when_riding_boat() {
        let mob_entity = DespawnTestMob::shared(None, false);
        let boat = MobControlVehicleEntity::new(2, &vanilla_entities::OAK_BOAT);
        EntityBase::restore_passenger_relationship(&boat, &mob_entity);

        mob_entity.with_mob(|mob| {
            mob.update_control_flags();

            let selector = &mut mob.mob_base().goal_selector;
            assert!(!selector.is_control_disabled(GoalControl::Move));
            assert!(selector.is_control_disabled(GoalControl::Jump));
            assert!(!selector.is_control_disabled(GoalControl::Look));
        });
    }

    #[test]
    fn mob_target_stores_living_target_weakly() {
        let mut mob = DespawnTestMob::new(None, false);
        let target: SharedEntity = DespawnTestMob::with_position(2, DVec3::ZERO, None, false);

        assert!(mob.set_target(Some(&target)));

        let stored = mob.target().expect("living target should be stored");
        assert!(Arc::ptr_eq(&stored, &target));
    }

    #[test]
    fn mob_target_can_be_cleared() {
        let mut mob = DespawnTestMob::new(None, false);
        let target: SharedEntity = DespawnTestMob::with_position(2, DVec3::ZERO, None, false);
        assert!(mob.set_target(Some(&target)));

        assert!(mob.set_target(None));

        assert!(mob.target().is_none());
    }

    #[test]
    fn mob_target_expires_with_target_entity() {
        let mut mob = DespawnTestMob::new(None, false);
        {
            let target: SharedEntity = DespawnTestMob::with_position(2, DVec3::ZERO, None, false);
            assert!(mob.set_target(Some(&target)));
        }

        assert!(mob.target().is_none());
    }

    #[test]
    fn mob_target_rejects_non_living_entities() {
        let mut mob = DespawnTestMob::new(None, false);
        let target: SharedEntity = MobControlVehicleEntity::new(2, &vanilla_entities::OAK_BOAT);

        assert!(!mob.set_target(Some(&target)));

        assert!(mob.target().is_none());
    }

    #[test]
    fn mob_target_rejects_targets_it_cannot_attack() {
        let mut mob = DespawnTestMob::new(None, false);
        let target = HiddenTarget::shared(2);

        assert!(!mob.set_target(Some(&target)));

        assert!(mob.target().is_none());
    }

    #[test]
    fn mob_target_filters_invalid_target_without_clearing_stored_target() {
        let mut mob = DespawnTestMob::new(None, false);
        let target = DespawnTestMob::with_position(2, DVec3::ZERO, None, false);

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
        let mut mob = DespawnTestMob::new(None, false);
        let previous = DespawnTestMob::with_position(2, DVec3::ZERO, None, false);
        let invalid = HiddenTarget::shared(3);

        assert!(mob.set_target(Some(&previous)));
        assert!(!mob.set_target(Some(&invalid)));

        assert!(mob.target().is_none());
    }

    #[test]
    fn melee_attack_range_uses_vanilla_default_reach() {
        let mob = DespawnTestMob::new(None, false);
        let close_target =
            DespawnTestMob::with_position_self(2, DVec3::new(1.7, 0.0, 0.0), None, false);
        let far_target =
            DespawnTestMob::with_position_self(3, DVec3::new(1.8, 0.0, 0.0), None, false);

        assert!(mob.is_within_melee_attack_range(&close_target));
        assert!(!mob.is_within_melee_attack_range(&far_target));
    }

    #[test]
    fn melee_attack_range_uses_vehicle_expanded_attack_box() {
        let mob_entity: SharedEntity =
            DespawnTestMob::with_position(1, DVec3::new(4.0, 0.0, 0.0), None, false);
        let target = DespawnTestMob::with_position_self(2, DVec3::new(1.1, 0.0, 0.0), None, false);

        assert!(
            !mob_entity
                .with_mob(|mob| mob.is_within_melee_attack_range(&target))
                .unwrap()
        );

        let vehicle: SharedEntity = MobControlVehicleEntity::new(3, &vanilla_entities::PIG);
        EntityBase::restore_passenger_relationship(&vehicle, &mob_entity);

        assert!(
            mob_entity
                .with_mob(|mob| mob.is_within_melee_attack_range(&target))
                .unwrap()
        );
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
    fn mob_base_tick_increments_ambient_sound_time_when_roll_fails() {
        let mut mob = DespawnTestMob::new(None, false);

        mob.mob_base_tick();

        assert_eq!(mob.mob_base().ambient_sound_time, 1);
    }

    #[test]
    fn mob_base_tick_resets_ambient_sound_time_after_vanilla_roll() {
        let mut mob = DespawnTestMob::new(None, false);

        mob.mob_base().ambient_sound_time = 1000;
        mob.mob_base_tick();

        assert_eq!(mob.mob_base().ambient_sound_time, -80);
    }

    #[test]
    fn mob_hurt_sound_resets_ambient_sound_time() {
        let mut mob = DespawnTestMob::new(None, false);
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        mob.mob_base().ambient_sound_time = 12;
        LivingEntity::play_hurt_sound(&mut mob, &source);

        assert_eq!(mob.mob_base().ambient_sound_time, -80);
    }

    #[test]
    fn mob_do_hurt_target_applies_attack_damage_and_records_target() {
        let mob = DespawnTestMob::with_entity_type(
            1,
            DVec3::ZERO,
            &vanilla_entities::ZOMBIE,
            None,
            false,
        );

        let mut mob = mob.lock_entity();
        let mob: &mut DespawnTestMob = unsafe { mob.downcast_unchecked() };

        mob.attributes_mut()
            .set_base_value(vanilla_attributes::ATTACK_DAMAGE, 4.0);
        let target: SharedEntity =
            DespawnTestMob::with_position(2, DVec3::new(1.0, 0.0, 0.0), None, false);

        assert!(mob.do_hurt_target(&target));

        let target_health = target.with_living(|living| living.get_health()).unwrap();
        assert_eq!(target_health.to_bits(), 6.0_f32.to_bits());
        let stored_target = mob
            .last_hurt_mob()
            .expect("successful mob attack should record target");
        assert!(Arc::ptr_eq(&stored_target, &target));
    }

    #[test]
    fn mob_do_hurt_target_applies_vanilla_extra_knockback() {
        let mob = DespawnTestMob::with_entity_type(
            1,
            DVec3::ZERO,
            &vanilla_entities::ZOMBIE,
            None,
            false,
        );

        let mut mob = mob.lock_entity();
        let mob: &mut DespawnTestMob = unsafe { mob.downcast_unchecked() };
        {
            let attributes = mob.attributes_mut();
            attributes.set_base_value(vanilla_attributes::ATTACK_DAMAGE, 4.0);
            attributes.set_base_value(vanilla_attributes::ATTACK_KNOCKBACK, 2.0);
        }
        mob.set_velocity(DVec3::new(1.0, 0.0, 1.0));
        let target: SharedEntity =
            DespawnTestMob::with_position(2, DVec3::new(1.0, 0.0, 0.0), None, false);

        assert!(mob.do_hurt_target(&target));

        assert_eq!(mob.velocity().x.to_bits(), 0.6_f64.to_bits());
        assert_eq!(mob.velocity().z.to_bits(), 0.6_f64.to_bits());
        assert!(target.velocity().length_squared() > 0.0);
        assert!(target.needs_velocity_sync());
    }

    #[test]
    fn mob_look_control_rotates_head_yaw_without_turning_body_yaw() {
        let mut mob = DespawnTestMob::new(None, false);
        mob.set_rotation((0.0, 0.0));
        mob.set_y_body_rot(0.0);
        mob.set_y_head_rot(0.0);
        let position = mob.position();
        let eye_y = mob.get_eye_y();
        mob.mob_base().controls.look_control.set_look_at(
            DVec3::new(position.x + 1.0, eye_y, position.z),
            DEFAULT_LOOK_Y_MAX_ROT_SPEED,
            DEFAULT_LOOK_X_MAX_ROT_ANGLE,
        );

        Mob::tick_look_control(&mut mob);

        assert_eq!(mob.rotation(), (0.0, 0.0));
        assert_eq!(mob.y_body_rot(), 0.0);
        assert_eq!(mob.y_head_rot(), -10.0);
    }

    #[test]
    fn mob_look_control_returns_head_yaw_toward_body_when_idle() {
        let mut mob = DespawnTestMob::new(None, false);
        mob.set_rotation((0.0, 20.0));
        mob.set_y_body_rot(90.0);
        mob.set_y_head_rot(0.0);

        Mob::tick_look_control(&mut mob);

        assert_eq!(mob.rotation(), (0.0, 0.0));
        assert_eq!(mob.y_body_rot(), 90.0);
        assert_eq!(mob.y_head_rot(), 10.0);
    }

    #[test]
    fn mob_body_rotation_control_uses_tick_position_delta() {
        let mut mob = DespawnTestMob::new(None, false);
        mob.set_old_position(DVec3::ZERO);
        mob.entity().set_position_local(DVec3::new(1.0, 0.0, 0.0));
        mob.set_rotation((90.0, 0.0));
        mob.set_y_body_rot(0.0);
        mob.set_y_head_rot(200.0);

        Mob::tick_body_rotation_control(&mut mob);

        assert_eq!(mob.y_body_rot(), 90.0);
        assert_eq!(mob.y_head_rot(), 165.0);
    }

    #[test]
    fn mob_tick_leash_applies_default_elastic_pull() {
        let mob = DespawnTestMob::with_position(1, DVec3::ZERO, None, false);
        let holder: SharedEntity =
            DespawnTestMob::with_position(2, DVec3::new(7.0, 0.0, 0.0), None, false);

        let mut mob = mob.lock_entity();
        let mob: &mut DespawnTestMob = unsafe { mob.downcast_unchecked() };
        assert!(mob.set_leashed_to(&holder, None));

        mob.tick_leash();

        assert!(mob.velocity().x > 0.0);
        assert!(mob.velocity().z < 0.0);
        assert!(mob.needs_velocity_sync());
        assert!(mob.rotation().0 < 0.0);
        assert!(mob.is_leashed());
    }

    #[test]
    fn mob_despawn_resets_no_action_time_near_player() {
        let mut mob = DespawnTestMob::new(Some(31.0 * 31.0), false);

        mob.set_no_action_time(42);
        mob.check_mob_despawn();

        assert_eq!(mob.no_action_time(), 0);
        assert!(!mob.is_removed());
    }

    #[test]
    fn mob_despawn_discards_far_removable_mob() {
        let mut mob = DespawnTestMob::new(Some(129.0 * 129.0), true);

        mob.check_mob_despawn();

        assert!(mob.is_removed());
    }

    #[test]
    fn mob_persistence_resets_no_action_time_and_blocks_removal() {
        let mut mob = DespawnTestMob::new(Some(129.0 * 129.0), true);

        mob.set_no_action_time(42);
        mob.set_persistence_required();
        mob.check_mob_despawn();

        assert_eq!(mob.no_action_time(), 0);
        assert!(!mob.is_removed());
    }

    #[test]
    fn mob_home_restriction_uses_vanilla_radius() {
        let mut mob = DespawnTestMob::new(None, false);

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
