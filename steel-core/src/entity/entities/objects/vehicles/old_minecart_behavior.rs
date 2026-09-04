//! Classic minecart physics and rail snapping implementation.
//! Mirrors vanilla `OldMinecartBehavior`.

use glam::DVec3;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{vanilla_blocks, vanilla_entities};
use steel_utils::BlockPos;

use super::minecart_behavior::MinecartBehavior;
use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::Entity;
use crate::physics::MoverType;
use crate::world::World;

const fn rail_exits(shape: RailShape) -> (DVec3, DVec3) {
    match shape {
        RailShape::NorthSouth => (DVec3::new(0.0, 0.0, -1.0), DVec3::new(0.0, 0.0, 1.0)),
        RailShape::EastWest => (DVec3::new(-1.0, 0.0, 0.0), DVec3::new(1.0, 0.0, 0.0)),
        RailShape::AscendingEast => (DVec3::new(-1.0, -1.0, 0.0), DVec3::new(1.0, 0.0, 0.0)),
        RailShape::AscendingWest => (DVec3::new(-1.0, 0.0, 0.0), DVec3::new(1.0, -1.0, 0.0)),
        RailShape::AscendingNorth => (DVec3::new(0.0, -1.0, -1.0), DVec3::new(0.0, 0.0, 1.0)),
        RailShape::AscendingSouth => (DVec3::new(0.0, 0.0, -1.0), DVec3::new(0.0, -1.0, 1.0)),
        RailShape::SouthEast => (DVec3::new(0.0, 0.0, 1.0), DVec3::new(1.0, 0.0, 0.0)),
        RailShape::SouthWest => (DVec3::new(0.0, 0.0, 1.0), DVec3::new(-1.0, 0.0, 0.0)),
        RailShape::NorthWest => (DVec3::new(0.0, 0.0, -1.0), DVec3::new(-1.0, 0.0, 0.0)),
        RailShape::NorthEast => (DVec3::new(0.0, 0.0, -1.0), DVec3::new(1.0, 0.0, 0.0)),
    }
}

/// Classic minecart physics logic mirroring vanilla `OldMinecartBehavior.java`.
pub struct OldMinecartBehavior {
    flipped: bool,
}

impl OldMinecartBehavior {
    /// Creates a new `OldMinecartBehavior`.
    #[must_use]
    pub const fn new() -> Self {
        Self { flipped: false }
    }
}

