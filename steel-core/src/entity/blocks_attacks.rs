//! Server side effects of the `minecraft:blocks_attacks` item component.
//!
//! The pure damage math lives on [`BlocksAttacks`] in `steel-registry`; these
//! helpers need the world, the blocking entity, and its inventory.

use steel_registry::data_components::BlocksAttacks;
use steel_registry::equipment::EquipmentSlot;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventHolder;
use steel_registry::stat::vanilla_stat_types;
use steel_utils::types::InteractionHand;

use crate::entity::LivingEntity;
use crate::world::World;

/// Plays the block sound after `user` blocks an attack.
pub(crate) fn on_blocked<E: LivingEntity + ?Sized>(
    blocks_attacks: &BlocksAttacks,
    world: &World,
    user: &E,
) {
    play_user_sound(world, user, blocks_attacks.block_sound(), 1.0);
}

/// Disables blocking after an attack with `base_seconds` of blocking disable.
pub(crate) fn disable<E: LivingEntity + ?Sized>(
    blocks_attacks: &BlocksAttacks,
    world: &World,
    user: &E,
    base_seconds: f32,
    blocking_with: &ItemStack,
) {
    let cooldown_ticks = blocks_attacks.disable_blocking_for_ticks(base_seconds);
    if cooldown_ticks <= 0 {
        return;
    }
    if let Some(player) = user.as_player() {
        player.add_item_cooldown(blocking_with, cooldown_ticks);
    }
    user.stop_using_item();
    play_user_sound(world, user, blocks_attacks.disabled_sound(), 0.8);
}

/// Awards item use and applies durability damage for blocking `damage`.
pub(crate) fn hurt_blocking_item<E: LivingEntity + ?Sized>(
    blocks_attacks: &BlocksAttacks,
    item: &ItemStack,
    user: &E,
    hand: InteractionHand,
    damage: f32,
) {
    let Some(player) = user.as_player() else {
        return;
    };
    player.award_stat(&vanilla_stat_types::ITEM_USED, item.item());

    let item_damage = blocks_attacks.item_damage().apply(damage);
    if item_damage <= 0 {
        return;
    }
    let slot = EquipmentSlot::from(hand);
    let mut broke = false;
    user.with_equipment_slot_mut(slot, &mut |stack| {
        broke = stack.hurt_and_break(item_damage, user.has_infinite_materials());
    });
    if broke {
        user.on_equipped_item_broken(item.item(), slot);
    }
}

fn play_user_sound<E: LivingEntity + ?Sized>(
    world: &World,
    user: &E,
    sound: Option<&SoundEventHolder>,
    volume: f32,
) {
    let Some(sound) = sound else {
        return;
    };
    world.play_sound_holder_at(
        sound,
        user.sound_source(),
        user.position(),
        volume,
        0.8 + rand::random::<f32>() * 0.4,
        None,
    );
}

#[cfg(test)]
mod tests;
