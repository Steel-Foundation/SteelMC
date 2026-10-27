//! Item behavior trait and registry.

use crate::player::player_inventory::Prediction;
use std::sync::Arc;

use std::borrow::Cow;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::Consumable;
use steel_registry::data_components::vanilla_components::{
    BLOCKS_ATTACKS, CONSUMABLE, FOOD, KINETIC_WEAPON, USE_REMAINDER,
};

use steel_registry::data_components::vanilla_components::{INTANGIBLE_PROJECTILE, ITEM_NAME};
use steel_registry::equipment::EquipmentSlot;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::vanilla_items;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt, sound_events, vanilla_game_events};
use steel_utils::types::{InteractionHand, SignTextSlot};
use text_components::TextComponent;

use crate::behavior::items::{DefaultItemBehavior, SpawnEggItem, block_transformer};
use crate::behavior::{
    ITEM_BEHAVIORS, InteractionResult, InventoryTickContext, UseItemContext, UseOnContext,
};
use crate::block_entity::entities::{SignBlockEntity, SignText};
use crate::enchantment_helper;
use crate::entity::consume_effect::apply_consume_effect;
use crate::entity::damage::DamageSource;
use crate::entity::projectile::{Projectile, spawn_projectile};
use crate::entity::{Entity, LivingEntity, LivingEntityRef, SharedEntity};
use crate::inventory::container::Container as _;
use crate::player::{Player, player_inventory::EquipmentSwapResult};
use crate::world::World;

pub use steel_registry::data_components::vanilla_components::ItemUseAnimation;

/// Vanilla `InteractionHand.asEquipmentSlot`, used to attribute weapon
/// durability loss to the hand that fired the projectile.
const fn equipment_slot(hand: InteractionHand) -> EquipmentSlot {
    match hand {
        InteractionHand::MainHand => EquipmentSlot::MainHand,
        InteractionHand::OffHand => EquipmentSlot::OffHand,
    }
}

/// Item capability for vanilla `ArrowItem` subclasses: creates the concrete
/// `AbstractArrow` a projectile weapon fires with this item as ammo.
pub trait ArrowItemBehavior: Send + Sync {
    /// Vanilla `ArrowItem.createArrow`.
    ///
    /// `is_crit` folds vanilla `ProjectileWeaponItem.createProjectile`'s
    /// `setCritArrow` call into construction because `AbstractArrow` is not
    /// object-safe, so the caller cannot set it afterwards.
    fn create_arrow(
        &self,
        world: &Arc<World>,
        pickup_item_stack: &mut ItemStack,
        shooter: LivingEntityRef<'_>,
        weapon: Option<&ItemStack>,
        is_crit: bool,
    ) -> Option<SharedEntity>;
}

/// Item capability for vanilla `ProjectileWeaponItem`: a weapon that draws ammo
/// from the holder's inventory and fires projectiles with it.
pub trait ProjectileWeaponItem: Send + Sync {
    /// Vanilla `getSupportedHeldProjectiles`.
    fn is_supported_held_projectile(&self, stack: &ItemStack) -> bool {
        self.is_supported_projectile(stack)
    }

    /// Vanilla `getAllSupportedProjectiles`. The base `ProjectileWeaponItem`
    /// default is `ARROW_ONLY`.
    fn is_supported_projectile(&self, stack: &ItemStack) -> bool {
        stack.item.has_tag(&ItemTag::ARROWS)
    }

    /// Vanilla `getDefaultProjectileRange`.
    fn get_default_projectile_range(&self) -> i32;

    /// Vanilla `getDurabilityUse`.
    fn get_durability_use(&self, _projectile: &ItemStack) -> i32 {
        1
    }

    /// Vanilla `shootProjectile`.
    fn shoot_projectile(
        &self,
        shooter: &Player,
        projectile: &dyn Projectile,
        index: usize,
        power: f32,
        uncertainty: f32,
        angle: f32,
    );

