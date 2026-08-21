//! Item behavior trait and registry.

use std::sync::Arc;

use std::borrow::Cow;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::vanilla_components::{
    BLOCKS_ATTACKS, CONSUMABLE, KINETIC_WEAPON, USE_REMAINDER,
};

use steel_registry::data_components::vanilla_components::ITEM_NAME;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};
use steel_utils::types::InteractionHand;
use text_components::TextComponent;

use crate::behavior::items::DefaultItemBehavior;
use crate::behavior::{InteractionResult, UseItemContext, UseOnContext};
use crate::entity::apply_consume_effect;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, LivingEntity};
use crate::player::{Player, player_inventory::EquipmentSwapResult};
use crate::world::World;

pub use steel_registry::data_components::vanilla_components::ItemUseAnimation;

/// Trait defining the behavior of an item.
///
/// This trait handles dynamic/functional aspects of items:
/// - Use on blocks (placing, interacting)
/// - Use in air
/// - etc.
pub trait ItemBehavior: Send + Sync {
    /// Returns the Rust type name of the concrete behavior implementation.
    #[cfg(feature = "flint")]
    #[must_use]
    #[expect(clippy::absolute_paths, reason = "easier for features")]
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns vanilla `Item.getName(stack)`.
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        stack
            .get(ITEM_NAME)
            .map_or_else(|| Cow::Owned(TextComponent::new()), Cow::Borrowed)
    }

    /// Called when this item is used on a block.
    fn use_on(&self, _context: &mut UseOnContext) -> InteractionResult {
        InteractionResult::Pass
    }

    /// Called when this item is used (e.g. right click in air).
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        // TODO: Mirror Item.use for BLOCKS_ATTACKS and KINETIC_WEAPON so
        // specialized behaviors inherit the complete Vanilla base path.
        if context.inv.with_item(|item| item.has(CONSUMABLE)) {
            // TODO: Gate this on vanilla food eligibility (hunger not full,
            // unless the food is always edible) once the food system lands.
            context.player.start_using_item(context.hand);
            return InteractionResult::Consume;
        }

        let Some(equippable) = context.inv.with_item(|item| item.get_equippable().cloned()) else {
            return InteractionResult::Pass;
        };

        if !equippable.swappable || !equippable.can_be_equipped_by(context.player.entity_type()) {
            return InteractionResult::Pass;
        }

        let slot = equippable.slot;
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
                    let _ = context.player.drop_item(overflow, false, false);
                }
                InteractionResult::Success
            }
            EquipmentSwapResult::Fail => InteractionResult::Fail,
        }
    }

    /// Returns vanilla `Item.getUseAnimation`.
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

    /// Returns vanilla `Item.getUseDuration`.
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
        _user: &dyn LivingEntity,
        _stack: &mut ItemStack,
        _ticks_remaining: i32,
    ) {
    }

    /// Called when active use is released before completion.
    ///
    /// Returns whether vanilla should update active use once more before stopping it.
    fn release_using(
        &self,
        _stack: &mut ItemStack,
        _world: &Arc<World>,
        _user: &dyn LivingEntity,
        _time_left: i32,
    ) -> bool {
        false
    }

    /// Called when active use reaches its full duration.
    fn finish_using(
        &self,
        stack: &mut ItemStack,
        world: &Arc<World>,
        user: &dyn LivingEntity,
    ) -> ItemStack {
        finish_consuming_stack(stack, world, user)
    }

    /// Called by vanilla `ItemStack.interactLivingEntity`.
    fn interact_living_entity(
        &self,
        _stack: &mut ItemStack,
        _player: &Player,
        _target: &dyn LivingEntity,
        _hand: InteractionHand,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    /// Returns vanilla `Item.getItemDamageSource`.
    fn get_item_damage_source(&self, _attacker: &dyn LivingEntity) -> Option<DamageSource> {
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

    /// Called by vanilla `Item.hurtEnemy`.
    fn hurt_enemy(
        &self,
        _stack: &mut ItemStack,
        _target: &dyn LivingEntity,
        _attacker: &dyn LivingEntity,
    ) {
    }

    /// Called by vanilla `Item.postHurtEnemy`.
    fn post_hurt_enemy(
        &self,
        _stack: &mut ItemStack,
        _target: &dyn LivingEntity,
        _attacker: &dyn LivingEntity,
    ) {
    }

    /// Returns how much durability this weapon consumes after a successful entity hit.
    fn item_damage_per_attack(&self, stack: &ItemStack) -> Option<i32> {
        stack
            .get_weapon()
            .map(|weapon| weapon.item_damage_per_attack)
    }
}

/// Applies vanilla `Consumable.onConsume`'s shared tail: runs
/// `on_consume_effects`, plays the consume sound, then shrinks the stack by
/// one (creative mode leaves it untouched).
///
/// Shared between the default [`ItemBehavior::finish_using`] and
/// `PotionItem::finish_using`, which additionally applies `PotionContents`
/// before calling this.
pub(crate) fn finish_consuming_stack(
    stack: &ItemStack,
    world: &Arc<World>,
    user: &dyn LivingEntity,
) -> ItemStack {
    let Some(consumable) = stack.get(CONSUMABLE) else {
        return apply_use_remainder(stack, stack.copy_with_count(stack.count()), user);
    };

    for effect in consumable.on_consume_effects() {
        apply_consume_effect(effect, world, user);
    }

    if let Some(sound) = consumable.sound().registry_ref() {
        world.play_sound_at(sound, SoundSource::Players, user.position(), 1.0, 1.0, None);
    }
    // TODO: Spawn item-crumb particles when `has_consume_particles()` is set.

    let mut used_stack = stack.copy_with_count(stack.count());
    if !user.has_infinite_materials() {
        used_stack.shrink(1);
    }

    apply_use_remainder(stack, used_stack, user)
}

/// Applies vanilla `UseRemainder.convertIntoRemainder`: if the original stack
/// had a `use_remainder` and was actually consumed, either swap the fully
/// emptied stack for the remainder, or — for a stack that still has items
/// left (e.g. one honey bottle out of several) — hand the remainder off to
/// [`LivingEntity::handle_extra_items_created_on_use`] instead of discarding
/// it, and keep the (shrunk) original stack.
fn apply_use_remainder(
    original_stack: &ItemStack,
    used_stack: ItemStack,
    user: &dyn LivingEntity,
) -> ItemStack {
    let Some(remainder) = original_stack.get(USE_REMAINDER) else {
        return used_stack;
    };
    if user.has_infinite_materials() || used_stack.count() >= original_stack.count() {
        return used_stack;
    }

    let remainder_stack = remainder.convert_into().create();
    if used_stack.is_empty() {
        return remainder_stack;
    }

    user.handle_extra_items_created_on_use(remainder_stack);
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

    /// Returns vanilla `ItemStack.getHoverName`, including item-specific
    /// `Item.getName(stack)` overrides when no custom name is present.
    #[must_use]
    pub fn hover_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        stack
            .custom_name()
            .unwrap_or_else(|| self.get_behavior(stack.item()).get_name(stack))
    }

    /// Get all behaviors.
    #[cfg(feature = "flint")]
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
    use steel_registry::{init_vanilla_registry, vanilla_items};
    use steel_utils::{ChunkPos, Downcast as _, WorldAabb};
    use uuid::Uuid;

    use super::finish_consuming_stack;
    use crate::entity::entities::ItemEntity;
    use crate::inventory::container::Container as _;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    /// Drinking one honey bottle out of a stack must only consume one item:
    /// the leftover bottles stay in hand, and the empty glass bottle it
    /// produces is handed off separately rather than replacing the whole
    /// stack. Mirrors vanilla `UseRemainder.convertIntoRemainder`.
    #[test]
    fn honey_bottle_stack_keeps_remaining_bottles_and_hands_off_the_remainder() {
        init_vanilla_registry();
        let world = fresh_test_world("finish_consuming_honey_bottle_stack");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(world.clone(), Uuid::from_u128(1), "Test", 1).build();
        player.set_client_loaded(true);

        // Fill the inventory so the glass bottle remainder cannot be stored
        // and must be dropped instead — the scenario the honey-bottle fix
        // targets.
        {
            let mut inventory = player.inventory.lock();
            for slot in 0..36 {
                inventory.set_item(
                    slot,
                    steel_registry::item_stack::ItemStack::with_count(&vanilla_items::STONE, 64),
                );
            }
        }

        let stack =
            steel_registry::item_stack::ItemStack::with_count(&vanilla_items::HONEY_BOTTLE, 5);
        let result = finish_consuming_stack(&stack, &world, player.as_ref());

        assert!(result.is(&vanilla_items::HONEY_BOTTLE));
        assert_eq!(result.count(), 4);

        let dropped = world.get_entities_in_aabb_matching(
            &WorldAabb::new(-2.0, -1.0, -2.0, 2.0, 3.0, 2.0),
            |entity| entity.entity_type() == &steel_registry::vanilla_entities::ITEM,
        );
        assert_eq!(dropped.len(), 1);
        let Some(item) = dropped[0].as_ref().downcast_ref::<ItemEntity>() else {
            panic!("dropped entity should retain its concrete item type");
        };
        assert!(item.get_item().is(&vanilla_items::GLASS_BOTTLE));
    }
}
