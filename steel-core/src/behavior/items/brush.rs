//! Brush item behavior for continuous archaeology brushing.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events;
use steel_registry::vanilla_attributes;
use steel_utils::Downcast as _;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Direction};

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, ItemBehavior, ItemUseAnimation};
use crate::block_entity::entities::BrushableBlockEntity;
use crate::entity::projectile::{ViewVectorHitResult, get_hit_result_on_view_vector};
use crate::entity::{Entity, LivingEntity};
use crate::inventory::equipment::EquipmentSlot;
use crate::player::Player;
use crate::world::World;

const USE_DURATION: i32 = 200;
const ANIMATION_DURATION: i32 = 10;
const SOUND_TICK_OFFSET: i32 = 5;

/// Vanilla brush item behavior for continuous archaeology brushing.
#[item_behavior]
pub struct BrushItem;

impl ItemBehavior for BrushItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        if calculate_block_hit(context.world, context.player).is_some() {
            context.player.start_using_item(context.hand);
        }
        InteractionResult::Consume
    }

    fn get_use_duration(&self, _stack: &ItemStack, _user: &dyn LivingEntity) -> i32 {
        USE_DURATION
    }

    fn get_use_animation(&self, _stack: &ItemStack) -> ItemUseAnimation {
        ItemUseAnimation::Brush
    }

    fn on_use_tick(
        &self,
        world: &Arc<World>,
        user: &dyn LivingEntity,
        stack: &mut ItemStack,
        ticks_remaining: i32,
    ) {
        if ticks_remaining < 0 {
            release_player_use(user);
            return;
        }

        let Some(player) = user.as_player() else {
            return;
        };
        let Some((pos, direction)) = calculate_block_hit(world, player) else {
            player.release_using_item();
            return;
        };

        let elapsed = USE_DURATION - ticks_remaining + 1;
        if elapsed % ANIMATION_DURATION != SOUND_TICK_OFFSET {
            return;
        }

        let state = world.get_block_state(pos);
        let sound = BLOCK_BEHAVIORS
            .get_behavior_for_state(state)
            .and_then(|behavior| behavior.brushable_data(state))
            .map_or(
                &sound_events::ITEM_BRUSH_BRUSHING_GENERIC,
                |(_, sound, _)| sound,
            );
        world.play_block_sound(sound, pos, 1.0, 1.0, Some(player.id()));

        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let outcome = {
            let mut guard = block_entity.lock();
            let Some(brushable) = guard.downcast_mut::<BrushableBlockEntity>() else {
                return;
            };
            brushable.brush(world.game_time(), world, player, direction, stack)
        };
        outcome.mutation.apply(world, pos);

        if outcome.durability_damage {
            let slot = equipped_brush_slot(player, stack);
            if stack.hurt_and_break(1, player.has_infinite_materials()) {
                player.on_equipped_item_broken(slot);
            }
        }
    }
}

fn calculate_block_hit(world: &World, player: &Player) -> Option<(BlockPos, Direction)> {
    let distance = player
        .attributes()
        .lock()
        .get_value(vanilla_attributes::BLOCK_INTERACTION_RANGE)
        .unwrap_or(4.5);
    match get_hit_result_on_view_vector(world, player, distance, |entity| entity.is_pickable()) {
        ViewVectorHitResult::Block(hit) => Some((hit.block_pos, hit.direction)),
        ViewVectorHitResult::Miss | ViewVectorHitResult::Entity(_) => None,
    }
}

fn equipped_brush_slot(player: &Player, stack: &ItemStack) -> EquipmentSlot {
    let inventory = player.inventory.lock();
    let offhand = inventory.get_item_in_hand(InteractionHand::OffHand);
    if ItemStack::matches(stack, offhand) {
        EquipmentSlot::OffHand
    } else {
        EquipmentSlot::MainHand
    }
}

fn release_player_use(user: &dyn LivingEntity) {
    if let Some(player) = user.as_player() {
        player.release_using_item();
    }
}
