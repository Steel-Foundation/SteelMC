use glam::DVec3;
use steel_macros::item_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_events;

use crate::behavior::{BLOCK_BEHAVIORS, InteractionResult, ItemBehavior, UseOnContext};
use crate::entity::entities::objects::vehicles::create_minecart;
use crate::world::game_event::GameEventContext;

const RAIL_HEIGHT_OFFSET: f64 = 0.0625;
const SLOPED_RAIL_HEIGHT_OFFSET: f64 = 0.5;

/// Item behavior for minecart entity.
#[item_behavior(class = "MinecartItem")]
pub struct MinecartItem {
    #[json_arg(vanilla_entities, json = "type")]
    entity_type: EntityTypeRef,
}

impl MinecartItem {
    /// Creates a minecart item behavior.
    #[must_use]
    pub const fn new(entity_type: EntityTypeRef) -> Self {
        Self { entity_type }
    }
}

impl ItemBehavior for MinecartItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;
        let state = context.world.get_block_state(pos);
        let block = state.get_block();

        if !block.has_tag(&BlockTag::RAILS) {
            return InteractionResult::Fail;
        }

        let shape = if BLOCK_BEHAVIORS.get_behavior(block).as_rail().is_some() {
            state.get_value(&BlockStateProperties::RAIL_SHAPE)
        } else {
            RailShape::NorthSouth
        };

        let offset = if shape.is_slope() {
            SLOPED_RAIL_HEIGHT_OFFSET
        } else {
            0.0
        };
        let position = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + RAIL_HEIGHT_OFFSET + offset,
            f64::from(pos.z()) + 0.5,
        );

        let stack = context.inv.with_item(|stack| stack.clone());
        let Some(cart) = create_minecart(
            context.world,
            position,
            self.entity_type,
            &stack,
            Some(context.player),
        ) else {
            return InteractionResult::Fail;
        };

        // TODO: When experimental movement is enabled, it checks if there's a minecart at the
        // location and fails the interaction if there is one.

        if let Err(error) = context.world.try_add_entity(cart) {
            log::warn!("Failed to add minecart {}: {error}", self.entity_type.key);
        }

        context.world.game_event(
            &vanilla_game_events::ENTITY_PLACE,
            pos,
            &GameEventContext::new(
                Some(context.player.as_ref()),
                Some(context.world.get_block_state(pos.below())),
            ),
        );

        let used_item = context.inv.with_item(|stack| {
            let item = stack.item();
            stack.shrink_one();
            item
        });

        context
            .player
            .award_stat(&vanilla_stat_types::ITEM_USED, used_item);

        InteractionResult::Success
    }
}