impl Default for OldMinecartBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl MinecartBehavior for OldMinecartBehavior {
    #[expect(
        clippy::too_many_lines,
        reason = "comprehensive 1:1 classic rail physics simulation tick"
    )]
    fn tick(&mut self, cart: &dyn Entity, world: &Arc<World>) {
        let start_pos = cart.position();
        let current_yaw = cart.rotation().0;

        cart.apply_gravity();
        let pos = cart.position();
        let current_block_pos = BlockPos::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        );
        let current_block_state = world.get_block_state(current_block_pos);
        let below_block_pos = current_block_pos.below();
        let below_block_state = world.get_block_state(below_block_pos);

        let (block_pos, block_state) = if current_block_state.get_block().has_tag(&BlockTag::RAILS)
        {
            (current_block_pos, current_block_state)
        } else if below_block_state.get_block().has_tag(&BlockTag::RAILS) {
            (below_block_pos, below_block_state)
        } else {
            (current_block_pos, current_block_state)
        };

        let block = block_state.get_block();
        let is_rail =
            block.has_tag(&BlockTag::RAILS) && BLOCK_BEHAVIORS.get_behavior(block).is_rail();
        cart.set_on_rails(is_rail);

        if is_rail {
            let shape = block_state
                .try_get_value(&BlockStateProperties::RAIL_SHAPE)
                .unwrap_or(RailShape::NorthSouth);

            let mut y = f64::from(block_pos.y());
            let slide_speed = if cart.is_in_water() {
                0.007_812_5 * 0.2
            } else {
                0.007_812_5
            };

            let mut movement = cart.velocity();
            match shape {
                RailShape::AscendingEast => {
                    movement.x -= slide_speed;
                    y += 1.0;
                }
                RailShape::AscendingWest => {
                    movement.x += slide_speed;
                    y += 1.0;
                }
                RailShape::AscendingNorth => {
                    movement.z += slide_speed;
                    y += 1.0;
                }
                RailShape::AscendingSouth => {
                    movement.z -= slide_speed;
                    y += 1.0;
                }
                _ => {}
            }

            let (exit0, exit1) = rail_exits(shape);
            let mut x_d = exit1.x - exit0.x;
            let mut z_d = exit1.z - exit0.z;
            let length = (x_d * x_d + z_d * z_d).sqrt();
            let flip = movement.x * x_d + movement.z * z_d;
            if flip < 0.0 {
                x_d = -x_d;
                z_d = -z_d;
            }

            let horizontal_speed = (movement.x * movement.x + movement.z * movement.z).sqrt();
            let pow = horizontal_speed.min(2.0);
            if length > 0.0 {
                movement = DVec3::new(pow * x_d / length, movement.y, pow * z_d / length);
            }

            let is_powered_rail = block_state.get_block() == &vanilla_blocks::POWERED_RAIL;
            let mut halt_track = false;
            if is_powered_rail {
                let powered = block_state
                    .try_get_value(&BlockStateProperties::POWERED)
                    .unwrap_or(false);
                if powered {
                    let speed = (movement.x * movement.x + movement.z * movement.z).sqrt();
                    if speed > 0.01 {
                        movement.x += (movement.x / speed) * 0.06;
                        movement.z += (movement.z / speed) * 0.06;
                    } else if shape == RailShape::EastWest {
                        let is_west_solid = world.get_block_state(block_pos.west()).is_solid();
                        let is_east_solid = world.get_block_state(block_pos.east()).is_solid();
                        if is_west_solid && !is_east_solid {
                            movement.x = 0.02;
                        } else if is_east_solid && !is_west_solid {
                            movement.x = -0.02;
                        }
                    } else if shape == RailShape::NorthSouth {
                        let is_north_solid = world.get_block_state(block_pos.north()).is_solid();
                        let is_south_solid = world.get_block_state(block_pos.south()).is_solid();
                        if is_north_solid && !is_south_solid {
                            movement.z = 0.02;
                        } else if is_south_solid && !is_north_solid {
                            movement.z = -0.02;
                        }
                    }
                } else {
                    halt_track = true;
                }
            }

            if halt_track {
                let speed = (movement.x * movement.x + movement.z * movement.z).sqrt();
                if speed < 0.03 {
                    movement = DVec3::ZERO;
                } else {
                    movement.x *= 0.5;
                    movement.y = 0.0;
                    movement.z *= 0.5;
                }
            }

            let x0 = f64::from(block_pos.x()) + 0.5 + exit0.x * 0.5;
            let z0 = f64::from(block_pos.z()) + 0.5 + exit0.z * 0.5;
            let x1 = f64::from(block_pos.x()) + 0.5 + exit1.x * 0.5;
            let z1 = f64::from(block_pos.z()) + 0.5 + exit1.z * 0.5;
            x_d = x1 - x0;
            z_d = z1 - z0;

            let progress = if x_d == 0.0 {
                pos.z - f64::from(block_pos.z())
            } else if z_d == 0.0 {
                pos.x - f64::from(block_pos.x())
            } else {
                let xx = pos.x - x0;
                let zz = pos.z - z0;
                (xx * x_d + zz * z_d) * 2.0
            };

            let snapped_x = x0 + x_d * progress;
            let snapped_z = z0 + z_d * progress;
            let _ = cart.try_set_position(DVec3::new(snapped_x, y + 0.0625, snapped_z));

            let scale = if cart.is_vehicle() { 0.75 } else { 1.0 };
            let max_speed = 0.4;
            let move_vec = DVec3::new(
                (scale * movement.x).clamp(-max_speed, max_speed),
                0.0,
                (scale * movement.z).clamp(-max_speed, max_speed),
            );
            cart.set_velocity(movement);
            cart.mark_velocity_sync();
            let _ = cart.move_entity(MoverType::SelfMovement, move_vec);

            if exit0.y != 0.0
                && (cart.position().x.floor() as i32) - block_pos.x() == (exit0.x as i32)
                && (cart.position().z.floor() as i32) - block_pos.z() == (exit0.z as i32)
            {
                let cur = cart.position();
                let _ = cart.try_set_position(DVec3::new(cur.x, cur.y + exit0.y, cur.z));
            } else if exit1.y != 0.0
                && (cart.position().x.floor() as i32) - block_pos.x() == (exit1.x as i32)
                && (cart.position().z.floor() as i32) - block_pos.z() == (exit1.z as i32)
            {
                let cur = cart.position();
                let _ = cart.try_set_position(DVec3::new(cur.x, cur.y + exit1.y, cur.z));
            }

            // Natural slowdown
            let slowdown = if cart.is_vehicle() { 0.997 } else { 0.96 };
            let mut final_vel = cart.velocity();
            final_vel.x *= slowdown;
            final_vel.z *= slowdown;
            cart.set_velocity(final_vel);
            cart.mark_velocity_sync();
        } else {
            let max_speed = if cart.is_in_water() { 0.2 } else { 0.4 };
            let mut vel = cart.velocity();
            vel.x = vel.x.clamp(-max_speed, max_speed);
            vel.z = vel.z.clamp(-max_speed, max_speed);
            if cart.on_ground() {
                vel *= 0.5;
            }

            cart.set_velocity(vel);
            cart.mark_velocity_sync();
            let _ = cart.move_entity(MoverType::SelfMovement, vel);

            if !cart.on_ground() {
                let post_vel = cart.velocity() * 0.95;
                cart.set_velocity(post_vel);
                cart.mark_velocity_sync();
            }
        }

        let cur_pos = cart.position();
        let x_diff = start_pos.x - cur_pos.x;
        let z_diff = start_pos.z - cur_pos.z;
        if x_diff * x_diff + z_diff * z_diff > 0.001 {
            let mut y_rot = z_diff.atan2(x_diff).to_degrees() as f32;
            if self.flipped {
                y_rot += 180.0;
            }
            let mut rot_diff = (y_rot - current_yaw) % 360.0;
            if rot_diff < -180.0 {
                rot_diff += 360.0;
            }
            if rot_diff >= 180.0 {
                rot_diff -= 360.0;
            }
            if rot_diff < -170.0 || rot_diff >= 170.0 {
                y_rot += 180.0;
                self.flipped = !self.flipped;
            }
            cart.set_rotation((y_rot % 360.0, 0.0));
            cart.mark_velocity_sync();
        }

        let hitbox = cart.bounding_box().inflate_xyz(0.2, 0.0, 0.2);
        let speed_sq =
            cart.velocity().x * cart.velocity().x + cart.velocity().z * cart.velocity().z;
        let is_rideable = cart.entity_type() == &vanilla_entities::MINECART;

        if is_rideable && speed_sq >= 0.01 {
            let pushable_entities = world.get_pushable_entities(cart, &hitbox);
            for entity in pushable_entities {
                if entity.as_living_entity().is_some()
                    && entity.entity_type() != &vanilla_entities::PLAYER
                    && entity.entity_type() != &vanilla_entities::IRON_GOLEM
                    && !cart.is_vehicle()
                    && !entity.is_passenger()
                    && let Some(cart_shared) = world.get_entity_by_id(cart.id())
                {
                    let _ = entity.start_riding(&cart_shared);
                } else {
                    entity.push_entity(cart);
                }
            }
        } else {
            let entities = world.get_pushable_entities(cart, &hitbox);
            for entity in entities {
                if !cart.has_passenger(entity.as_ref())
                    && entity.is_pushable()
                    && entity.entity_type().is_abstract_minecart
                {
                    cart.push_entity(entity.as_ref());
                }
            }
        }

        if cart.is_vehicle() {
            for passenger in cart.passengers() {
                cart.position_rider(passenger.as_ref());
            }
        }
    }
}
