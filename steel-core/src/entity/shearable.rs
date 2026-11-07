//! Vanilla `Shearable`: mobs that shears act on (sheep, mooshrooms, snow golems, bogged).

use steel_protocol::packets::game::SoundSource;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_game_events;
use steel_utils::types::InteractionHand;

use crate::entity::LivingParts as _;
use crate::entity::prelude::*;
use crate::player::Player;
use crate::world::World;

/// A mob shears act on. Found through [`Entity::as_shearable`].
pub trait Shearable: Entity {
    /// `Shearable.shear`: drops the shearing loot and marks the mob sheared.
    fn shear(&self, world: &World, sound_source: SoundSource, tool: &ItemStack);

    /// `Shearable.readyForShearing`.
    fn ready_for_shearing(&self) -> bool;
}

/// What shearable mobs do when a ready one is sheared by a player in
/// `mobInteract`: shear it, emit `SHEAR` from the player, and wear the shears.
pub fn shear_by_player<E: Shearable + ?Sized>(entity: &E, player: &Player, hand: InteractionHand) {
    let Some(world) = entity.level() else {
        return;
    };
    let tool = player.inventory.lock().get_item_in_hand(hand).clone();
    entity.shear(&world, SoundSource::Players, &tool);
    entity.game_event_with_source_entity(&vanilla_game_events::SHEAR, Some(player));
    let has_infinite_materials = player.living().has_infinite_materials();
    player
        .inventory
        .lock()
        .hurt_item_in_hand(hand, 1, has_infinite_materials);
}
