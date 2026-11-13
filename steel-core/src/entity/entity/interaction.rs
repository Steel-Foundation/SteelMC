//! Player interaction, leashes and shearing.

use super::*;

/// Player interaction, leashes and shearing.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityProps`], never by implementing this trait.
pub trait EntityInteraction: Entity {
    /// Returns whether vanilla lets this entity interact with its loaded level.
    fn can_interact_with_level(&self) -> bool {
        self.is_alive() && !self.is_removed() && !self.is_spectator()
    }

    /// Finds leashable mobs in vanilla's nearby leash scan whose holder is this entity.
    fn leashables_leashed_to(&self) -> Vec<SharedEntity> {
        self.leashables_leashed_to_holder_in_area(self.as_entity())
    }

    /// Finds leashable mobs in this entity's nearby leash scan whose holder is `holder`.
    fn leashables_leashed_to_holder_in_area(&self, holder: &dyn Entity) -> Vec<SharedEntity> {
        let Some(world) = self.level() else {
            return Vec::new();
        };
        leashables_leashed_to_holder_in_area_near_position(
            &world,
            world_aabb_center(self.bounding_box()),
            holder,
        )
    }

    /// Transfers leashables currently held by `old_holder` to this entity.
    fn transfer_leashables_from_holder(&self, old_holder: &dyn Entity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(new_holder) = world.get_entity_by_id(self.id()) else {
            return false;
        };

        transfer_leashables_to_holder(
            self.leashables_leashed_to_holder_in_area(old_holder),
            &new_holder,
        )
    }

    /// Drops this entity's leash and all leashables attached to it.
    fn drop_all_leash_connections(&self, player: Option<&Player>) -> bool {
        let leashables = self.leashables_leashed_to();
        let mut dropped = !leashables.is_empty();

        if let Some(mob) = self.as_leashable()
            && mob.is_leashed()
        {
            mob.drop_leash();
            dropped = true;
        }

        for leashable in leashables {
            if let Some(mob) = leashable.as_leashable() {
                mob.drop_leash();
            }
        }

        if !dropped {
            return false;
        }

        let source_entity = player.map(|player| player as &dyn Entity);
        self.game_event_with_source_entity(&vanilla_game_events::SHEAR, source_entity);
        true
    }

    /// Shears off all leash connections reachable from this entity.
    fn shear_off_all_leash_connections(&self, player: Option<&Player>) -> bool {
        if !self.drop_all_leash_connections(player) {
            return false;
        }

        if let Some(world) = self.level() {
            let sound_source =
                player.map_or_else(|| self.sound_source(), EntityMethods::sound_source);
            world.play_sound(
                &sound_events::ITEM_SHEARS_SNIP,
                sound_source,
                self.block_position(),
                1.0,
                1.0,
                None,
            );
        }
        true
    }

    /// Shears the first shearable equipped item on this mob, spawning it as a
    /// dropped item and playing its shearing sound; returns whether anything was sheared.
    fn attempt_to_shear_equipment(&self, player: &Player, hand: InteractionHand) -> bool {
        let Some(mob) = self.as_mob() else {
            return false;
        };
        if !mob.can_shear_equipment(player) || player.is_secondary_use_active() {
            return false;
        }

        let has_infinite_materials = player.has_infinite_materials();
        for slot in EquipmentSlot::ALL {
            let sheared = {
                let mut equipment = mob.living_base().equipment().lock();
                let item_stack = equipment.get_ref(slot);
                let Some(equippable) = item_stack.get_equippable() else {
                    continue;
                };
                if !equippable.can_be_sheared
                    || !has_infinite_materials
                        && item_stack
                            .has_enchantment_effect(EnchantmentEffectComponent::PreventArmorChange)
                {
                    continue;
                }

                let shearing_sound = equippable.shearing_sound.registry_ref();
                (equipment.take(slot), shearing_sound)
            };
            let (item_stack, shearing_sound) = sheared;
            if item_stack.is_empty() {
                continue;
            }

            player
                .inventory
                .lock()
                .hurt_item_in_hand(hand, 1, has_infinite_materials);
            mob.set_guaranteed_drop(slot);
            mob.set_persistence_required();

            self.game_event_with_source_entity(&vanilla_game_events::SHEAR, Some(player));
            if let Some(shearing_sound) = shearing_sound {
                self.play_sound(shearing_sound, 1.0, 1.0);
            }

            let dimensions = self.base().dimensions();
            let spawn_offset = dimensions
                .attachments
                .get_average(EntityAttachment::Passenger, dimensions);
            let _ = self.spawn_at_location_with_offset(item_stack, spawn_offset);
            // TODO: Trigger PLAYER_SHEARED_EQUIPMENT once advancement criteria exist.
            return true;
        }

        false
    }
}

