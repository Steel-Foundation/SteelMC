//! Equipment slots, equipping and item use.

use super::Parts;
use super::{
    CEntityEvent, EntityStatus, EquipmentSlot, EquipmentSlotItem, InteractionHand,
    InteractionResult, InventoryTickContext, ItemRef, ItemStack, Living, Player, SoundEventRef,
    equipment_items_to_packet_items, vanilla_game_events,
};
use crate::entity::prelude::*;

impl Living<'_> {
    /// Returns whether the item in this entity's main hand matches `predicate`.
    pub fn is_holding(&self, predicate: &mut dyn FnMut(&ItemStack) -> bool) -> bool {
        let mut holding = false;
        self.with_equipment_slot(EquipmentSlot::MainHand, &mut |item_stack| {
            holding = predicate(item_stack);
        });
        if holding {
            return true;
        }

        self.with_equipment_slot(EquipmentSlot::OffHand, &mut |item_stack| {
            holding = predicate(item_stack);
        });
        holding
    }

    /// Returns whether this entity currently has an item in `slot`.
    #[must_use]
    pub fn has_item_in_slot(&self, slot: EquipmentSlot) -> bool {
        let mut has_item = false;
        self.with_equipment_slot(slot, &mut |item_stack| {
            has_item = !item_stack.is_empty();
        });
        has_item
    }

    /// Returns whether a dispenser may auto-equip this item onto this entity:
    /// alive, not a spectator, and the item is dispensable equipment for an
    /// open, usable slot this entity's type accepts.
    #[must_use]
    pub fn can_equip_with_dispenser(&self, item_stack: &ItemStack) -> bool {
        if !EntityMethods::is_alive(self.entity()) || self.is_spectator() {
            return false;
        }

        let Some(equippable) = item_stack.get_equippable() else {
            return false;
        };
        if !equippable.dispensable {
            return false;
        }

        let slot = equippable.slot;
        self.can_use_slot(slot)
            && equippable.can_be_equipped_by(self.entity_type())
            && !self.has_item_in_slot(slot)
            && self.can_dispenser_equip_into_slot(slot)
    }

    /// Returns whether `item_stack` may be worn in `slot`, defaulting to
    /// holdable-in-main-hand items with no equippable component.
    #[must_use]
    pub fn is_equippable_in_slot(&self, item_stack: &ItemStack, slot: EquipmentSlot) -> bool {
        let Some(equippable) = item_stack.get_equippable() else {
            return slot == EquipmentSlot::MainHand && self.can_use_slot(EquipmentSlot::MainHand);
        };

        slot == equippable.slot
            && self.can_use_slot(equippable.slot)
            && equippable.can_be_equipped_by(self.entity_type())
    }

    /// `LivingEntity.onEquipItem`: the equip sound and the equip game event.
    pub fn on_equip_item(&self, slot: EquipmentSlot, previous: &ItemStack, stack: &ItemStack) {
        if self.is_spectator()
            || self.is_first_tick()
            || ItemStack::is_same_item_same_components(previous, stack)
        {
            return;
        }

        if !self.is_silent()
            && let Some(sound) = self.equip_sound(slot, stack)
            && let Some(world) = self.level()
        {
            world.play_sound_at(sound, self.sound_source(), self.position(), 1.0, 1.0, None);
        }

        if self.does_emit_equip_event(slot) {
            let event = if stack.get_equippable().is_some() {
                &vanilla_game_events::EQUIP
            } else {
                &vanilla_game_events::UNEQUIP
            };
            self.game_event(event);
        }
    }

    /// Refreshes transient item attribute modifiers for one equipment slot.
    pub fn refresh_equipment_attribute_modifiers(&self, slot: EquipmentSlot) {
        self.with_equipment_slot(slot, &mut |item_stack| {
            self.living_base()
                .refresh_equipment_attribute_modifiers(slot, item_stack);
        });
    }

    /// Detects and applies Vanilla living-equipment changes once per tick.
    pub fn detect_equipment_updates(&self) {
        let mut changes = self.living_base().collect_equipment_changes();
        if changes.is_empty() {
            return;
        }

        for (slot, _, current) in &changes {
            self.living_base()
                .refresh_equipment_attribute_modifiers(*slot, current);
        }

        let main_hand = changes
            .iter()
            .find(|(slot, _, _)| *slot == EquipmentSlot::MainHand);
        let offhand = changes
            .iter()
            .find(|(slot, _, _)| *slot == EquipmentSlot::OffHand);
        let hands_swapped = main_hand.zip(offhand).is_some_and(
            |((_, previous_main, current_main), (_, previous_off, current_off))| {
                ItemStack::matches(current_main, previous_off)
                    && ItemStack::matches(current_off, previous_main)
            },
        );

        if hands_swapped {
            if let Some(world) = self.level() {
                world.broadcast_to_entity_trackers(
                    self.id(),
                    CEntityEvent {
                        entity_id: self.id(),
                        event: EntityStatus::SwapHands,
                    },
                    None,
                );
            }
            changes.retain(|(slot, _, _)| {
                !matches!(slot, EquipmentSlot::MainHand | EquipmentSlot::OffHand)
            });
        }

        self.living_base().queue_equipment_changes(
            changes
                .into_iter()
                .map(|(slot, _, current)| (slot, current)),
        );
    }

    /// Packs non-empty living equipment slots for initial spawn pairing.
    #[must_use]
    pub fn pack_living_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.living_base().equipment().lock().non_empty_items())
    }

    /// Drains dirty living equipment slots for tracker sync.
    #[must_use]
    pub fn drain_dirty_living_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.living_base().drain_equipment_changes())
    }
}

