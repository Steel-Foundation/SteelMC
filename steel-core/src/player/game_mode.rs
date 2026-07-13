//! Game mode specific logic for player interactions.
//!
//! This module implements the logic from Java's `ServerPlayerGameMode`, particularly
//! the `useItemOn` method that handles block placement and block interactions.

use std::mem::swap;
use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::{
    CBlockChangedAck, CBlockUpdate, CChangeDifficulty, CGameEvent, COpenSignEditor,
    CPlayerInfoUpdate, CSetCamera, CSetEntityMotion, CSetHeldSlot, GameEventType, PlayerAction,
    SAttack, SInteract, SPickItemFromBlock, SPlayerAction, SSignUpdate, SSpectatorAction, SUseItem,
    SUseItemOn,
};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::damage_type::DamageType;
use steel_registry::data_components::components::PiercingWeapon;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::{SoundEventHolder, SoundEventRef};
use steel_registry::{REGISTRY, vanilla_attributes, vanilla_damage_types, vanilla_entities};
use steel_utils::entity_events::EntityStatus;
use steel_utils::types::{Difficulty, GameType, InteractionHand};
use steel_utils::{BlockPos, Downcast as _, Identifier, WorldAabb};
use text_components::TextComponent;
use text_components::translation::TranslatedMessage;

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockCollisionContext, BlockHitResult, ITEM_BEHAVIORS, InteractionResult,
    InventoryAccess, UseOnContext,
};
use crate::block_entity::BlockEntity;
use crate::block_entity::entities::SignBlockEntity;
use crate::command::player_can_change_difficulty;
use crate::enchantment_helper::{self, EnchantmentDamageContext, EnchantmentPostAttackContext};
use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, LivingEntity, SharedEntity};
use crate::inventory::equipment::EquipmentSlot;
use crate::inventory::menu::Menu;
use crate::physics::collision::{CollisionWorld, WorldCollisionProvider};
use crate::physics::shapes;
use crate::player::Player;
use crate::player::block_breaking::BlockBreakAction;
use crate::player::movement::wrap_degrees;
use crate::player::player_inventory::PlayerInventory;
use crate::world::{ClipBlockShape, ClipFluid, World};
use steel_utils::axis::Axis;

const CREATIVE_BLOCK_RANGE_MODIFIER_AMOUNT: f64 = 0.5;
const CREATIVE_ENTITY_RANGE_MODIFIER_AMOUNT: f64 = 2.0;
const ATTACK_RANGE_BUFFER: f64 = 3.0;
const ENTITY_INTERACTION_RANGE_BUFFER: f64 = 3.0;
const FLIGHT_DISABLE_RANGE: f64 = 1.0;

/// Handles using an item on a block.
///
/// This implements the logic from Java's `ServerPlayerGameMode.useItemOn()`.
///
/// # Flow
/// 1. Spectator mode: Only allow opening menus (currently returns Pass)
/// 2. Check if block interaction should be suppressed (sneaking + holding items)
/// 3. If not suppressed: Call block's `use_item_on` method
/// 4. If block returns `TryEmptyHandInteraction` and main hand: Call block's `use_without_item`
/// 5. If item not empty: Call item behavior's `use_on` for placement
/// 6. Handle creative mode infinite materials
pub fn use_item_on(
    player: &Player,
    world: &Arc<World>,
    hand: InteractionHand,
    hit_result: &BlockHitResult,
) -> InteractionResult {
    let pos = hit_result.block_pos;
    let state = world.get_block_state(pos);

    // Spectator mode: can only open menus
    // TODO: Implement menu providers for blocks like chests
    if player.game_mode() == GameType::Spectator {
        return InteractionResult::Pass;
    }

    // Check if block interaction should be suppressed (sneaking + holding items in either hand)
    let have_something = {
        let inv = player.inventory.lock();
        !inv.get_item_in_hand(InteractionHand::MainHand).is_empty()
            || !inv.get_item_in_hand(InteractionHand::OffHand).is_empty()
    };

    let suppress_block_use = player.is_secondary_use_active() && have_something;

    // Get behavior registries
    let block_behaviors = &*BLOCK_BEHAVIORS;
    let item_behaviors = &*ITEM_BEHAVIORS;

    // Try block interaction first (if not suppressed).
    // No inventory lock held — block behaviors may need inventory access (e.g. opening chests).
    if !suppress_block_use {
        let Some(block) = REGISTRY.blocks.by_state_id(state) else {
            return InteractionResult::Pass;
        };
        let behavior = block_behaviors.get_behavior(block);

        let mut inventory_access = InventoryAccess::new(player.inventory.clone(), hand);

        let block_result = behavior.use_item_on(
            state,
            world,
            pos,
            player,
            hand,
            hit_result,
            &mut inventory_access,
        );

        if block_result.consumes_action() {
            return block_result;
        }

        if matches!(block_result, InteractionResult::TryEmptyHandInteraction)
            && hand == InteractionHand::MainHand
        {
            let empty_result = behavior.use_without_item(
                state,
                world,
                pos,
                player,
                hit_result,
                &mut inventory_access,
            );

            if empty_result.consumes_action() {
                return empty_result;
            }
        }
    }

    let inventory_access = InventoryAccess::new(player.inventory.clone(), hand);
    let (is_empty, original_count, item_ref, stack_before_use) =
        inventory_access.with_item(|item| (item.is_empty(), item.count, item.item, item.clone()));

    if !is_empty {
        if player.is_item_on_cooldown(&stack_before_use) {
            return InteractionResult::Pass;
        }

        let mut context = UseOnContext::new(
            player,
            hand,
            hit_result.clone(),
            world,
            player.inventory.clone(),
        );
        let item_behavior = item_behaviors.get_behavior(item_ref);
        let result = item_behavior.use_on(&mut context);

        // Restore count for creative mode (infinite materials)
        if player.has_infinite_materials() {
            context.inv.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }

        return result;
    }

    InteractionResult::Pass
}