    /// Vanilla `createProjectile`.
    fn create_projectile(
        &self,
        world: &Arc<World>,
        shooter: LivingEntityRef<'_>,
        weapon: &ItemStack,
        projectile: &mut ItemStack,
        is_crit: bool,
    ) -> Option<SharedEntity> {
        let arrow_item = ITEM_BEHAVIORS
            .get_behavior(projectile.item())
            .as_arrow_item()
            .or_else(|| {
                ITEM_BEHAVIORS
                    .get_behavior(&vanilla_items::ARROW)
                    .as_arrow_item()
            });
        let Some(arrow_item) = arrow_item else {
            log::debug!(
                "no arrow item behavior for {} or minecraft:arrow",
                projectile.item().key
            );
            return None;
        };
        arrow_item.create_arrow(world, projectile, shooter, Some(weapon), is_crit)
    }

    /// Vanilla `ProjectileWeaponItem.shoot`: spawns each drawn projectile with
    /// its spread angle, then damages the weapon once per shot.
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla ProjectileWeaponItem.shoot"
    )]
    fn shoot(
        &self,
        world: &Arc<World>,
        shooter: &Player,
        shooter_entity: &SharedEntity,
        hand: InteractionHand,
        weapon: &mut ItemStack,
        projectiles: &[ItemStack],
        power: f32,
        uncertainty: f32,
        is_crit: bool,
    ) {
        let Some(shooter_ref) = LivingEntityRef::new(shooter_entity) else {
            return;
        };
        let max_angle = enchantment_helper::process_projectile_spread(weapon, shooter, 0.0);
        let angle_step = if projectiles.len() == 1 {
            0.0
        } else {
            2.0 * max_angle / (projectiles.len() - 1) as f32
        };
        let angle_offset = ((projectiles.len() - 1) % 2) as f32 * angle_step / 2.0;
        let mut direction = 1.0_f32;
        let slot = equipment_slot(hand);

        for (index, projectile) in projectiles.iter().enumerate() {
            if projectile.is_empty() {
                continue;
            }
            let angle = angle_offset + direction * index.div_ceil(2) as f32 * angle_step;
            direction = -direction;

            let mut projectile = projectile.clone();
            if let Some(entity) =
                self.create_projectile(world, shooter_ref, weapon, &mut projectile, is_crit)
            {
                let _ = spawn_projectile(world, entity, &mut projectile, |entity| {
                    self.shoot_projectile(shooter, entity, index, power, uncertainty, angle);
                });
            }

            // Vanilla damages the weapon after the spawn attempt regardless of
            // whether the projectile entity could be created or added.
            let durability = self.get_durability_use(&projectile);
            let weapon_item = weapon.item();
            if weapon.hurt_and_break(durability, shooter.has_infinite_materials()) {
                shooter.on_equipped_item_broken(weapon_item, slot);
            }
            if weapon.is_empty() {
                break;
            }
        }
    }

    /// Vanilla `draw`: splits the ammo stack into `processProjectileCount`
    /// projectiles.
    fn draw(
        &self,
        weapon: &ItemStack,
        projectile: &ItemStack,
        slot: Option<usize>,
        shooter: &Player,
    ) -> Vec<ItemStack> {
        if projectile.is_empty() {
            return Vec::new();
        }

        let count = enchantment_helper::process_projectile_count(weapon, shooter, 1) as usize;
        let mut drawn = Vec::with_capacity(count);
        let mut first = projectile.clone();
        let mut extra = projectile.clone();
        for index in 0..count {
            let drawn_stack = if index == 0 {
                self.use_ammo(weapon, &mut first, slot, shooter, false)
            } else {
                self.use_ammo(weapon, &mut extra, None, shooter, true)
            };
            if !drawn_stack.is_empty() {
                drawn.push(drawn_stack);
            }
        }

        drawn
    }

    /// Vanilla `useAmmo`.
    ///
    /// `slot` is the inventory slot the ammo came from; the split remainder is
    /// written back there because Steel hands out a stack copy where vanilla
    /// mutates the live inventory stack.
    fn use_ammo(
        &self,
        weapon: &ItemStack,
        projectile: &mut ItemStack,
        slot: Option<usize>,
        holder: &Player,
        force_infinite: bool,
    ) -> ItemStack {
        let ammo_to_use = if force_infinite || holder.has_infinite_materials() {
            0
        } else {
            enchantment_helper::process_ammo_use(weapon, projectile, 1)
        };
        if ammo_to_use > projectile.count() {
            return ItemStack::empty();
        }

        if ammo_to_use == 0 {
            let mut copy = projectile.copy_with_count(1);
            copy.set(INTANGIBLE_PROJECTILE, ());
            return copy;
        }

        let used = projectile.split(ammo_to_use);
        if let Some(slot) = slot {
            holder.inventory.lock().set_item(slot, projectile.clone());
        }
        used
    }
}