/// `LivingEntity`'s [`LivingProps::is_using_item`].
///
/// Checks if the entity is currently using an item.
#[must_use]
pub const fn is_using_item<E: Parts>(_entity: &E) -> bool {
    false
}

/// `LivingEntity`'s [`LivingProps::is_blocking`].
///
/// Checks if the entity is blocking with a shield or similar item.
#[must_use]
pub const fn is_blocking<E: Parts>(_entity: &E) -> bool {
    false
}

/// `LivingEntity`'s [`LivingProps::with_equipment_slot`].
///
/// Visits the item in a vanilla living-entity equipment slot.
pub fn with_equipment_slot<E: Parts>(
    entity: &E,
    slot: EquipmentSlot,
    visitor: &mut dyn FnMut(&ItemStack),
) {
    let living = &entity.living();
    let equipment = living.living_base().equipment().lock();
    visitor(equipment.get_ref(slot));
}

/// `LivingEntity`'s [`LivingProps::tick_equipment`].
///
/// Ticks every equipped item.
pub fn tick_equipment<E: Parts>(entity: &E) {
    let living = &entity.living();
    let (Some(world), Some(owner)) = (living.level(), living.as_living_entity()) else {
        return;
    };
    InventoryTickContext::tick_equipment(&world, &owner, EquipmentSlot::ALL);
}

/// `LivingEntity`'s [`LivingProps::with_equipment_slot_mut`].
///
/// Mutates the item in a vanilla living-entity equipment slot.
pub fn with_equipment_slot_mut<E: Parts>(
    entity: &E,
    slot: EquipmentSlot,
    visitor: &mut dyn FnMut(&mut ItemStack),
) {
    let living = &entity.living();
    let mut equipment = living.living_base().equipment().lock();
    visitor(equipment.get_mut(slot));
}

/// `LivingEntity`'s [`LivingProps::equip_sound`].
///
/// Returns the equip sound Steel can currently resolve for this entity.
#[must_use]
pub fn equip_sound<E: Parts>(
    _entity: &E,
    slot: EquipmentSlot,
    stack: &ItemStack,
) -> Option<SoundEventRef> {
    let equippable = stack.get_equippable()?;
    (slot == equippable.slot)
        .then(|| equippable.equip_sound.registry_ref())
        .flatten()
}

/// `LivingEntity`'s [`LivingProps::does_emit_equip_event`].
///
/// Whether changing `slot` emits the equip game event; every slot does by default.
#[must_use]
pub const fn does_emit_equip_event<E: Parts>(_entity: &E, _slot: EquipmentSlot) -> bool {
    true
}

/// `LivingEntity`'s [`LivingProps::interact_living_entity_with_equippable`].
///
/// Runs vanilla's equippable `ItemStack.interactLivingEntity` branch.
pub fn interact_living_entity_with_equippable<E: Parts>(
    entity: &E,
    player: &Player,
    hand: InteractionHand,
) -> InteractionResult {
    let living = &entity.living();
    let item_stack = {
        let inventory = player.inventory.lock();
        let item_stack = inventory.get_item_in_hand(hand);
        item_stack.copy_with_count(item_stack.count())
    };
    let Some(equippable) = item_stack.get_equippable() else {
        return InteractionResult::Pass;
    };
    if !equippable.equip_on_interact {
        return InteractionResult::Pass;
    }

    let slot = equippable.slot;
    if !living.is_equippable_in_slot(&item_stack, slot) || !EntityMethods::is_alive(living.entity())
    {
        return InteractionResult::Pass;
    }

    let equipped = {
        let mut equipment = living.living_base().equipment().lock();
        if !equipment.get_ref(slot).is_empty() {
            return InteractionResult::Pass;
        }

        let mut inventory = player.inventory.lock();
        if !living.is_equippable_in_slot(inventory.get_item_in_hand(hand), slot) {
            return InteractionResult::Pass;
        }

        let equipped = inventory.split_item_in_hand(hand, 1);
        if equipped.is_empty() {
            return InteractionResult::Pass;
        }

        equipment.set(slot, equipped);
        equipment.get_ref(slot).copy_with_count(1)
    };

    living.on_equip_item(slot, &ItemStack::empty(), &equipped);
    if let Some(mob) = living.as_mob() {
        mob.set_guaranteed_drop(slot);
    }
    // TODO: Emit EQUIP game event once game-event dispatch is implemented.
    InteractionResult::Success
}

/// `LivingEntity`'s [`LivingProps::has_infinite_materials`].
///
/// Returns whether equipment durability should be skipped for this entity.
#[must_use]
pub const fn has_infinite_materials<E: Parts>(_entity: &E) -> bool {
    false
}

/// `LivingEntity`'s [`LivingProps::handle_extra_items_created_on_use`].
///
/// Mirrors vanilla `LivingEntity.handleExtraItemsCreatedOnUse`,
/// which is a no-op for non-player mobs.
pub fn handle_extra_items_created_on_use<E: Parts>(_entity: &E, _extra: ItemStack) {}

/// `LivingEntity`'s [`LivingProps::on_equipped_item_broken`].
///
/// Called after an equipped item breaks.
pub fn on_equipped_item_broken<E: Parts>(entity: &E, _item: ItemRef, slot: EquipmentSlot) {
    let living = &entity.living();
    living.broadcast_entity_event(slot.into());
    living.refresh_equipment_attribute_modifiers(slot);
}