/// Handles using an item (general usage like right-clicking air).
///
/// This implements logic similar to `ServerPlayerGameMode.useItem()`.
pub fn use_item(player: &Player, world: &Arc<World>, hand: InteractionHand) -> InteractionResult {
    // Spectator mode: can only open menus
    if player.game_mode() == GameType::Spectator {
        return InteractionResult::Pass;
    }

    let inventory_access = InventoryAccess::new(player.inventory.clone(), hand);
    let (is_empty, original_count, item_ref, stack_before_use) =
        inventory_access.with_item(|item| (item.is_empty(), item.count, item.item, item.clone()));

    if !is_empty {
        if player.is_item_on_cooldown(&stack_before_use) {
            return InteractionResult::Pass;
        }

        let mut context =
            crate::behavior::UseItemContext::new(player, hand, world, player.inventory.clone());

        // Get behavior registries
        let item_behaviors = &*ITEM_BEHAVIORS;
        let item_behavior = item_behaviors.get_behavior(item_ref);

        let result = item_behavior.use_item(&mut context);

        // Restore count for creative mode (infinite materials)
        if player.has_infinite_materials() {
            context.inv.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }

        if result.should_apply_item_use_side_effects() {
            player.apply_item_use_cooldown(&stack_before_use);
        }

        return result;
    }

    InteractionResult::Pass
}

const fn sound_holder_ref(holder: &SoundEventHolder) -> Option<SoundEventRef> {
    match holder {
        SoundEventHolder::Registry(sound) => Some(*sound),
        SoundEventHolder::Direct { .. } => {
            // TODO: Support direct sound holders when entity sound playback can send them.
            None
        }
    }
}

fn piercing_ray_hit_t(
    world: &World,
    bounding_box: WorldAabb,
    from: DVec3,
    to: DVec3,
    entity_margin: f64,
) -> Option<f64> {
    if let Some(hit_t) = ray_aabb_hit_t(bounding_box, from, to) {
        return Some(hit_t);
    }
    if entity_margin <= 0.0 {
        return None;
    }

    let outside_hit_t = ray_aabb_hit_t(bounding_box.inflate(entity_margin), from, to)?;
    let outside_hit = from + (to - from) * outside_hit_t;
    let mut towards_target = DVec3::new(
        f64::midpoint(bounding_box.min_x(), bounding_box.max_x()),
        f64::midpoint(bounding_box.min_y(), bounding_box.max_y()),
        f64::midpoint(bounding_box.min_z(), bounding_box.max_z()),
    );
    let block_hit = world.clip(
        outside_hit,
        towards_target,
        ClipBlockShape::Collider,
        ClipFluid::None,
    );
    if !block_hit.is_miss() {
        towards_target = block_hit.location;
    }
    ray_aabb_hit_t(bounding_box, outside_hit, towards_target).map(|_| outside_hit_t)
}

fn ray_aabb_hit_t(aabb: WorldAabb, from: DVec3, to: DVec3) -> Option<f64> {
    if aabb.contains(from) {
        return Some(0.0);
    }

    let delta = to - from;
    let mut t_min = 0.0_f64;
    let mut t_max = 1.0_f64;
    if !update_ray_axis(
        from.x,
        delta.x,
        aabb.min_x(),
        aabb.max_x(),
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }
    if !update_ray_axis(
        from.y,
        delta.y,
        aabb.min_y(),
        aabb.max_y(),
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }
    if !update_ray_axis(
        from.z,
        delta.z,
        aabb.min_z(),
        aabb.max_z(),
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }

    Some(t_min)
}

fn update_ray_axis(
    start: f64,
    delta: f64,
    min: f64,
    max: f64,
    t_min: &mut f64,
    t_max: &mut f64,
) -> bool {
    if delta.abs() < f64::EPSILON {
        return start >= min && start <= max;
    }

    let inverse_delta = 1.0 / delta;
    let mut enter = (min - start) * inverse_delta;
    let mut exit = (max - start) * inverse_delta;
    if enter > exit {
        swap(&mut enter, &mut exit);
    }

    *t_min = (*t_min).max(enter);
    *t_max = (*t_max).min(exit);
    *t_min <= *t_max
}

impl Player {
    fn invalid_entity_attacked_message() -> TextComponent {
        TranslatedMessage {
            key: "multiplayer.disconnect.invalid_entity_attacked".into(),
            fallback: None,
            args: None,
        }
        .component()
    }

    fn eye_position(&self) -> DVec3 {
        let position = self.position();
        DVec3::new(position.x, self.get_eye_y(), position.z)
    }

    fn damage_source_for_attack_type(&self, damage_type: &'static DamageType) -> DamageSource {
        DamageSource::environment(damage_type)
            .with_causing_entity(self.id())
            .with_direct_entity(self.id())
            .with_source_position(self.position())
    }

    fn attack_damage_source(&self, attacking_item: &ItemStack) -> DamageSource {
        if let Some(damage_type) = attacking_item.get_damage_type() {
            return self.damage_source_for_attack_type(damage_type);
        }
        if let Some(source) = ITEM_BEHAVIORS
            .get_behavior(attacking_item.item())
            .get_item_damage_source(self)
        {
            return source;
        }
        self.damage_source_for_attack_type(&vanilla_damage_types::PLAYER_ATTACK)
    }

    /// Ticks vanilla attack-strength recovery and resets it on main-hand item changes.
    pub(super) fn tick_attack_strength(&self) {
        self.tick_state.lock().advance_attack_strength_ticker();

        let main_hand_item = {
            let inventory = self.inventory.lock();
            let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
            stack.copy_with_count(stack.count())
        };

        let mut last_item = self.last_item_in_main_hand.lock();
        if ItemStack::matches(&last_item, &main_hand_item) {
            return;
        }

        if !ItemStack::is_same_item(&last_item, &main_hand_item) {
            self.reset_attack_strength_ticker();
        }

        *last_item = main_hand_item;
    }

    fn reset_attack_strength_ticker(&self) {
        self.tick_state.lock().reset_attack_strength_ticker();
    }

