use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::{
    entity_type::EntityTypeRef, item_stack::ItemStack, sound_events, vanilla_entities,
    vanilla_game_events,
};

use crate::{
    behavior::{InteractionResult, ItemBehavior, UseOnContext},
    entity::{
        BlockAttachedEntity, Entity, SharedEntity, apply_item_stack_components,
        entities::ItemFrameEntity, next_entity_id,
    },
    world::game_event::GameEventContext,
};

/// Places normal and glowing frames on any permitted face.
#[item_behavior(class = "ItemFrameItem")]
pub struct ItemFrameItem {
    #[json_arg(vanilla_entities, json = "type")]
    entity_type: EntityTypeRef,
}

impl ItemFrameItem {
    /// Creates frame behavior using the extracted entity type.
    #[must_use]
    pub const fn new(entity_type: EntityTypeRef) -> Self {
        Self { entity_type }
    }
}

impl ItemBehavior for ItemFrameItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let direction = context.hit_result.direction;
        let pos = context.hit_result.block_pos.relative(direction);
        let stack = context.inv.with_item(|item| item.clone());
        if context.world.is_outside_build_height(pos.y())
            || !context.player.may_use_item_at(pos, direction, &stack)
        {
            return InteractionResult::Fail;
        }
        let frame = Arc::new(ItemFrameEntity::new_attached(
            self.entity_type,
            next_entity_id(),
            pos,
            direction,
            Arc::downgrade(context.world),
        ));
        let entity: SharedEntity = Arc::<ItemFrameEntity>::clone(&frame);
        if let Err(error) =
            apply_item_stack_components(&entity, &stack, context.player.is_operator())
        {
            log::warn!("Could not apply frame item data at {pos:?}: {error:?}");
            return InteractionResult::Fail;
        }
        if !frame.survives() {
            return InteractionResult::Consume;
        }
        frame.play_sound(
            if self.entity_type == &vanilla_entities::GLOW_ITEM_FRAME {
                &sound_events::ENTITY_GLOW_ITEM_FRAME_PLACE
            } else {
                &sound_events::ENTITY_ITEM_FRAME_PLACE
            },
            1.0,
            1.0,
        );
        context.world.game_event(
            &vanilla_game_events::ENTITY_PLACE,
            pos,
            &GameEventContext::new(Some(context.player.as_ref()), None),
        );
        if let Err(error) = context.world.try_add_entity(entity) {
            log::warn!("Could not add item frame at {pos:?}: {error}");
            return InteractionResult::Fail;
        }
        context.inv.with_item(ItemStack::shrink_one);
        InteractionResult::Success
    }
}