impl<T: Entity + ?Sized> EntityInteraction for T {}

/// `Entity`'s [`EntityProps::notify_leash_holder`].
pub fn notify_leash_holder<E: Entity + ?Sized>(_entity: &E, _leashable: &dyn Entity) {}

/// `Entity`'s [`EntityProps::notify_leashee_removed`].
pub fn notify_leashee_removed<E: Entity + ?Sized>(_entity: &E, _leashable: &dyn Entity) {}

/// `Entity`'s [`EntityProps::player_touch`].
pub const fn player_touch<E: Entity + ?Sized>(_entity: &E, _player: &Arc<Player>) {}

/// `Entity`'s [`EntityProps::interact`]: leashing/unleashing, cutting leash
/// connections, or shearing equipment with shears.
pub fn interact<E: Entity + ?Sized>(
    entity: &E,
    player: &Player,
    hand: InteractionHand,
    _location: DVec3,
) -> InteractionResult {
    // Vanilla's `Leashable` is Steel's mob layer.
    let mob = entity.as_mob();
    let is_alive = entity
        .as_living_entity()
        .is_none_or(|living| living.is_alive());

    if let Some(mob) = &mob
        && is_alive
        && player.is_secondary_use_active()
        && mob.leashable().can_be_leashed()
        && entity
            .as_living_entity()
            .is_none_or(|living| !living.is_baby())
        && entity.transfer_leashables_from_holder(player)
    {
        if let Some(world) = entity.level() {
            world.game_event(
                &vanilla_game_events::ENTITY_ACTION,
                entity.block_position(),
                &GameEventContext::new(Some(player), None),
            );
        }
        entity.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
        return InteractionResult::SuccessServer;
    }

    let holding_shears = {
        let inventory = player.inventory.lock();
        inventory.get_item_in_hand(hand).is(&vanilla_items::SHEARS)
    };
    if holding_shears && entity.shear_off_all_leash_connections(Some(player)) {
        let has_infinite_materials = player.has_infinite_materials();
        player
            .inventory
            .lock()
            .hurt_item_in_hand(hand, 1, has_infinite_materials);
        return InteractionResult::Success;
    }
    if holding_shears && entity.attempt_to_shear_equipment(player, hand) {
        return InteractionResult::Success;
    }

    let Some(mob) = mob else {
        return InteractionResult::Pass;
    };
    if !is_alive {
        return InteractionResult::Pass;
    }

    if let Some(holder) = mob.leashable().leash_holder() {
        if holder.id() == player.id() {
            if player.has_infinite_materials() {
                mob.leashable().remove_leash();
            } else {
                mob.leashable().drop_leash();
            }

            entity
                .game_event_with_source_entity(&vanilla_game_events::ENTITY_INTERACT, Some(player));
            entity.play_sound(&sound_events::ITEM_LEAD_UNTIED, 1.0, 1.0);
            return InteractionResult::Success;
        }

        if holder.as_player().is_some() {
            return InteractionResult::Pass;
        }
    }

    let holding_lead = {
        let inventory = player.inventory.lock();
        inventory.get_item_in_hand(hand).is(&vanilla_items::LEAD)
    };
    if !holding_lead || !mob.leashable().can_have_a_leash_attached_to(player) {
        return InteractionResult::Pass;
    }

    let Some(world) = entity.level() else {
        return InteractionResult::Pass;
    };
    let Some(player_entity) = world.get_entity_by_id(player.id()) else {
        return InteractionResult::Pass;
    };

    if mob.leashable().is_leashed() {
        mob.leashable().drop_leash();
    }
    if !mob.leashable().set_leashed_to(&player_entity) {
        return InteractionResult::Pass;
    }

    entity.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
    player.inventory.lock().shrink_item_in_hand(hand, 1);
    InteractionResult::SuccessServer
}

/// `Entity`'s [`EntityProps::may_interact`].
pub const fn may_interact<E: Entity + ?Sized>(_entity: &E, _world: &World, _pos: BlockPos) -> bool {
    true
}
