use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::{DispenseItemBehavior, DispenseResult};
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::entity::projectile::triangle_random;
use crate::world::World;

pub(super) const NOZZLE_DISTANCE: f64 = 0.7;
const HORIZONTAL_EJECTION_Y_OFFSET: f64 = 0.15625;
const VERTICAL_EJECTION_Y_OFFSET: f64 = 0.125;
const EJECTION_ACCURACY: f64 = 6.0;
const EJECTION_SPREAD: f64 = 0.017_227_5 * EJECTION_ACCURACY;
const EJECTION_BASE_SPEED: f64 = 0.2;
const EJECTION_SPEED_VARIATION: f64 = 0.1;

/// returns the shared spawn origin for items and projectiles before their y offsets
pub(super) fn nozzle_position(pos: BlockPos, facing: Direction) -> DVec3 {
    pos.get_center() + facing.offset_vec().as_dvec3() * NOZZLE_DISTANCE
}

/// eject a item with minecraft axis dependent spawn offset and trig spread
pub(crate) fn eject_item(world: &Arc<World>, pos: BlockPos, facing: Direction, item: ItemStack) {
    let mut spawn = nozzle_position(pos, facing);
    spawn.y -= match facing {
        Direction::Up | Direction::Down => VERTICAL_EJECTION_Y_OFFSET,
        _ => HORIZONTAL_EJECTION_Y_OFFSET,
    };

    let direction = facing.offset_vec().as_dvec3();
    let speed = EJECTION_BASE_SPEED + rand::random::<f64>() * EJECTION_SPEED_VARIATION;
    let velocity = DVec3::new(
        triangle_random(direction.x * speed, EJECTION_SPREAD),
        triangle_random(EJECTION_BASE_SPEED, EJECTION_SPREAD),
        triangle_random(direction.z * speed, EJECTION_SPREAD),
    );

    world.spawn_item_with_velocity(spawn, item, velocity);
}

pub struct DefaultDispenseBehavior;

impl DispenseItemBehavior for DefaultDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult {
        eject_item(world, pos, state.get_value(FACING), item.split(1));
        DispenseResult::Success
    }
}
