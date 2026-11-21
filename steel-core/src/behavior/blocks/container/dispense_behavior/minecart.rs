use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entities;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::{DispenseItemBehavior, DispenseResult, nested_default_dispense};
use crate::behavior::blocks::BaseRailBlock;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::entity::entities::objects::vehicles::create_minecart;
use crate::world::World;

const HORIZONTAL_SPAWN_DISTANCE: f64 = 1.125;
const FLAT_RAIL_Y_OFFSET: f64 = 0.1;
const FLAT_RAIL_BELOW_Y_OFFSET: f64 = -0.9;
const SLOPED_RAIL_RISE: f64 = 0.5;

pub(super) struct MinecartDispenseBehavior;

impl DispenseItemBehavior for MinecartDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult {
        let facing = state.get_value(FACING);
        let front = pos.relative(facing);
        let front_state = world.get_block_state(front);

        let (rail, flat_y_offset, rise_applies) =
            if front_state.get_block().has_tag(&BlockTag::RAILS) {
                (front_state, FLAT_RAIL_Y_OFFSET, true)
            } else {
                if !front_state.is_air() {
                    return nested_default_dispense(world, pos, state, item);
                }

                let below_state = world.get_block_state(front.below());
                if !below_state.get_block().has_tag(&BlockTag::RAILS) {
                    return nested_default_dispense(world, pos, state, item);
                }

                (
                    below_state,
                    FLAT_RAIL_BELOW_Y_OFFSET,
                    facing != Direction::Down,
                )
            };

        let rail_y_offset = if rise_applies && BaseRailBlock::is_slope(rail) {
            flat_y_offset + SLOPED_RAIL_RISE
        } else {
            flat_y_offset
        };

        let center = pos.get_center();
        let direction = facing.offset_vec().as_dvec3();
        let position = DVec3::new(
            center.x + direction.x * HORIZONTAL_SPAWN_DISTANCE,
            center.y.floor() + direction.y + rail_y_offset,
            center.z + direction.z * HORIZONTAL_SPAWN_DISTANCE,
        );

        match create_minecart(world, position, &vanilla_entities::MINECART, item, None) {
            Ok(cart) => {
                if let Err(error) = world.try_add_entity(cart) {
                    log::warn!("Failed to add minecart dispensed at {pos:?}: {error}");
                }

                item.shrink_one();
            }
            Err(error) => log::warn!("Failed to create minecart dispensed at {pos:?}: {error:?}"),
        }

        DispenseResult::Success
    }
}