    fn current_item_attack_strength_delay(&self) -> f32 {
        let attack_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::ATTACK_SPEED);
        Self::attack_strength_delay_from_speed(attack_speed)
    }

    fn attack_strength_delay_from_speed(attack_speed: f64) -> f32 {
        (1.0 / attack_speed * 20.0) as f32
    }

    /// Returns vanilla `Player.getAttackStrengthScale`.
    #[must_use]
    pub fn attack_strength_scale(&self, partial_tick: f32) -> f32 {
        let attack_strength_delay = self.current_item_attack_strength_delay();
        self.attack_strength_scale_for_delay(partial_tick, attack_strength_delay)
    }

    fn attack_strength_scale_for_delay(
        &self,
        partial_tick: f32,
        attack_strength_delay: f32,
    ) -> f32 {
        let ticker = self.tick_state.lock().attack_strength_ticker() as f32;
        ((ticker + partial_tick) / attack_strength_delay).clamp(0.0, 1.0)
    }

    fn base_damage_scale_factor(attack_strength_scale: f32) -> f32 {
        0.2 + attack_strength_scale * attack_strength_scale * 0.8
    }

    fn get_knockback(
        attack_knockback: f64,
        weapon: &ItemStack,
        enchantment_context: &EnchantmentDamageContext<'_>,
    ) -> f64 {
        let modified = enchantment_helper::modify_knockback(
            weapon,
            enchantment_context,
            attack_knockback as f32,
        );
        f64::from(modified) / 2.0
    }

    fn cause_extra_knockback(
        &self,
        entity: &dyn Entity,
        knockback_amount: f64,
        old_movement: DVec3,
    ) {
        if knockback_amount > 0.0 {
            let yaw_radians = self.rotation().0.to_radians();
            let yaw_sin = f64::from(yaw_radians.sin());
            let yaw_cos = f64::from(yaw_radians.cos());
            if let Some(living_target) = entity.as_living_entity() {
                living_target.knockback(knockback_amount, yaw_sin, -yaw_cos);
            } else {
                entity.push_impulse(DVec3::new(
                    -yaw_sin * knockback_amount,
                    0.1,
                    yaw_cos * knockback_amount,
                ));
            }

            let velocity = self.velocity();
            self.set_velocity(DVec3::new(velocity.x * 0.6, velocity.y, velocity.z * 0.6));
            self.set_sprinting(false);
        }

        if entity.entity_type() == &vanilla_entities::PLAYER
            && entity.hurt_marked()
            && let Some(player) = self.get_world().players.get_by_entity_id(entity.id())
        {
            let velocity = entity.velocity();
            player.send_packet(CSetEntityMotion::new(entity.id(), velocity));
            entity.clear_hurt_mark();
            entity.set_velocity(old_movement);
        }
    }

    fn entity_interaction_range(&self) -> f64 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::ENTITY_INTERACTION_RANGE)
    }

    /// Returns true if the target box is within the player's attack range for `item_stack`.
    #[must_use]
    pub fn is_within_attack_range_with_buffer(
        &self,
        item_stack: &ItemStack,
        aabb: WorldAabb,
        buffer: f64,
    ) -> bool {
        let distance = aabb.distance_to_sqr(self.eye_position()).sqrt();
        let (min_reach, max_reach, hitbox_margin) =
            if let Some(attack_range) = item_stack.get_attack_range() {
                if self.game_mode() == GameType::Creative {
                    (
                        attack_range.min_creative_reach,
                        attack_range.max_creative_reach,
                        attack_range.hitbox_margin,
                    )
                } else {
                    (
                        attack_range.min_reach,
                        attack_range.max_reach,
                        attack_range.hitbox_margin,
                    )
                }
            } else {
                (0.0, self.entity_interaction_range() as f32, 0.0)
            };
        let min_reach = f64::from(min_reach) - f64::from(hitbox_margin) - buffer;
        let max_reach = f64::from(max_reach) + f64::from(hitbox_margin) + buffer;
        distance >= min_reach && distance <= max_reach
    }

    /// Returns true if the target box is within the player's entity interaction range.
    #[must_use]
    pub fn is_within_entity_interaction_range_with_buffer(
        &self,
        aabb: WorldAabb,
        buffer: f64,
    ) -> bool {
        let max_range = self.entity_interaction_range() + buffer;
        aabb.distance_to_sqr(self.eye_position()) <= max_range * max_range
    }

    fn attack_range_for_item(&self, item_stack: &ItemStack) -> (f64, f64, f64) {
        let Some(attack_range) = item_stack.get_attack_range() else {
            return (0.0, self.entity_interaction_range(), 0.0);
        };

        let (min_reach, max_reach) = if self.game_mode() == GameType::Creative {
            (
                attack_range.min_creative_reach,
                attack_range.max_creative_reach,
            )
        } else {
            (attack_range.min_reach, attack_range.max_reach)
        };
        (
            f64::from(min_reach),
            f64::from(max_reach),
            f64::from(attack_range.hitbox_margin),
        )
    }

    fn piercing_hit_entities(&self, item_stack: &ItemStack, world: &World) -> Vec<SharedEntity> {
        let look = self.look_angle();
        if look.length_squared() <= f64::EPSILON {
            return Vec::new();
        }

        let (min_reach, max_reach, hitbox_margin) = self.attack_range_for_item(item_stack);
        let eye_position = self.eye_position();
        let from = eye_position + look * min_reach;
        let movement_extension = self.known_movement().dot(look).max(0.0);
        let mut to = eye_position + look * (max_reach + movement_extension);

        let block_hit = world.clip(eye_position, to, ClipBlockShape::Collider, ClipFluid::None);
        if !block_hit.is_miss() {
            to = block_hit.location;
            if eye_position.distance_squared(to) < eye_position.distance_squared(from) {
                return Vec::new();
            }
        }

        let search_area = WorldAabb::new(from.x, from.y, from.z, from.x, from.y, from.z)
            .inflate_xyz(hitbox_margin, hitbox_margin, hitbox_margin)
            .expand_towards(to - from)
            .inflate(1.0);
        let mut hits = world
            .get_entities_in_aabb_matching(&search_area, |entity| {
                self.can_piercing_hit_entity(entity)
            })
            .into_iter()
            .filter_map(|entity| {
                piercing_ray_hit_t(world, entity.bounding_box(), from, to, hitbox_margin)
                    .map(|hit_t| (hit_t, entity))
            })
            .collect::<Vec<_>>();
        hits.sort_by(|(left, _), (right, _)| left.total_cmp(right));
        hits.into_iter().map(|(_, entity)| entity).collect()
    }

    fn can_piercing_hit_entity(&self, target: &dyn Entity) -> bool {
        target.id() != self.id()
            && !target.is_invulnerable()
            && target.is_alive()
            && target.can_be_hit_by_projectile()
            && !self.is_passenger_of_same_vehicle(target)
    }

    fn piercing_attack(&self, item_stack: &ItemStack, piercing_weapon: &PiercingWeapon) {
        let world = self.get_world();
        LivingEntity::refresh_equipment_attribute_modifiers(self, EquipmentSlot::MainHand);
        let base_damage = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::ATTACK_DAMAGE) as f32;
        let mut hit_something = false;
        for target in self.piercing_hit_entities(item_stack, &world) {
            hit_something |= self.stab_attack(
                &target,
                base_damage,
                true,
                piercing_weapon.deals_knockback,
                piercing_weapon.dismounts,
            );
        }

        self.reset_attack_strength_ticker();
        enchantment_helper::do_post_piercing_attack_effects(&world, self);
        if hit_something {
            self.play_sound_holder(piercing_weapon.hit_sound.as_ref());
        }
        self.play_sound_holder(piercing_weapon.sound.as_ref());
        self.swing(InteractionHand::MainHand, false);
    }

    fn stab_attack(
        &self,
        target: &SharedEntity,
        base_damage: f32,
        deals_damage: bool,
        deals_knockback: bool,
        dismounts: bool,
    ) -> bool {
        let entity = target.as_ref();
        if self.cannot_attack(entity) {
            return false;
        }

        let attacking_item = {
            let inventory = self.inventory.lock();
            let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
            stack.copy_with_count(stack.count())
        };
        let damage_source = self.attack_damage_source(&attacking_item);
        let enchantment_context = EnchantmentDamageContext::new(
            entity.entity_type(),
            Some(self.entity_type()),
            Some(self.entity_type()),
            &damage_source,
        );
        let enchanted_damage =
            enchantment_helper::modify_damage(&attacking_item, &enchantment_context, base_damage);
        let attack_strength_scale = self.attack_strength_scale(0.5);
        let magic_boost = attack_strength_scale * (enchanted_damage - base_damage);
        let base_damage = base_damage * Self::base_damage_scale_factor(attack_strength_scale);
        let damage = base_damage + magic_boost;
        let old_movement = entity.velocity();
        let mut affected = deals_knockback;
        let damage_dealt = deals_damage
            && entity
                .level()
                .is_some_and(|world| entity.hurt(&world, &damage_source, damage));
        affected |= damage_dealt;
        if deals_knockback {
            self.cause_extra_knockback(
                entity,
                0.4 + Self::get_knockback(0.0, &attacking_item, &enchantment_context),
                old_movement,
            );
        }
        if dismounts && entity.is_passenger() {
            affected = true;
            entity.stop_riding();
        }

        if !affected {
            return false;
        }

        self.item_attack_interaction(entity, &damage_source, damage_dealt);
        self.set_last_hurt_mob(Some(target));
        self.cause_food_exhaustion(0.1);
        true
    }

    fn play_sound_holder(&self, holder: Option<&SoundEventHolder>) {
        let Some(sound) = holder.and_then(sound_holder_ref) else {
            return;
        };
        self.play_sound(sound, 1.0, 1.0);
    }

    fn cannot_attack(&self, entity: &dyn Entity) -> bool {
        !entity.attackable() || entity.skip_attack_interaction(self)
    }

    /// Attacks an entity with the player's main-hand base damage.
    ///
    /// Returns `true` if the target accepted damage.
    #[must_use]
    pub fn attack(&self, target: &SharedEntity) -> bool {
        let entity = target.as_ref();
        if self.cannot_attack(entity) {
            return false;
        }

        LivingEntity::refresh_equipment_attribute_modifiers(self, EquipmentSlot::MainHand);
        let attacking_item = {
            let inventory = self.inventory.lock();
            let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
            stack.copy_with_count(stack.count())
        };
        let (attack_damage, attack_speed, attack_knockback) = {
            let attributes = self.attributes().lock();
            (
                attributes.required_value(vanilla_attributes::ATTACK_DAMAGE) as f32,
                attributes.required_value(vanilla_attributes::ATTACK_SPEED),
                attributes.required_value(vanilla_attributes::ATTACK_KNOCKBACK),
            )
        };
        let attack_strength_delay = Self::attack_strength_delay_from_speed(attack_speed);
        let attack_strength_scale =
            self.attack_strength_scale_for_delay(0.5, attack_strength_delay);
        let damage_source = self.attack_damage_source(&attacking_item);
        let enchantment_context = EnchantmentDamageContext::new(
            entity.entity_type(),
            Some(self.entity_type()),
            Some(self.entity_type()),
            &damage_source,
        );
        let enchanted_damage =
            enchantment_helper::modify_damage(&attacking_item, &enchantment_context, attack_damage);
        let magic_boost = attack_strength_scale * (enchanted_damage - attack_damage);
        let mut base_damage = attack_damage * Self::base_damage_scale_factor(attack_strength_scale);
        base_damage += ITEM_BEHAVIORS
            .get_behavior(attacking_item.item())
            .get_attack_damage_bonus(self, entity, base_damage, &damage_source);
        let total_damage = base_damage + magic_boost;
        let full_strength_attack = attack_strength_scale > 0.9;
        let knockback_attack = self.is_sprinting() && full_strength_attack;
        self.reset_attack_strength_ticker();

        if total_damage <= 0.0 {
            return false;
        }

        // TODO: Apply crits, sweep attacks, damage stats, and sounds.
        let old_movement = entity.velocity();
        let Some(target_world) = entity.level() else {
            return false;
        };
        let was_hurt = entity.hurt(&target_world, &damage_source, total_damage);
        if was_hurt {
            self.set_last_hurt_mob(Some(target));
            let sprint_knockback = if knockback_attack { 0.5 } else { 0.0 };
            self.cause_extra_knockback(
                entity,
                Self::get_knockback(attack_knockback, &attacking_item, &enchantment_context)
                    + sprint_knockback,
                old_movement,
            );
            self.item_attack_interaction(entity, &damage_source, true);
            self.cause_food_exhaustion(0.1);
        }

        let world = self.get_world();
        enchantment_helper::do_post_piercing_attack_effects(&world, self);
        was_hurt
    }

    fn item_attack_interaction(
        &self,
        entity: &dyn Entity,
        damage_source: &DamageSource,
        apply_to_target: bool,
    ) {
        let post_attack_context =
            EnchantmentPostAttackContext::new(entity, Some(self), Some(self), damage_source);
        let (source_item, item_hurt_enemy) = {
            let mut inventory = self.inventory.lock();
            inventory.mutate_item_in_hand(InteractionHand::MainHand, |stack| {
                if stack.is_empty() {
                    return (ItemStack::empty(), false);
                }
                let behavior = ITEM_BEHAVIORS.get_behavior(stack.item());
                if let Some(living_target) = entity.as_living_entity() {
                    behavior.hurt_enemy(stack, living_target, self);
                }
                let source_item = stack.copy_with_count(stack.count());
                (source_item, stack.get_weapon().is_some())
            })
        };

        if apply_to_target {
            let world = self.get_world();
            enchantment_helper::do_post_attack_effects_with_item_source(
                &world,
                entity,
                &source_item,
                &post_attack_context,
            );
        }

        if !item_hurt_enemy {
            return;
        }

        let Some(living_target) = entity.as_living_entity() else {
            return;
        };
        let has_infinite_materials = self.has_infinite_materials();
        let mut inventory = self.inventory.lock();
        inventory.mutate_item_in_hand(InteractionHand::MainHand, |stack| {
            if stack.is_empty() {
                return;
            }
            let behavior = ITEM_BEHAVIORS.get_behavior(stack.item());
            behavior.post_hurt_enemy(stack, living_target, self);
            if let Some(damage) = behavior.item_damage_per_attack(stack) {
                stack.hurt_and_break(damage, has_infinite_materials);
            }
        });
    }

    /// Interacts with an entity using the held item.
    pub fn interact_on(
        &self,
        entity: &dyn Entity,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        if self.is_spectator() {
            // TODO: Open entity menu providers in spectator once that foundation exists.
            return InteractionResult::Pass;
        }

        let inventory_access = InventoryAccess::new(self.inventory.clone(), hand);
        let original_count = inventory_access.with_item(|item| item.count);
        let result = entity.interact(self, hand, location);

        if self.has_infinite_materials() {
            inventory_access.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }

        if result.consumes_action() {
            return result;
        }

        if inventory_access.with_item(|item| item.is_empty()) {
            return InteractionResult::Pass;
        }
        let Some(living_entity) = entity.as_living_entity() else {
            return InteractionResult::Pass;
        };
        let result = living_entity.interact_living_entity_with_equippable(self, hand);
        if self.has_infinite_materials() {
            inventory_access.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }
        if result.consumes_action() {
            return result;
        }

        let item_ref = inventory_access.with_item(|item| item.item());
        let item_behavior = ITEM_BEHAVIORS.get_behavior(item_ref);
        let result = inventory_access.with_item(|item| {
            item_behavior.interact_living_entity(item, self, living_entity, hand)
        });
        if self.has_infinite_materials() {
            inventory_access.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }
        result
    }

    /// Handles a client request to attack an entity.
    pub fn handle_attack(&self, packet: SAttack) {
        if !self.has_client_loaded() || self.is_spectator() {
            return;
        }

        let world = self.get_world();
        let Some(target) = world.get_accessible_entity_by_id(packet.entity_id) else {
            return;
        };

        let target_pos = target.block_position();
        if !world.world_border_snapshot().is_within_bounds_with_margin(
            f64::from(target_pos.x()),
            f64::from(target_pos.z()),
            0.0,
        ) {
            return;
        }

        let main_hand_item = {
            let inventory = self.inventory.lock();
            let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
            stack.copy_with_count(stack.count())
        };

        if !self.is_within_attack_range_with_buffer(
            &main_hand_item,
            target.bounding_box(),
            ATTACK_RANGE_BUFFER,
        ) {
            return;
        }

        if main_hand_item.get_piercing_weapon().is_some() {
            return;
        }

        if Self::is_invalid_attack_target(self.id(), target.id(), target.entity_type()) {
            self.disconnect(Self::invalid_entity_attacked_message());
            log::warn!(
                "Player {} tried to attack an invalid entity",
                self.gameprofile.name
            );
            return;
        }

        if self.cannot_attack_with_item(&main_hand_item, 5) {
            return;
        }

        let _ = self.attack(&target);
    }

    fn cannot_attack_with_item(&self, item_stack: &ItemStack, tolerance: i32) -> bool {
        let required_strength = item_stack.minimum_attack_charge();
        if required_strength <= 0.0 {
            return false;
        }

        let optimistic_strength = {
            let ticker = self.tick_state.lock().attack_strength_ticker() + tolerance;
            ticker as f32 / self.current_item_attack_strength_delay()
        };
        optimistic_strength < required_strength
    }

    fn is_invalid_attack_target(
        player_id: i32,
        target_id: i32,
        target_type: EntityTypeRef,
    ) -> bool {
        target_id == player_id
            || target_type == &vanilla_entities::ITEM
            || target_type == &vanilla_entities::EXPERIENCE_ORB
    }

    /// Handles a client request to interact with an entity.
    pub fn handle_interact(&self, packet: SInteract) {
        if !self.has_client_loaded() {
            return;
        }

        let world = self.get_world();
        let target = world.get_accessible_entity_by_id(packet.entity_id);
        self.set_crouching(packet.using_secondary_action);
        let Some(target) = target else {
            return;
        };

        let target_pos = target.block_position();
        if !world.world_border_snapshot().is_within_bounds_with_margin(
            f64::from(target_pos.x()),
            f64::from(target_pos.z()),
            0.0,
        ) {
            return;
        }

        if !self.is_within_entity_interaction_range_with_buffer(
            target.bounding_box(),
            ENTITY_INTERACTION_RANGE_BUFFER,
        ) {
            return;
        }

        let result = self.interact_on(target.as_ref(), packet.hand, packet.location);
        if result.should_swing_server() {
            self.swing(packet.hand, true);
        }
        self.broadcast_inventory_changes();
    }

    /// Sets the player's game mode and notifies the client.
    ///
    /// Returns `true` if the game mode was changed, `false` if the player was already in the requested game mode.
    pub fn set_game_mode(&self, gamemode: GameType) -> bool {
        let was_spectator = self.game_mode() == GameType::Spectator;
        if !self.change_game_mode_state(gamemode) {
            return false;
        }

        // Update abilities based on new game mode (mirrors vanilla GameType.updatePlayerAbilities)
        let flying_after_update = {
            let mut abilities = self.abilities.lock();
            abilities.update_for_game_mode(gamemode);
            abilities.flying
        };
        if flying_after_update
            && gamemode != GameType::Spectator
            && self.is_in_range_of_ground_for_flight_disable()
        {
            self.set_flying(false);
        }
        self.send_abilities();
        self.update_game_mode_invisibility();

        let update_packet =
            CPlayerInfoUpdate::update_game_mode(self.gameprofile.id, gamemode as i32);
        self.server().broadcast_to_online(update_packet);

        // TODO: Refresh sleeping-player aggregation once world sleep tracking is implemented.

        if gamemode == GameType::Creative {
            self.reset_current_impulse_context();
        }

        self.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: gamemode.into(),
        });

        if gamemode == GameType::Spectator {
            self.stop_riding();
            // TODO: Remove shoulder entities once player shoulder storage is implemented.
            // TODO: Stop item use once living item-use state is implemented.
            // TODO: Stop location-based enchantment effects once those effects are implemented.
        } else if was_spectator {
            self.send_packet(CSetCamera {
                camera_id: self.id(),
            });
            // TODO: Restart location-based enchantment effects once those effects are implemented.
        }

        self.send_abilities();
        self.update_game_mode_invisibility();
        self.living_base.mark_effects_dirty();

        true
    }

    fn update_game_mode_invisibility(&self) {
        self.living_base.mark_effects_dirty();
        self.update_dirty_mob_effect_entity_data();
        self.sync_entity_data();
    }

    fn is_in_range_of_ground_for_flight_disable(&self) -> bool {
        let world = self.get_world();
        let collision_world = WorldCollisionProvider::for_entity(&world, self);
        let bounding_box = self.bounding_box();
        let collision_context =
            BlockCollisionContext::entity(self.position().y, self.is_descending())
                .with_fall_distance(self.fall_distance())
                .with_can_walk_on_powder_snow(self.can_walk_on_powder_snow());

        if collision_world.has_collision_with_context(&bounding_box, collision_context) {
            return false;
        }

        let below = WorldAabb::new(
            bounding_box.min_x(),
            bounding_box.min_y() - FLIGHT_DISABLE_RANGE,
            bounding_box.min_z(),
            bounding_box.max_x(),
            bounding_box.min_y(),
            bounding_box.max_z(),
        );
        let colliders = collision_world.get_collisions_with_context(&below, collision_context);
        if colliders.is_empty() {
            return false;
        }

        let available_space_below =
            -shapes::collide(Axis::Y, &bounding_box, &colliders, -FLIGHT_DISABLE_RANGE);
        available_space_below < FLIGHT_DISABLE_RANGE
    }

    /// Sends the current world difficulty to the client.
    pub fn send_difficulty(&self) {
        let world = self.get_world();
        let level_data = world.level_data.read();
        let difficulty = level_data.data().difficulty;
        let locked = level_data.data().difficulty_locked;
        drop(level_data);
        self.send_packet(CChangeDifficulty { difficulty, locked });
    }

    /// Handles a client request to change the world difficulty.
    pub fn handle_change_difficulty(&self, difficulty: Difficulty) {
        let world = self.get_world();
        if !player_can_change_difficulty(self, &world) {
            log::warn!(
                "Player {} tried to change difficulty to {difficulty:?} without permission",
                self.gameprofile.name
            );
            return;
        }
        {
            let level_data = world.level_data.read();
            if level_data.data().difficulty_locked {
                let current = level_data.data().difficulty;
                drop(level_data);
                self.send_packet(CChangeDifficulty {
                    difficulty: current,
                    locked: true,
                });
                return;
            }
        }

        let domain = self.get_world().domain().to_owned();
        for world in self.server().worlds.worlds_in_domain(&domain) {
            world.set_difficulty(difficulty);
        }
    }

    /// Updates interaction range attribute modifiers based on game mode.
    ///
    /// Vanilla: `ServerPlayer.updatePlayerAttributes()` — applies creative-mode
    /// range modifiers every tick.
    pub(super) fn update_player_attributes(&self) {
        LivingEntity::refresh_all_equipment_attribute_modifiers(self);

        let is_creative = self.game_mode() == GameType::Creative;
        let mut attrs = self.attributes().lock();

        if is_creative {
            attrs.set_modifier(
                vanilla_attributes::BLOCK_INTERACTION_RANGE,
                AttributeModifier {
                    id: Identifier::vanilla_static("creative_mode_block_range"),
                    amount: CREATIVE_BLOCK_RANGE_MODIFIER_AMOUNT,
                    operation: AttributeModifierOperation::AddValue,
                },
                false,
            );
            attrs.set_modifier(
                vanilla_attributes::ENTITY_INTERACTION_RANGE,
                AttributeModifier {
                    id: Identifier::vanilla_static("creative_mode_entity_range"),
                    amount: CREATIVE_ENTITY_RANGE_MODIFIER_AMOUNT,
                    operation: AttributeModifierOperation::AddValue,
                },
                false,
            );
        } else {
            attrs.remove_modifier(
                vanilla_attributes::BLOCK_INTERACTION_RANGE,
                &Identifier::vanilla_static("creative_mode_block_range"),
            );
            attrs.remove_modifier(
                vanilla_attributes::ENTITY_INTERACTION_RANGE,
                &Identifier::vanilla_static("creative_mode_entity_range"),
            );
        }
    }

    /// Returns true if player has infinite materials (Creative mode).
    #[must_use]
    pub fn has_infinite_materials(&self) -> bool {
        self.game_mode() == GameType::Creative
    }

    /// Acknowledges block changes up to the given sequence number.
    ///
    /// The ack is batched and sent once per tick (in `tick_ack_block_changes`),
    /// matching vanilla behavior.
    pub fn ack_block_changes_up_to(&self, sequence: i32) {
        self.tick_state.lock().ack_block_changes_up_to(sequence);
    }

    /// Sends pending block change ack if any. Called once per tick.
    pub(super) fn tick_ack_block_changes(&self) {
        let sequence = self.tick_state.lock().take_ack_block_changes_up_to();
        if sequence > -1 {
            self.send_packet(CBlockChangedAck { sequence });
        }
    }

    /// Returns true if player is within block interaction range.
    ///
    /// Uses eye position and AABB distance (nearest point on block surface),
    /// matching vanilla's `Player.isWithinBlockInteractionRange(pos, 1.0)`.
    #[must_use]
    pub fn is_within_block_interaction_range(&self, pos: BlockPos) -> bool {
        self.is_within_block_interaction_range_with_buffer(pos, 1.0)
    }

    /// Returns true if player is within block interaction range plus a vanilla buffer.
    #[must_use]
    pub fn is_within_block_interaction_range_with_buffer(
        &self,
        pos: BlockPos,
        buffer: f64,
    ) -> bool {
        let player_pos = self.position();
        let eye_y = player_pos.y + self.get_eye_height();

        let min_x = f64::from(pos.x());
        let min_y = f64::from(pos.y());
        let min_z = f64::from(pos.z());
        let max_x = min_x + 1.0;
        let max_y = min_y + 1.0;
        let max_z = min_z + 1.0;

        let dx = f64::max(f64::max(min_x - player_pos.x, player_pos.x - max_x), 0.0);
        let dy = f64::max(f64::max(min_y - eye_y, eye_y - max_y), 0.0);
        let dz = f64::max(f64::max(min_z - player_pos.z, player_pos.z - max_z), 0.0);
        let dist_sq = dx * dx + dy * dy + dz * dz;

        let base_range = self
            .attributes()
            .lock()
            .get_value(vanilla_attributes::BLOCK_INTERACTION_RANGE)
            .unwrap_or(4.5);
        let max_range = base_range + buffer;
        dist_sq < max_range * max_range
    }

    /// Returns true if the player's eye position is within entity interaction range.
    #[must_use]
    pub fn is_within_entity_interaction_range(&self, aabb: WorldAabb, buffer: f64) -> bool {
        let player_pos = self.position();
        let eye_y = player_pos.y + self.get_eye_height();

        let dx = f64::max(
            f64::max(aabb.min_x() - player_pos.x, player_pos.x - aabb.max_x()),
            0.0,
        );
        let dy = f64::max(f64::max(aabb.min_y() - eye_y, eye_y - aabb.max_y()), 0.0);
        let dz = f64::max(
            f64::max(aabb.min_z() - player_pos.z, player_pos.z - aabb.max_z()),
            0.0,
        );
        let dist_sq = dx * dx + dy * dy + dz * dz;

        let base_range = self
            .attributes()
            .lock()
            .get_value(vanilla_attributes::ENTITY_INTERACTION_RANGE)
            .unwrap_or(3.0);
        let max_range = base_range + buffer;
        dist_sq < max_range * max_range
    }

    /// Handles spectator camera selection.
    pub fn handle_spectator_action(&self, packet: SSpectatorAction) {
        if !self.has_client_loaded() || self.game_mode() != GameType::Spectator {
            return;
        }

        let Some(entity_id) = packet.spectate_entity_id else {
            return;
        };

        let world = self.get_world();
        let Some(target) = world.get_accessible_entity_by_id(entity_id) else {
            return;
        };

        // TODO: Store the camera entity and apply world-border/cross-world
        // setCamera semantics once those foundations exist.
        if self.is_within_entity_interaction_range(target.bounding_box(), 3.0)
            && target.is_pickable()
        {
            self.send_packet(CSetCamera {
                camera_id: target.id(),
            });
        }
    }

    /// Returns true if player is sneaking (secondary use active).
    #[must_use]
    pub fn is_secondary_use_active(&self) -> bool {
        self.is_crouching()
    }

    /// Sends block update packets for a position and its neighbor.
    /// Optionally also sends an update for an additional placement position
    /// (useful for items like buckets that place blocks at different positions).
    fn send_block_updates(&self, pos: BlockPos, direction: Direction) {
        let world = self.get_world();
        let state = world.get_block_state(pos);
        self.send_packet(CBlockUpdate {
            pos,
            block_state: state,
        });

        let neighbor_pos = direction.relative(pos);
        let neighbor_state = world.get_block_state(neighbor_pos);
        self.send_packet(CBlockUpdate {
            pos: neighbor_pos,
            block_state: neighbor_state,
        });
    }

    /// Triggers arm swing animation and broadcasts it to tracking players.
    pub fn swing(&self, hand: InteractionHand, update_self: bool) {
        LivingEntity::swing(self, hand, update_self);
    }

    /// Handles the use of an item on a block.
    ///
    /// Implements the logic from Java's `ServerGamePacketListenerImpl.handleUseItemOn()`.
    pub fn handle_use_item_on(&self, packet: SUseItemOn) {
        if !self.has_client_loaded() {
            return;
        }

        self.ack_block_changes_up_to(packet.sequence);

        let pos = packet.block_hit.block_pos;
        let direction = packet.block_hit.direction;

        if !self.is_within_block_interaction_range(pos) {
            self.send_block_updates(pos, direction);
            return;
        }

        let center_x = f64::from(pos.x()) + 0.5;
        let center_y = f64::from(pos.y()) + 0.5;
        let center_z = f64::from(pos.z()) + 0.5;
        let location = &packet.block_hit.location;
        let limit = 1.000_000_1;

        if (location.x - center_x).abs() >= limit
            || (location.y - center_y).abs() >= limit
            || (location.z - center_z).abs() >= limit
        {
            log::warn!(
                "Rejecting UseItemOnPacket from {}: location {:?} too far from block {:?}",
                self.gameprofile.name,
                location,
                pos
            );
            self.send_block_updates(pos, direction);
            return;
        }

        let world = self.get_world();

        if pos.y() >= world.max_build_height() {
            // TODO: Send "build.tooHigh" message to player
            self.send_block_updates(pos, direction);
            return;
        }

        if self.is_awaiting_teleport() {
            self.send_block_updates(pos, direction);
            return;
        }

        if !world.may_interact(self, pos) {
            self.send_block_updates(pos, direction);
            return;
        }

        let result = use_item_on(self, &world, packet.hand, &packet.block_hit);

        if result.should_swing_server() {
            self.swing(packet.hand, true);
        }

        self.send_block_updates(pos, direction);
        self.broadcast_inventory_changes();
    }

    /// Handles a player action packet (block breaking, item dropping, etc.).
    pub fn handle_player_action(&self, packet: SPlayerAction) {
        if !self.has_client_loaded() {
            return;
        }

        let world = self.get_world();
        match packet.action {
            PlayerAction::StartDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    packet.pos,
                    BlockBreakAction::Start,
                    packet.direction,
                );
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::StopDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    packet.pos,
                    BlockBreakAction::Stop,
                    packet.direction,
                );
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::AbortDestroyBlock => {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    packet.pos,
                    BlockBreakAction::Abort,
                    packet.direction,
                );
                self.ack_block_changes_up_to(packet.sequence);
            }
            PlayerAction::DropAllItems => {
                self.drop_from_selected(true);
            }
            PlayerAction::DropItem => {
                self.drop_from_selected(false);
            }
            PlayerAction::ReleaseUseItem => {
                // TODO: Implement release use item (releasing bow, etc.)
                log::debug!("Player {} released use item", self.gameprofile.name);
            }
            PlayerAction::SwapItemWithOffhand => {
                if self.game_mode() == GameType::Spectator {
                    return;
                }

                let changed = self.inventory.lock().swap_hands();
                if changed {
                    self.broadcast_entity_event(EntityStatus::SwapHands);
                    self.broadcast_inventory_changes();
                }
                // TODO: Stop active item use once the using-item foundation exists.
            }
            PlayerAction::Stab => {
                if self.game_mode() == GameType::Spectator {
                    return;
                }

                let main_hand_item = {
                    let inventory = self.inventory.lock();
                    let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
                    stack.copy_with_count(stack.count())
                };
                if self.cannot_attack_with_item(&main_hand_item, 5) {
                    return;
                }
                if let Some(piercing_weapon) = main_hand_item.get_piercing_weapon() {
                    self.piercing_attack(&main_hand_item, piercing_weapon);
                }
            }
        }
    }

    /// Handles the use of an item.
    pub fn handle_use_item(&self, packet: SUseItem) {
        if !self.has_client_loaded() {
            return;
        }

        log::info!(
            "Player {} used {:?} (sequence: {}, yaw: {}, pitch: {})",
            self.gameprofile.name,
            packet.hand,
            packet.sequence,
            packet.y_rot,
            packet.x_rot
        );

        self.ack_block_changes_up_to(packet.sequence);

        let item_stack_is_empty = {
            let inventory = self.inventory.lock();
            inventory.get_item_in_hand(packet.hand).is_empty()
        };
        if item_stack_is_empty {
            return;
        }

        let target_yaw = wrap_degrees(packet.y_rot);
        let target_pitch = wrap_degrees(packet.x_rot);
        if self.rotation() != (target_yaw, target_pitch) {
            self.set_rotation((target_yaw, target_pitch));
        }

        let world = self.get_world();
        let result = use_item(self, &world, packet.hand);

        if result.should_swing_server() {
            self.swing(packet.hand, true);
        }

        self.broadcast_inventory_changes();
    }

    /// Handles the pick block action (middle click on a block).
    ///
    /// # Panics
    ///
    /// Panics if the behavior registry has not been initialized.
    pub fn handle_pick_item_from_block(&self, packet: SPickItemFromBlock) {
        if !self.is_within_block_interaction_range(packet.pos) {
            return;
        }

        let state = self.get_world().get_block_state(packet.pos);
        if state.is_air() {
            return;
        }

        let block = state.get_block();
        let block_behaviors = &*BLOCK_BEHAVIORS;
        let behavior = block_behaviors.get_behavior(block);

        let include_data = self.has_infinite_materials() && packet.include_data;

        let Some(item_stack) = behavior.get_clone_item_stack(block, state, include_data) else {
            return;
        };

        if item_stack.is_empty() {
            return;
        }

        // TODO: If include_data, add block entity NBT data to the item stack
        // This requires block entity support which isn't implemented yet

        let mut inventory = self.inventory.lock();

        let slot_with_item = inventory.find_slot_matching_item(&item_stack);

        if slot_with_item != -1 {
            if PlayerInventory::is_hotbar_slot(slot_with_item as usize) {
                inventory.set_selected_slot(slot_with_item as u8);
            } else {
                inventory.pick_slot(slot_with_item);
            }
        } else if self.has_infinite_materials() {
            inventory.add_and_pick_item(item_stack);
        } else {
            return;
        }

        self.send_packet(CSetHeldSlot {
            slot: i32::from(inventory.get_selected_slot()),
        });

        drop(inventory);
        self.inventory_menu
            .lock()
            .behavior_mut()
            .broadcast_changes(&self.connection);
    }

    /// Handles a sign update packet from the client.
    pub fn handle_sign_update(&self, packet: SSignUpdate) {
        if !self.is_within_block_interaction_range(packet.pos) {
            return;
        }

        let world = self.get_world();

        let Some(block_entity) = world.get_block_entity(packet.pos) else {
            return;
        };

        let mut guard = block_entity.lock();
        let Some(sign) = guard.downcast_mut::<SignBlockEntity>() else {
            return;
        };

        if sign.is_waxed {
            return;
        }

        if sign.get_player_who_may_edit() != Some(self.gameprofile.id) {
            log::warn!(
                "Player {} tried to edit sign they're not allowed to edit",
                self.gameprofile.name
            );
            return;
        }

        let text = sign.get_text_mut(packet.is_front_text);
        for (i, line) in packet.lines.iter().enumerate() {
            if i < 4 {
                let stripped = strip_formatting_codes(line);
                text.set_message(i, TextComponent::plain(stripped));
            }
        }

        sign.set_player_who_may_edit(None);
        sign.set_changed();

        let update_tag = sign.get_update_tag();
        let block_entity_type = sign.get_type();
        let pos = packet.pos;

        drop(guard);

        if let Some(nbt) = update_tag {
            world.broadcast_block_entity_update(pos, block_entity_type, nbt);
        }
    }

    /// Opens the sign editor for the player.
    ///
    /// # Arguments
    /// * `pos` - Position of the sign block
    /// * `is_front_text` - Whether to edit front (true) or back (false) text
    pub fn open_sign_editor(&self, pos: BlockPos, is_front_text: bool) {
        let world = self.get_world();

        if let Some(block_entity) = world.get_block_entity(pos) {
            let mut guard = block_entity.lock();
            if let Some(sign) = guard.downcast_mut::<SignBlockEntity>() {
                sign.set_player_who_may_edit(Some(self.gameprofile.id));
            }
        }

        let state = world.get_block_state(pos);
        self.send_packet(CBlockUpdate {
            pos,
            block_state: state,
        });

        self.send_packet(COpenSignEditor { pos, is_front_text });
    }
}

/// Strips Minecraft formatting codes (§ followed by a character) from a string.
///
/// This is equivalent to vanilla's `ChatFormatting.stripFormatting()`.
fn strip_formatting_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use steel_registry::vanilla_entities;

    use super::Player;

    #[test]
    fn invalid_attack_targets_include_xp_orbs() {
        assert!(Player::is_invalid_attack_target(
            1,
            1,
            &vanilla_entities::PLAYER
        ));
        assert!(Player::is_invalid_attack_target(
            1,
            2,
            &vanilla_entities::ITEM
        ));
        assert!(Player::is_invalid_attack_target(
            1,
            2,
            &vanilla_entities::EXPERIENCE_ORB
        ));
        assert!(!Player::is_invalid_attack_target(
            1,
            2,
            &vanilla_entities::PIG
        ));
    }
}
