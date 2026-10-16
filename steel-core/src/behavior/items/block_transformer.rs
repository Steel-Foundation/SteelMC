use glam::DVec3;
use steel_protocol::packets::game::SoundSource;
use steel_registry::block_transformer::{
    BlockTransformData, DropStrategy, TransformParticle, TransformType,
};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::vanilla_components::{BLOCK_TRANSFORMER, BLOCKS_ATTACKS};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{REGISTRY, RegistryExt as _, level_events, vanilla_game_events};
use steel_utils::BlockStateId;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::{InteractionHand, UpdateFlags};

use crate::behavior::block::drop_from_block_interact_loot_table;
use crate::behavior::{InteractionResult, UseOnContext};
use crate::block_state_provider::BlockStateProviderEvaluator;
use crate::entity::{Entity as _, LivingEntity as _};
use crate::inventory::equipment::EquipmentSlot;
use crate::player::Player;
use crate::world::game_event::GameEventContext;

use super::copper_chest_events::emit_connected_chest_block_change;

pub(crate) fn use_on(context: &mut UseOnContext) -> InteractionResult {
    let transformer = context.inv.with_item(|item| {
        item.get(BLOCK_TRANSFORMER)
            .map(|component| component.block_transformer)
    });

    let Some(transformer) = transformer else {
        return InteractionResult::Pass;
    };

    if player_has_blocking_item_use_intent(context) {
        return InteractionResult::Pass;
    }

    let pos = context.hit_result.block_pos;
    let mut random = LegacyRandom::from_seed(rand::random());

    for transform in &transformer.transforms {
        if transform
            .disallowed_faces
            .contains(&context.hit_result.direction)
        {
            continue;
        }

        let Some(new_state) = BlockStateProviderEvaluator::sample_block_state_provider_optional(
            context.world.as_ref(),
            &REGISTRY,
            &mut random,
            &transform.block_state_provider,
            pos,
        ) else {
            continue;
        };

        let new_state = if transform.update_from_neighbors {
            context.world.update_from_neighbor_shapes(new_state, pos)
        } else {
            new_state
        };

        // TODO: Trigger ITEM_USED_ON_BLOCK advancements here
        let old_state = context.world.get_block_state(pos);
        drop_loot(transform, context, old_state);
        let broken_item = context.inv.with_item(|item| {
            consume_transform_item(item, transform, context.player.has_infinite_materials())
        });

        if let Some(item) = broken_item {
            let slot = match context.hand {
                InteractionHand::MainHand => EquipmentSlot::MainHand,
                InteractionHand::OffHand => EquipmentSlot::OffHand,
            };
            context.player.on_equipped_item_broken(item, slot);
        }

        context
            .world
            .set_block(pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);

        let (x, y, z) = pos.get_center();
        context.world.play_sound_holder_at(
            &transform.sound,
            SoundSource::Blocks,
            DVec3::new(x, y, z),
            1.0,
            1.0,
            Some(context.player.id()),
        );

        let particle_event = particle_event(transform.particle);
        if let Some(event) = particle_event {
            context
                .world
                .level_event(event, pos, 0, Some(context.player.id()));
        }

        let player: &Player = context.player;
        context.world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), Some(new_state)),
        );

        if transform.transform_type == TransformType::CopperChest
            && old_state.get_block().has_tag(&BlockTag::COPPER_CHESTS)
        {
            emit_connected_chest_block_change(
                context.world,
                pos,
                old_state,
                context.player,
                particle_event,
            );
        }

        return InteractionResult::Success;
    }
    InteractionResult::Pass
}

fn player_has_blocking_item_use_intent(context: &UseOnContext) -> bool {
    context.hand == InteractionHand::MainHand
        && !context.player.is_secondary_use_active()
        && context
            .player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::OffHand)
            .has(BLOCKS_ATTACKS)
}

fn consume_transform_item(
    item: &mut ItemStack,
    transform: &BlockTransformData,
    has_infinite_materials: bool,
) -> Option<ItemRef> {
    if item.is_stackable() {
        if transform.consume_on_use && !has_infinite_materials {
            item.shrink_one();
        }

        return None;
    }

    let item_ref = item.item();
    item.hurt_and_break(transform.item_damage_per_use, has_infinite_materials)
        .then_some(item_ref)
}

fn drop_loot(transform: &BlockTransformData, context: &UseOnContext, old_state: BlockStateId) {
    let Some(loot) = &transform.loot else {
        return;
    };

    let Some(table) = REGISTRY.loot_tables.by_key(loot) else {
        return;
    };

    let pos = context.hit_result.block_pos;
    let tool = context.inv.with_item(|item| item.clone());

    let player: &Player = context.player;
    let drops = drop_from_block_interact_loot_table(
        table,
        old_state,
        context.world.get_block_entity(pos),
        Some(&tool),
        Some(player),
        &mut rand::rng(),
    );

    for drop in drops {
        match transform.drop_strategy {
            DropStrategy::ClickedFace => {
                context
                    .world
                    .pop_resource_from_face(pos, context.hit_result.direction, drop);
            }
            DropStrategy::FromMiddle => {
                context.world.pop_resource(pos, drop);
            }
        }
    }
}

const fn particle_event(particle: TransformParticle) -> Option<i32> {
    match particle {
        TransformParticle::None => None,
        TransformParticle::Scrape => Some(level_events::PARTICLES_SCRAPE),
        TransformParticle::WaxOn => Some(level_events::PARTICLES_WAX_ON),
        TransformParticle::WaxOff => Some(level_events::PARTICLES_WAX_OFF),
    }
}

#[cfg(test)]
mod tests;
