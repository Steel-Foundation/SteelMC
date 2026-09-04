//! Minecart item behavior (`MinecartItem`).

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_events;

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::{ENTITIES, Entity as _, next_entity_id};
use crate::world::game_event::GameEventContext;

/// Behavior for placing minecart items on rails.
#[item_behavior(class = "MinecartItem")]
pub struct MinecartItem {
    #[json_arg(vanilla_entities, json = "type")]
    entity_type: EntityTypeRef,
}

impl MinecartItem {
    /// Creates a minecart item behavior for the given vehicle entity type.
    #[must_use]
    pub const fn new(entity_type: EntityTypeRef) -> Self {
        Self { entity_type }
    }
}

impl ItemBehavior for MinecartItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let block_pos = context.hit_result.block_pos;
        let block_state = context.world.get_block_state(block_pos);
        if !block_state.get_block().has_tag(&BlockTag::RAILS) {
            return InteractionResult::Fail;
        }

        let mut y_offset = 0.0625;
        if let Some(rail_shape) = block_state.try_get_value(&BlockStateProperties::RAIL_SHAPE)
            && rail_shape.is_slope()
        {
            y_offset += 0.5;
        }

        let spawn_pos = DVec3::new(
            f64::from(block_pos.x()) + 0.5,
            f64::from(block_pos.y()) + y_offset,
            f64::from(block_pos.z()) + 0.5,
        );

        let Some(entity) = ENTITIES.create(
            self.entity_type,
            next_entity_id(),
            spawn_pos,
            Arc::downgrade(context.world),
        ) else {
            return InteractionResult::Fail;
        };

        let player_yaw = context.player.rotation().0;
        let initial_yaw = if let Some(rail_shape) =
            block_state.try_get_value(&BlockStateProperties::RAIL_SHAPE)
        {
            match rail_shape {
                RailShape::EastWest | RailShape::AscendingEast | RailShape::AscendingWest => {
                    let wrapped = (player_yaw % 360.0 + 360.0) % 360.0;
                    if (180.0..360.0).contains(&wrapped) {
                        180.0
                    } else {
                        0.0
                    }
                }
                RailShape::NorthSouth | RailShape::AscendingNorth | RailShape::AscendingSouth => {
                    let wrapped = (player_yaw % 360.0 + 360.0) % 360.0;
                    if (90.0..270.0).contains(&wrapped) {
                        90.0
                    } else {
                        270.0
                    }
                }
                RailShape::SouthEast => {
                    let wrapped = (player_yaw % 360.0 + 360.0) % 360.0;
                    if (135.0..315.0).contains(&wrapped) {
                        180.0
                    } else {
                        270.0
                    }
                }
                RailShape::SouthWest => {
                    let wrapped = (player_yaw % 360.0 + 360.0) % 360.0;
                    if (45.0..225.0).contains(&wrapped) {
                        0.0
                    } else {
                        270.0
                    }
                }
                RailShape::NorthWest => {
                    let wrapped = (player_yaw % 360.0 + 360.0) % 360.0;
                    if (0.0..135.0).contains(&wrapped) || wrapped >= 315.0 {
                        0.0
                    } else {
                        90.0
                    }
                }
                RailShape::NorthEast => {
                    let wrapped = (player_yaw % 360.0 + 360.0) % 360.0;
                    if (0.0..225.0).contains(&wrapped) {
                        90.0
                    } else {
                        180.0
                    }
                }
            }
        } else {
            player_yaw
        };
        entity.set_rotation((initial_yaw, 0.0));

        if context.world.try_add_entity(entity).is_err() {
            return InteractionResult::Fail;
        }

        context.world.game_event(
            &vanilla_game_events::ENTITY_PLACE,
            block_pos,
            &GameEventContext::new(Some(context.player), None),
        );

        if !context.player.has_infinite_materials() {
            context.inv.with_item(|item| item.shrink(1));
        }

        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities, vanilla_items};
    use steel_utils::types::{InteractionHand, UpdateFlags};
    use steel_utils::{BlockPos, ChunkPos, Direction};

    use crate::behavior::{BlockHitResult, init_behaviors};
    use crate::entity::init_entities;
    use crate::inventory::container::Container;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn placing_minecart_on_flat_rail_spawns_entity_and_consumes_item() {
        init_vanilla_registry();
        init_behaviors();
        init_entities();

        let world = fresh_test_world("minecart_place_flat");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        world.set_block(
            pos,
            vanilla_blocks::RAIL.default_state(),
            UpdateFlags::UPDATE_NONE,
        );

        let player = TestPlayerBuilder::new(Arc::clone(&world), "Steve", 1).build();
        {
            let mut inv = player.inventory.lock();
            inv.set_item(0, ItemStack::with_count(&vanilla_items::MINECART, 2));
        }

        let hit_result = BlockHitResult {
            location: DVec3::new(0.5, 64.5, 0.5),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };

        let behavior = MinecartItem::new(&vanilla_entities::MINECART);
        let mut context = UseOnContext::new(
            &player,
            InteractionHand::MainHand,
            hit_result,
            &world,
            player.inventory.clone(),
        );

        let result = behavior.use_on(&mut context);
        assert_eq!(result, InteractionResult::Success);

        let count = player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .count();
        assert_eq!(count, 1);

        let entities = world.get_entities_in_aabb_matching(
            &steel_utils::WorldAabb::new(0.0, 64.0, 0.0, 1.0, 65.0, 1.0),
            |e| e.entity_type() == &vanilla_entities::MINECART,
        );
        assert_eq!(entities.len(), 1);
        let cart = &entities[0];
        assert_eq!(cart.position(), DVec3::new(0.5, 64.0625, 0.5));
    }

    #[test]
    fn placing_minecart_on_ascending_rail_offsets_y_position() {
        init_vanilla_registry();
        init_behaviors();
        init_entities();

        let world = fresh_test_world("minecart_place_ascending");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let rail_state = vanilla_blocks::RAIL
            .default_state()
            .set_value(&BlockStateProperties::RAIL_SHAPE, RailShape::AscendingNorth);
        world.set_block(pos, rail_state, UpdateFlags::UPDATE_NONE);

        let player = TestPlayerBuilder::new(Arc::clone(&world), "Steve", 1).build();
        {
            let mut inv = player.inventory.lock();
            inv.set_item(0, ItemStack::with_count(&vanilla_items::MINECART, 1));
        }

        let hit_result = BlockHitResult {
            location: DVec3::new(0.5, 64.5, 0.5),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };

        let behavior = MinecartItem::new(&vanilla_entities::MINECART);
        let mut context = UseOnContext::new(
            &player,
            InteractionHand::MainHand,
            hit_result,
            &world,
            player.inventory.clone(),
        );

        let result = behavior.use_on(&mut context);
        assert_eq!(result, InteractionResult::Success);

        let entities = world.get_entities_in_aabb_matching(
            &steel_utils::WorldAabb::new(0.0, 64.0, 0.0, 1.0, 66.0, 1.0),
            |e| e.entity_type() == &vanilla_entities::MINECART,
        );
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].position(), DVec3::new(0.5, 64.5625, 0.5));
    }

    #[test]
    fn placing_minecart_on_non_rail_fails() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("minecart_place_non_rail");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        world.set_block(
            pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        );

        let player = TestPlayerBuilder::new(Arc::clone(&world), "Steve", 1).build();
        {
            let mut inv = player.inventory.lock();
            inv.set_item(0, ItemStack::with_count(&vanilla_items::MINECART, 1));
        }

        let hit_result = BlockHitResult {
            location: DVec3::new(0.5, 64.5, 0.5),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };

        let behavior = MinecartItem::new(&vanilla_entities::MINECART);
        let mut context = UseOnContext::new(
            &player,
            InteractionHand::MainHand,
            hit_result,
            &world,
            player.inventory.clone(),
        );

        let result = behavior.use_on(&mut context);
        assert_eq!(result, InteractionResult::Fail);

        let count = player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .count();
        assert_eq!(count, 1);
    }
}
