//! Item behavior trait and registry.

use std::sync::Arc;

use steel_registry::data_components::vanilla_components::{
    BLOCKS_ATTACKS, CONSUMABLE, KINETIC_WEAPON,
};
use std::borrow::Cow;

use steel_registry::data_components::vanilla_components::ITEM_NAME;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};
use steel_utils::types::InteractionHand;
use text_components::TextComponent;

use crate::behavior::items::DefaultItemBehavior;
use crate::behavior::{InteractionResult, UseItemContext, UseOnContext};
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
        // TODO: Mirror Item.use/finishUsingItem for CONSUMABLE, BLOCKS_ATTACKS, and
        // KINETIC_WEAPON so specialized behaviors inherit the complete Vanilla base path.
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
    fn release_using(
        &self,
        _stack: &mut ItemStack,
        _world: &Arc<World>,
        _user: &dyn LivingEntity,
        _time_left: i32,
    ) {
    }

    /// Called when active use reaches its full duration.
    fn finish_using(
        &self,
        stack: &mut ItemStack,
        _world: &Arc<World>,
        _user: &dyn LivingEntity,
    ) -> ItemStack {
        stack.copy_with_count(stack.count())
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

    /// Gets the behavior for an item by its ID.
    #[must_use]
    pub fn get_behavior_by_id(&self, id: usize) -> Option<&dyn ItemBehavior> {
        self.behaviors.get(id).map(AsRef::as_ref)
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