/// Item capability applied by a sign before its editor fallback.
pub trait SignApplicator: Send + Sync {
    /// Returns whether this item can change the selected sign text.
    fn can_apply_to_sign(&self, text: &SignText, _item: &ItemStack, _player: &Player) -> bool {
        text.has_message()
    }

    /// Applies the change; the sign handles consumption, statistics and updates.
    fn try_apply_to_sign(
        &self,
        world: &World,
        sign: &SignBlockEntity,
        slot: SignTextSlot,
        item: &ItemStack,
        player: &Player,
    ) -> bool;
}

/// Trait defining the behavior of an item.
///
/// This trait handles dynamic/functional aspects of items:
/// - Use on blocks (placing, interacting)
/// - Use in air
/// - etc.
pub trait ItemBehavior: Send + Sync {
    /// Returns the Rust type name of the concrete behavior implementation.
    #[cfg(feature = "test-framework")]
    #[must_use]
    #[expect(clippy::absolute_paths, reason = "easier for features")]
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns this item's display name, from the stack's `item_name`
    /// component if present.
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        stack
            .get(ITEM_NAME)
            .map_or_else(|| Cow::Owned(TextComponent::new()), Cow::Borrowed)
    }

    /// Returns the canonical single-item stack for this item, including
    /// component defaults.
    fn get_default_instance(&self, item: ItemRef) -> ItemStack {
        ItemStack::new(item)
    }

    /// Called when this item is used on a block.
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        block_transformer::use_on(context)
    }

    /// Called when this item is used (e.g. right click in air).
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        // TODO: Mirror Item.use for BLOCKS_ATTACKS and KINETIC_WEAPON so
        // specialized behaviors inherit the complete Vanilla base path.
        let (consume_ticks, can_eat) = context.inv.with_item(|item| {
            let can_eat = item
                .get(FOOD)
                .is_none_or(|food| context.player.can_eat(food.can_always_eat()));
            (item.get(CONSUMABLE).map(Consumable::consume_ticks), can_eat)
        });
        if let Some(consume_ticks) = consume_ticks {
            if !can_eat {
                return InteractionResult::Fail;
            }
            // Mirrors vanilla `Consumable.startConsuming`: an item with no
            // consume duration is consumed immediately instead of entering
            // the timed-use state — `start_using_item` rejects a duration of
            // 0 outright, so going through it here would silently never
            // finish and never run the item's effects.
            if consume_ticks > 0 {
                context.player.start_using_item(context.hand);
            } else {
                let stack = context.inv.with_item(|item| item.clone());
                let result = finish_consuming_stack(&stack, context.world, context.player.as_ref());
                context.inv.with_item(|item| *item = result);
            }
            return InteractionResult::Consume;
        }

        let Some(equippable) = context.inv.with_item(|item| item.get_equippable().cloned()) else {
            return InteractionResult::Pass;
        };

        if !equippable.swappable || !equippable.can_be_equipped_by(context.player.entity_type()) {
            return InteractionResult::Pass;
        }

        let slot = equippable.slot;
        let equipped = context.inv.with_item(|item| item.copy_with_count(1));
        let result = context.inv.with_inventory(|inventory| {
            inventory.try_swap_with_equipment_slot(
                context.hand,
                slot,
                context.player.has_infinite_materials(),
            )
        });

        match result {
            EquipmentSwapResult::Success(overflow) => {
                if !overflow.is_empty() {
                    let _ = context
                        .player
                        .drop_item(overflow, false, Prediction::Predicted);
                }
                context
                    .player
                    .on_equip_item(slot, &ItemStack::empty(), &equipped);
                InteractionResult::Success
            }
            EquipmentSwapResult::Fail => InteractionResult::Fail,
        }
    }

    /// Returns which animation plays while this item is actively being used,
    /// based on its data components.
    fn get_use_animation(&self, stack: &ItemStack) -> ItemUseAnimation {
        if let Some(consumable) = stack.get(CONSUMABLE) {
            consumable.animation()
        } else if stack.has(BLOCKS_ATTACKS) {
            ItemUseAnimation::Block
        } else if stack.has(KINETIC_WEAPON) {
            ItemUseAnimation::Spear
        } else {
            ItemUseAnimation::None
        }
    }

    /// Returns how many ticks this item can be held in active use before it
    /// finishes automatically.
    fn get_use_duration(&self, stack: &ItemStack, _user: &dyn LivingEntity) -> i32 {
        if let Some(consumable) = stack.get(CONSUMABLE) {
            consumable.consume_ticks()
        } else if stack.has(BLOCKS_ATTACKS) || stack.has(KINETIC_WEAPON) {
            72000
        } else {
            0
        }
    }

    /// Called every tick while a living entity is actively using this item.
    fn on_use_tick(
        &self,
        _world: &Arc<World>,
        user: LivingEntityRef<'_>,
        stack: &mut ItemStack,
        ticks_remaining: i32,
    ) {
        if let Some(consumable) = stack.get(CONSUMABLE)
            && should_emit_consume_particles_and_sounds(consumable, ticks_remaining)
        {
            emit_consume_particles_and_sounds(consumable, user.living());
        }
    }

    /// Called when active use is released before completion.
    ///
    /// Returns whether vanilla should apply the stack's after-use component side effects.
    fn release_using(
        &self,
        _stack: &mut ItemStack,
        _world: &Arc<World>,
        _user: LivingEntityRef<'_>,
        _time_left: i32,
    ) -> bool {
        false
    }

    /// Returns whether the item acts when the use key is released, rather than the use timer expiring
    fn use_on_release(&self, _stack: &ItemStack) -> bool {
        false
    }

    /// Called when active use reaches its full duration.
    fn finish_using(
        &self,
        stack: &mut ItemStack,
        world: &Arc<World>,
        user: LivingEntityRef<'_>,
    ) -> ItemStack {
        finish_consuming_stack(stack, world, user.living())
    }

    /// Called when this item is used to interact with a living entity
    /// (e.g. right-clicking a mob while holding it).
    fn interact_living_entity(
        &self,
        _stack: &mut ItemStack,
        _player: &Player,
        _target: &dyn LivingEntity,
        _hand: InteractionHand,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    /// Returns a custom damage source this item should inflict when its
    /// wielder attacks, overriding the caller's default attack damage type.
    fn get_item_damage_source(&self, _attacker: &SharedEntity) -> Option<DamageSource> {
        None
    }

    /// Returns item-specific attack damage added by `Item.getAttackDamageBonus`.
    fn get_attack_damage_bonus(
        &self,
        _attacker: &dyn LivingEntity,
        _victim: &dyn Entity,
        _base_damage: f32,
        _damage_source: &DamageSource,
    ) -> f32 {
        0.0
    }

    /// Called immediately when this item's wielder deals a successful melee
    /// hit, before post-attack enchantment effects and durability loss.
    fn hurt_enemy(
        &self,
        _stack: &mut ItemStack,
        _target: &dyn LivingEntity,
        _attacker: &dyn LivingEntity,
    ) {
    }

    /// Called after `hurt_enemy` and post-attack enchantment effects, right
    /// before the item takes durability damage from the hit.
    fn post_hurt_enemy(
        &self,
        _stack: &mut ItemStack,
        _target: &dyn LivingEntity,
        _attacker: &dyn LivingEntity,
    ) {
    }

    /// Called every tick for each carried item.
    fn inventory_tick(&self, _context: &mut InventoryTickContext<'_>) {}

    /// Returns how much durability this weapon consumes after a successful entity hit.
    fn item_damage_per_attack(&self, stack: &ItemStack) -> Option<i32> {
        stack
            .get_weapon()
            .map(|weapon| weapon.item_damage_per_attack)
    }

    /// Whether this item may be stored inside container items such as shulker
    /// boxes and bundles, which vanilla uses to stop them nesting.
    #[must_use]
    fn can_fit_inside_container_items(&self) -> bool {
        true
    }

    /// Returns this item behavior as a `SpawnEggItem`.
    fn as_spawn_egg(&self) -> Option<&SpawnEggItem> {
        None
    }

    /// Returns the capability used by sign block interactions.
    fn as_sign_applicator(&self) -> Option<&dyn SignApplicator> {
        None
    }

    /// Returns the projectile-weapon capability (vanilla `ProjectileWeaponItem`).
    fn as_projectile_weapon(&self) -> Option<&dyn ProjectileWeaponItem> {
        None
    }

    /// Returns the arrow-item capability (vanilla `ArrowItem`) used as
    /// projectile-weapon ammo.
    fn as_arrow_item(&self) -> Option<&dyn ArrowItemBehavior> {
        None
    }
}

fn should_emit_consume_particles_and_sounds(consumable: &Consumable, ticks_remaining: i32) -> bool {
    let consume_ticks = consumable.consume_ticks();
    let ticks_used = consume_ticks - ticks_remaining;
    let wait_ticks = (consume_ticks as f32 * 0.21875) as i32;
    ticks_used > wait_ticks && ticks_remaining % 4 == 0
}

fn emit_consume_particles_and_sounds(consumable: &Consumable, user: &dyn LivingEntity) {
    // TODO: spawn item-crumb particles when `has_consume_particles()` is set.
    let (volume, pitch) = if consumable.animation() == ItemUseAnimation::Drink {
        (0.5, 0.9 + rand::random::<f32>() * 0.1)
    } else {
        let volume = if rand::random::<bool>() { 0.5 } else { 1.0 };
        let pitch = 1.0 + 0.2 * (rand::random::<f32>() - rand::random::<f32>());
        (volume, pitch)
    };
    if let Some(sound) = consumable.sound().registry_ref() {
        user.play_sound(sound, volume, pitch);
    }
}

/// Applies vanilla `Consumable.onConsume`'s shared tail: runs
/// `on_consume_effects`, plays the consume sound, then shrinks the stack by
/// one (creative mode leaves it untouched).
pub(crate) fn finish_consuming_stack(
    stack: &ItemStack,
    world: &Arc<World>,
    user: &dyn LivingEntity,
) -> ItemStack {
    let Some(consumable) = stack.get(CONSUMABLE) else {
        return apply_use_remainder(stack, stack.copy_with_count(stack.count()), user);
    };

    emit_consume_particles_and_sounds(consumable, user);

    if let Some(food) = stack.get(FOOD) {
        if let Some(sound) = consumable.sound().registry_ref() {
            world.play_sound_at(
                sound,
                SoundSource::Neutral,
                user.position(),
                1.0,
                // Java `RandomSource.triangle(1.0, 0.4)`.
                1.0 + 0.4 * (rand::random::<f32>() - rand::random::<f32>()),
                None,
            );
        }
        if let Some(player) = user.as_player() {
            player
                .food_data
                .lock()
                .add_food(food.nutrition(), food.saturation());
            world.play_sound_at(
                &sound_events::ENTITY_PLAYER_BURP,
                SoundSource::Players,
                player.position(),
                0.5,
                // Java `Mth.randomBetween(random, 0.9, 1.0)`.
                0.9 + rand::random::<f32>() * 0.1,
                None,
            );
        }
    }

    for effect in consumable.on_consume_effects() {
        apply_consume_effect(effect, world, user);
    }

    let event = if consumable.animation() == ItemUseAnimation::Drink {
        &vanilla_game_events::DRINK
    } else {
        &vanilla_game_events::EAT
    };
    user.game_event(event);
    if let Some(player) = user.as_player() {
        player.award_stat(&vanilla_stat_types::ITEM_USED, stack.item());
    }
    // TODO: Spawn item-crumb particles when `has_consume_particles()` is set.
    // TODO: Trigger CriteriaTriggers.CONSUME_ITEM once the advancement-criteria
    // foundation exists.

    let mut used_stack = stack.copy_with_count(stack.count());
    if !user.has_infinite_materials() {
        used_stack.shrink(1);
    }

    apply_use_remainder(stack, used_stack, user)
}

/// Applies vanilla `UseRemainder.convertIntoRemainder`: if the original stack
/// had a `use_remainder` and was actually consumed, either swap the fully
/// emptied stack for the remainder, or — for a stack that still has items
/// left (e.g. one honey bottle out of several)
pub(crate) fn apply_use_remainder(
    original_stack: &ItemStack,
    mut used_stack: ItemStack,
    user: &dyn LivingEntity,
) -> ItemStack {
    let Some(remainder) = original_stack.get(USE_REMAINDER) else {
        return used_stack;
    };
    if let Some(extra) = remainder.convert_into_remainder(
        &mut used_stack,
        original_stack.count(),
        user.has_infinite_materials(),
    ) {
        user.handle_extra_items_created_on_use(extra);
    }
    used_stack
}

/// Registry for item behaviors.
///
/// Created after the main registry is frozen. Block items get `BlockItemBehavior`,
/// other items get `DefaultItemBehavior`. Custom behaviors can be registered.
pub struct ItemBehaviorRegistry {
    behaviors: Vec<Box<dyn ItemBehavior>>,
}

impl ItemBehaviorRegistry {
    /// Creates a new behavior registry with default behaviors for all items.
    ///
    /// Call `register_item_behaviors()` after this to set up proper behaviors.
    #[must_use]
    pub fn new() -> Self {
        let item_count = REGISTRY.items.len();
        let behaviors = (0..item_count)
            .map(|_| Box::new(DefaultItemBehavior) as Box<dyn ItemBehavior>)
            .collect();

        Self { behaviors }
    }

    /// Sets a custom behavior for an item.
    pub fn set_behavior(&mut self, item: ItemRef, behavior: Box<dyn ItemBehavior>) {
        let id = item.id();
        self.behaviors[id] = behavior;
    }

    /// Gets the behavior for an item.
    #[must_use]
    pub fn get_behavior(&self, item: ItemRef) -> &dyn ItemBehavior {
        let id = item.id();
        self.behaviors[id].as_ref()
    }

    /// Returns the default stack for `item`, dispatching to its behavior's
    /// [`ItemBehavior::get_default_instance`].
    #[must_use]
    pub fn default_instance(&self, item: ItemRef) -> ItemStack {
        self.get_behavior(item).get_default_instance(item)
    }

    /// Returns vanilla `ItemStack.getHoverName`, including item-specific
    /// `Item.getName(stack)` overrides when no custom name is present.
    #[must_use]
    pub fn hover_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        stack
            .custom_name()
            .unwrap_or_else(|| self.get_behavior(stack.item()).get_name(stack))
    }

    /// Get all behaviors.
    #[cfg(feature = "test-framework")]
    #[must_use]
    pub fn get_behaviors(&self) -> &[Box<dyn ItemBehavior>] {
        &self.behaviors
    }
}

