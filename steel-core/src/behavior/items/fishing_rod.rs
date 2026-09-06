use crate::behavior::{InteractionResult, ItemBehavior, UseItemContext};
use crate::entity::entities::objects::projectiles::FishingHookEntity;
use crate::entity::{Entity, RemovalReason, SharedEntity, next_entity_id};
use glam::DVec3;
use rand::{RngExt, rng};
use std::sync::Arc;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::sound_events::{ENTITY_FISHING_BOBBER_RETRIEVE, ENTITY_FISHING_BOBBER_THROW};
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{vanilla_entities, vanilla_items};

/// Behavior for the fishing rod item.
#[item_behavior]
pub struct FishingRodItem;

impl ItemBehavior for FishingRodItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let player = context.player;
        let infinite_materials = context.player.has_infinite_materials();
        let world = context.world;
        let item = context.inv.with_item(|item| item.clone());

        let pitch = 0.4 / (rng().random::<f32>() * 0.4 + 0.8);

        if let Some(fishing) = player.fishing_hook() {
            if item.is(&vanilla_items::FISHING_ROD) {
                let damage = fishing.retrieve(&item);

                context.inv.with_item(|item| {
                    item.hurt_and_break(damage, infinite_materials);
                });
            }

            world.play_sound_at(
                &ENTITY_FISHING_BOBBER_RETRIEVE,
                SoundSource::Neutral,
                player.position(),
                1.0,
                pitch,
                None,
            );
            // TODO: add vibration (Java equivalent: `ItemStack.causeVibration()`)
            // Problem: `ItemStack` in steel currently doesn't implement any fn that elicits this behaviour
            // Possibility: Re-Use the bone-meal exclusive implementation the bone-meal PR introduced
            //
            // Another Problem: If we were to do that, we'd run into a circular dependency; the existing implementation for bone-meal uses `UseItemOnContext`
            // (better would be `UseItemContext`, if we were to re-use this for all `ItemStacks`), which resides inside `steel-core`, but `ItemStack` resides inside
            // `steel-registry` and `steel-core` depends on `steel-registry` (I think, lmao)
        } else {
            world.play_sound_at(
                &ENTITY_FISHING_BOBBER_THROW,
                SoundSource::Neutral,
                player.position(),
                0.5,
                pitch,
                None,
            );

            let player_pos = player.position();
            let spawn_pos = DVec3::new(player_pos.x, player.get_eye_y() - 0.1, player_pos.z);

            let luck = item.apply_unconditional_enchantment_value_effects(
                EnchantmentEffectComponent::FishingLuckBonus,
                0.0,
            );

            let lure_speed = item.apply_unconditional_enchantment_value_effects(
                EnchantmentEffectComponent::FishingTimeReduction,
                0.0,
            );

            let hook = Arc::new(FishingHookEntity::new(
                &vanilla_entities::FISHING_BOBBER,
                next_entity_id(),
                spawn_pos,
                Arc::downgrade(world),
            ));

            let player_arc = world
                .players
                .get_by_uuid(&player.gameprofile.id)
                .expect("Failed to obtain `player_arc` for launching fishing hook: `player` must be registered in the world.");

            hook.shoot_from_player(&player_arc, luck as i32, lure_speed as i32);

            let entity: SharedEntity = hook;

            if let Err(error) = world.try_add_entity(Arc::clone(&entity)) {
                entity.set_removed(RemovalReason::Discarded);
                log::error!("Failed to spawn fishing hook: {error}");
                return InteractionResult::Fail;
            }

            player.award_stat(&vanilla_stat_types::ITEM_USED, item.item());

            // TODO: add vibration, see above TODO
        }
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{item_stack::ItemStack, vanilla_items};
    use steel_utils::types::InteractionHand;
    use uuid::Uuid;

    use super::*;
    use crate::behavior::UseItemContext;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world};

    #[test]
    fn retrieving_grounded_hook_does_not_relock_inventory() {
        let world = fresh_test_world("fishing_rod_grounded_retrieve");
        let player = TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(1), 1).build();
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::FISHING_ROD));

        let player_owner = Arc::clone(&player);
        let owner: SharedEntity = player_owner;
        let hook = Arc::new(FishingHookEntity::new(
            &vanilla_entities::FISHING_BOBBER,
            2,
            DVec3::ZERO,
            Arc::downgrade(&world),
        ));
        hook.set_owner(&owner);
        hook.set_on_ground(true);

        let mut context = UseItemContext::new(
            &player,
            InteractionHand::MainHand,
            &world,
            Arc::clone(&player.inventory),
        );

        assert_eq!(
            FishingRodItem.use_item(&mut context),
            InteractionResult::Success
        );
        assert!(hook.is_removed());
        assert!(player.fishing_hook().is_none());
        assert_eq!(context.inv.with_item(|item| item.get_damage_value()), 2);
    }

    #[test]
    fn casting_fishing_rod_spawns_and_links_hook() {
        use crate::test_support::insert_ready_full_chunk;
        use steel_utils::ChunkPos;

        let world = fresh_test_world("fishing_rod_cast");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(2), 10).build();
        player
            .try_set_position(DVec3::new(8.0, 64.0, 8.0))
            .expect("should position player in center of chunk");
        world.players.insert(Arc::clone(&player));
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::FISHING_ROD));

        let mut context = UseItemContext::new(
            &player,
            InteractionHand::MainHand,
            &world,
            Arc::clone(&player.inventory),
        );

        assert_eq!(
            FishingRodItem.use_item(&mut context),
            InteractionResult::Success
        );
        assert!(player.fishing_hook().is_some());
    }
}