impl Default for ItemBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::data_components::{Consumable, vanilla_components};
    use steel_registry::item_stack::ItemStack;
    use steel_registry::stat::vanilla_stat_types;
    use steel_registry::{init_vanilla_registry, vanilla_entities, vanilla_items, vanilla_potions};
    use steel_utils::types::InteractionHand;
    use steel_utils::{ChunkPos, Downcast as _, WorldAabb};

    use super::finish_consuming_stack;
    use crate::behavior::{ITEM_BEHAVIORS, InteractionResult, UseItemContext, init_behaviors};
    use crate::entity::entities::ItemEntity;
    use crate::inventory::container::Container as _;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn potion_items_default_to_their_vanilla_base_potion() {
        init_vanilla_registry();
        init_behaviors();

        for item in [
            &vanilla_items::POTION,
            &vanilla_items::SPLASH_POTION,
            &vanilla_items::LINGERING_POTION,
        ] {
            let stack = ITEM_BEHAVIORS.default_instance(item);
            assert!(stack.is(item));
            assert_eq!(stack.count(), 1);
            let contents = stack
                .get(vanilla_components::POTION_CONTENTS)
                .expect("potion default instance carries potion contents");
            assert!(contents.is(&vanilla_potions::WATER));
        }

        let arrow = ITEM_BEHAVIORS.default_instance(&vanilla_items::TIPPED_ARROW);
        let contents = arrow
            .get(vanilla_components::POTION_CONTENTS)
            .expect("tipped arrow default instance carries potion contents");
        assert!(contents.is(&vanilla_potions::POISON));
    }

    #[test]
    fn items_without_an_override_default_to_a_plain_stack() {
        init_vanilla_registry();
        init_behaviors();

        let stack = ITEM_BEHAVIORS.default_instance(&vanilla_items::STONE);
        assert_eq!(stack, ItemStack::new(&vanilla_items::STONE));
    }

    /// Drinking one honey bottle out of a stack must only consume one item:
    /// the leftover bottles stay in hand, and the empty glass bottle it
    /// produces is handed off separately rather than replacing the whole
    /// stack. Mirrors vanilla `UseRemainder.convertIntoRemainder`.
    #[test]
    fn honey_bottle_stack_keeps_remaining_bottles_and_hands_off_the_remainder() {
        init_vanilla_registry();
        init_behaviors();
        let world_fixture = fresh_test_world("finish_consuming_honey_bottle_stack");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(world), "Test", 1).build();
        player.set_client_loaded(true);

        // Fill the inventory so the glass bottle remainder cannot be stored
        // and must be dropped instead — the scenario the honey-bottle fix
        // targets.
        {
            let mut inventory = player.inventory.lock();
            for slot in 0..36 {
                inventory.set_item(slot, ItemStack::with_count(&vanilla_items::STONE, 64));
            }
        }

        let stack = ItemStack::with_count(&vanilla_items::HONEY_BOTTLE, 5);
        let result = finish_consuming_stack(&stack, world, player.as_ref());

        assert!(result.is(&vanilla_items::HONEY_BOTTLE));
        assert_eq!(result.count(), 4);

        let dropped = world.get_entities_in_aabb_matching(
            &WorldAabb::new(-2.0, -1.0, -2.0, 2.0, 3.0, 2.0),
            |entity| entity.entity_type() == &vanilla_entities::ITEM,
        );
        assert_eq!(dropped.len(), 1);
        let Some(item) = dropped[0].as_ref().downcast_ref::<ItemEntity>() else {
            panic!("dropped entity should retain its concrete item type");
        };
        assert!(item.get_item().is(&vanilla_items::GLASS_BOTTLE));
    }

    /// Regression test for a deadlock: `use_item` on a `consume_seconds:
    /// 0.0` item must finish it immediately (mirroring vanilla `Consumable
    /// .startConsuming`'s `consumeTicks() > 0` branch) *without* holding the
    /// inventory lock while doing so — `finish_consuming_stack` can recurse
    /// into the same lock via `USE_REMAINDER` (honey bottle → glass bottle)
    /// through `handle_extra_items_created_on_use`, which used to deadlock
    /// when called from inside the lock `use_item` originally held.
    #[test]
    fn instant_consumable_with_use_remainder_finishes_without_deadlocking() {
        init_vanilla_registry();
        init_behaviors();
        let world_fixture = fresh_test_world("instant_consumable_no_deadlock");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(world), "Test", 1).build();
        player.set_client_loaded(true);

        let mut stack = ItemStack::with_count(&vanilla_items::HONEY_BOTTLE, 2);
        let existing = stack
            .get(vanilla_components::CONSUMABLE)
            .expect("honey bottle is consumable")
            .clone();
        let instant_consumable = Consumable::new(
            0.0,
            existing.animation(),
            existing.sound().clone(),
            existing.has_consume_particles(),
            existing.on_consume_effects().to_vec(),
        )
        .expect("valid consumable");
        stack.set(vanilla_components::CONSUMABLE, instant_consumable);

        {
            let mut inventory = player.inventory.lock();
            inventory.set_item_in_hand(InteractionHand::MainHand, stack);
        }

        let behavior = ITEM_BEHAVIORS.get_behavior(&vanilla_items::HONEY_BOTTLE);
        let mut context = UseItemContext::new(
            &player,
            InteractionHand::MainHand,
            world,
            Arc::clone(&player.inventory),
        );

        let result = behavior.use_item(&mut context);

        assert_eq!(result, InteractionResult::Consume);
        let remaining = {
            let inventory = player.inventory.lock();
            inventory
                .get_item_in_hand(InteractionHand::MainHand)
                .clone()
        };
        assert!(remaining.is(&vanilla_items::HONEY_BOTTLE));
        assert_eq!(remaining.count(), 1);

        let glass_bottle_count: i32 = {
            let inventory = player.inventory.lock();
            inventory
                .items()
                .iter()
                .filter(|item| item.is(&vanilla_items::GLASS_BOTTLE))
                .map(ItemStack::count)
                .sum()
        };
        assert_eq!(
            glass_bottle_count, 1,
            "USE_REMAINDER glass bottle should land in the player's inventory"
        );
    }

    /// Eating a food item must restore hunger/saturation by the exact
    /// vanilla amounts from its `minecraft:food` component, applied as-is
    /// (not recomputed from a modifier).
    #[test]
    fn eating_food_applies_its_nutrition_and_saturation() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("finish_consuming_food_applies_nutrition");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(world), "Test", 1).build();
        player.set_client_loaded(true);
        {
            let mut food = player.food_data.lock();
            food.food_level = 10;
            food.saturation_level = 0.0;
        }

        let stack = ItemStack::new(&vanilla_items::APPLE);
        let _ = finish_consuming_stack(&stack, world, player.as_ref());

        let food = player.food_data.lock();
        // Vanilla apple: nutrition 4, saturation 2.4.
        assert_eq!(food.food_level, 14);
        assert!((food.saturation_level - 2.4).abs() < f32::EPSILON);
    }

    /// Consuming an item must award one count of `Stats.ITEM_USED` for that
    /// item, matching vanilla `Consumable.onConsume`.
    #[test]
    fn consuming_an_item_awards_the_item_used_stat() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("finish_consuming_awards_item_used_stat");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(world), "Test", 1).build();
        player.set_client_loaded(true);

        let stack = ItemStack::new(&vanilla_items::APPLE);
        let _ = finish_consuming_stack(&stack, world, player.as_ref());

        let apple_used = vanilla_stat_types::ITEM_USED.get(&vanilla_items::APPLE);
        assert_eq!(
            player
                .stats()
                .into_iter()
                .find(|(stat, _)| *stat == apple_used)
                .map(|(_, count)| count),
            Some(1)
        );
    }
}
