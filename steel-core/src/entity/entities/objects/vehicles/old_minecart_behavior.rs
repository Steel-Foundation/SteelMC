//! Old minecart movement on rails, off-track physics, and entity collisions.

use std::f64::consts::PI;
use std::sync::Arc;

use glam::DVec3;
use glam::Vec3Swizzles;
use steel_math::{DEGREE_180, DEGREE_360, trig::atan2, wrap_degrees};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, EnumProperty, RailShape,
};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entities;
use steel_utils::{BlockPos, Direction};

use super::abstract_minecart::{AbstractMinecart, rail_exits};
use super::minecart_behavior::MinecartBehavior;
use crate::behavior::blocks::BaseRailBlock;
use crate::entity::SharedEntity;
use crate::physics::MoverType;
use crate::world::World;

const RAIL_SHAPE: &EnumProperty<RailShape> = &BlockStateProperties::RAIL_SHAPE;
const POWERED: &BoolProperty = &BlockStateProperties::POWERED;

const RAIL_HEIGHT_OFFSET: f64 = 0.0625;
const SLIDE_SPEED: f64 = 0.007_812_5;
const WATER_SLIDE_SPEED: f64 = SLIDE_SPEED * 0.2;
const MAX_SPEED_IN_WATER: f64 = 0.2;
const MAX_SPEED_ON_LAND: f64 = 0.4;
const ABSOLUTE_MAX_SPEED: f64 = 0.4;
const POWERED_RAIL_ACCEL: f64 = 0.06;
const POWERED_RAIL_KICKSTART: f64 = 0.02;
const RIDDEN_SLOWDOWN_FACTOR: f64 = 0.997;
const EMPTY_SLOWDOWN_FACTOR: f64 = 0.96;
const OCCUPIED_MOVEMENT_SCALE: f64 = 0.75;
const PLAYER_NUDGE_ACCELERATION: f64 = 0.001;
const PLAYER_NUDGE_MAX_SPEED_SQUARED: f64 = 0.01;
const MINECART_RIDABLE_THRESHOLD: f64 = 0.01;
const MIN_ROTATION_DISTANCE_SQUARED: f64 = 0.001;
const ENTITY_INTERACTION_INFLATION: f64 = 0.2_f32 as f64;

#[derive(Debug, Default)]
pub struct OldMinecartBehavior;

impl MinecartBehavior for OldMinecartBehavior {
    fn tick(&self, minecart: &dyn AbstractMinecart, entity: &SharedEntity, world: &World) {
        minecart.apply_gravity();
        let pos = minecart.current_block_pos_or_rail_below(world);
        let block_state = world.get_block_state(pos);
        let on_rails = BaseRailBlock::is_rail_state(block_state);
        minecart.minecart_base().set_on_rails(on_rails);

        if on_rails {
            self.move_along_track(minecart, entity, world);
            if block_state.get_block() == &vanilla_blocks::ACTIVATOR_RAIL {
                minecart.activate_minecart(world, pos, block_state.get_value(POWERED));
            }
        } else {
            minecart.come_off_track(entity, world);
        }

        minecart.apply_effects_from_blocks();
        Self::update_rotation_from_movement(minecart);
        self.push_and_pickup_entities(minecart);
    }

    fn push_and_pickup_entities(&self, minecart: &dyn AbstractMinecart) -> bool {
        let Some(world) = minecart.level() else {
            return false;
        };
        let Some(minecart_shared) = world.get_entity_by_id(minecart.id()) else {
            return false;
        };

        let hitbox = minecart.bounding_box().inflate_xyz(
            ENTITY_INTERACTION_INFLATION,
            0.0,
            ENTITY_INTERACTION_INFLATION,
        );

        if minecart.is_rideable()
            && minecart.velocity().xz().length_squared() >= MINECART_RIDABLE_THRESHOLD
        {
            let entities = world.get_pushable_entities(minecart.as_entity_event_source(), &hitbox);

            for entity in entities {
                let is_player = entity.entity_type() == &vanilla_entities::PLAYER;
                let is_iron_golem = entity.entity_type() == &vanilla_entities::IRON_GOLEM;
                let is_minecart = entity.entity_type().is_abstract_minecart;

                if !is_player
                    && !is_iron_golem
                    && !is_minecart
                    && !minecart.is_vehicle()
                    && !entity.is_passenger()
                {
                    entity.start_riding(&minecart_shared);
                } else {
                    entity.push_entity(minecart.as_entity_event_source());
                }
            }
        } else {
            let entities =
                world.get_entities_in_aabb_matching(&hitbox, |entity| entity.id() != minecart.id());

            for entity in entities {
                if !minecart.has_passenger(entity.as_ref())
                    && entity.is_pushable()
                    && entity.entity_type().is_abstract_minecart
                {
                    entity.push_entity(minecart.as_entity_event_source());
                }
            }
        }

        false
    }

    fn known_movement(&self, known_movement: DVec3) -> DVec3 {
        if known_movement.x.is_nan() || known_movement.y.is_nan() || known_movement.z.is_nan() {
            return DVec3::ZERO;
        }

        DVec3::new(
            known_movement
                .x
                .clamp(-ABSOLUTE_MAX_SPEED, ABSOLUTE_MAX_SPEED),
            known_movement.y,
            known_movement
                .z
                .clamp(-ABSOLUTE_MAX_SPEED, ABSOLUTE_MAX_SPEED),
        )
    }

    fn motion_direction(&self, minecart: &dyn AbstractMinecart) -> Direction {
        let direction = minecart.direction_yaw();

        if minecart.is_flipped() {
            direction.opposite().rotate_y_clockwise()
        } else {
            direction.rotate_y_clockwise()
        }
    }

    fn max_speed(&self, minecart: &dyn AbstractMinecart, _world: &World) -> f64 {
        if minecart.is_in_water() {
            MAX_SPEED_IN_WATER
        } else {
            MAX_SPEED_ON_LAND
        }
    }

    fn slowdown_factor(&self, minecart: &dyn AbstractMinecart) -> f64 {
        if minecart.base().is_vehicle() {
            RIDDEN_SLOWDOWN_FACTOR
        } else {
            EMPTY_SLOWDOWN_FACTOR
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "keeping it together makes it easier to audit"
    )]
    fn move_along_track(
        &self,
        minecart: &dyn AbstractMinecart,
        entity: &SharedEntity,
        world: &World,
    ) {
        let pos = minecart.current_block_pos_or_rail_below(world);
        let state = world.get_block_state(pos);
        minecart.reset_fall_distance();

        let position = minecart.position();
        let old_projected = Self::get_pos(world, position.x, position.y, position.z);
        let mut y = f64::from(pos.y());

        let block = state.get_block();
        let (power_track, mut halt_track) = if block == &vanilla_blocks::POWERED_RAIL {
            let powered = state.get_value(POWERED);
            (powered, !powered)
        } else {
            (false, false)
        };

        let slide_speed = if minecart.is_in_water() {
            WATER_SLIDE_SPEED
        } else {
            SLIDE_SPEED
        };
        let shape = state.get_value(RAIL_SHAPE);
        let mut velocity = minecart.velocity();
        match shape {
            RailShape::AscendingEast => velocity.x -= slide_speed,
            RailShape::AscendingWest => velocity.x += slide_speed,
            RailShape::AscendingNorth => velocity.z += slide_speed,
            RailShape::AscendingSouth => velocity.z -= slide_speed,
            _ => {}
        }
        if matches!(
            shape,
            RailShape::AscendingEast
                | RailShape::AscendingWest
                | RailShape::AscendingNorth
                | RailShape::AscendingSouth
        ) {
            y += 1.0;
        }
        minecart.set_velocity(velocity);

        let velocity = minecart.velocity();
        let ((ex0, ey0, ez0), (ex1, ey1, ez1)) = rail_exits(shape);
        let (mut xd, mut zd) = (f64::from(ex1 - ex0), f64::from(ez1 - ez0));
        let length = (xd * xd + zd * zd).sqrt();
        if velocity.x * xd + velocity.z * zd < 0.0 {
            xd = -xd;
            zd = -zd;
        }
        let pow = velocity.xz().length().min(2.0);
        minecart.set_velocity(DVec3::new(pow * xd / length, velocity.y, pow * zd / length));

        if let Some(passenger) = minecart.first_passenger()
            && let Some(player) = passenger.as_player()
        {
            let move_intent = player.last_client_move_intent();

            if move_intent.length_squared() > 0.0 {
                let rider_movement = move_intent.normalize();
                let velocity = minecart.velocity();
                let own_distance_squared = velocity.xz().length_squared();

                if rider_movement.length_squared() > 0.0
                    && own_distance_squared < PLAYER_NUDGE_MAX_SPEED_SQUARED
                {
                    minecart.set_velocity(
                        velocity
                            + DVec3::new(
                                move_intent.x * PLAYER_NUDGE_ACCELERATION,
                                0.0,
                                move_intent.z * PLAYER_NUDGE_ACCELERATION,
                            ),
                    );
                    halt_track = false;
                }
            }
        }

        if halt_track {
            let velocity = minecart.velocity();
            let speed_length = velocity.xz().length();
            let velocity = if speed_length < 0.03 {
                DVec3::ZERO
            } else {
                velocity * DVec3::new(0.5, 0.0, 0.5)
            };
            minecart.set_velocity(velocity);
        }

        let (px, pz) = (f64::from(pos.x()), f64::from(pos.z()));
        let (x0, z0) = (
            px + 0.5 + f64::from(ex0) * 0.5,
            pz + 0.5 + f64::from(ez0) * 0.5,
        );
        let (x1, z1) = (
            px + 0.5 + f64::from(ex1) * 0.5,
            pz + 0.5 + f64::from(ez1) * 0.5,
        );
        let (xd, zd) = (x1 - x0, z1 - z0);
        let (x, z) = (position.x, position.z);
        let progress = if xd == 0.0 {
            z - pz
        } else if zd == 0.0 {
            x - px
        } else {
            ((x - x0) * xd + (z - z0) * zd) * 2.0
        };
        if let Err(error) =
            minecart.try_set_position(DVec3::new(x0 + xd * progress, y, z0 + zd * progress))
        {
            log::debug!(
                "failed to project minecart {} onto rail: {error}",
                minecart.id()
            );
            return;
        }

        let scale = if minecart.base().is_vehicle() {
            OCCUPIED_MOVEMENT_SCALE
        } else {
            1.0
        };
        let max_speed = minecart.max_speed(world);
        let velocity = minecart.velocity();
        let clamped = DVec3::new(
            (scale * velocity.x).clamp(-max_speed, max_speed),
            0.0,
            (scale * velocity.z).clamp(-max_speed, max_speed),
        );
        if Arc::clone(entity)
            .move_entity(MoverType::SelfMovement, clamped)
            .is_none()
        {
            return;
        }

        let position = minecart.position();
        let (floor_x, floor_z) = (position.x.floor() as i32, position.z.floor() as i32);
        if ey0 != 0 && floor_x - pos.x() == ex0 && floor_z - pos.z() == ez0 {
            let position = minecart.position();
            if let Err(error) = minecart.try_set_position(DVec3::new(
                position.x,
                position.y + f64::from(ey0),
                position.z,
            )) {
                log::debug!(
                    "failed to adjust minecart {} to rail exit height: {error}",
                    minecart.id()
                );
                return;
            }
        } else if ey1 != 0 && floor_x - pos.x() == ex1 && floor_z - pos.z() == ez1 {
            let position = minecart.position();
            if let Err(error) = minecart.try_set_position(DVec3::new(
                position.x,
                position.y + f64::from(ey1),
                position.z,
            )) {
                log::debug!(
                    "failed to adjust minecart {} to rail exit height: {error}",
                    minecart.id()
                );
                return;
            }
        }

        minecart.set_velocity(minecart.apply_natural_slowdown(minecart.velocity()));

        let position = minecart.position();
        if let (Some(new_projected), Some(old_projected)) = (
            Self::get_pos(world, position.x, position.y, position.z),
            old_projected,
        ) {
            let vertical_speed = (old_projected.y - new_projected.y) * 0.05;
            let velocity = minecart.velocity();
            let horizontal = velocity.xz().length();
            if horizontal > 0.0 {
                let factor = (horizontal + vertical_speed) / horizontal;
                minecart.set_velocity(DVec3::new(
                    velocity.x * factor,
                    velocity.y,
                    velocity.z * factor,
                ));
            }
            let position = minecart.position();
            if let Err(error) =
                minecart.try_set_position(DVec3::new(position.x, new_projected.y, position.z))
            {
                log::debug!(
                    "failed to correct minecart {} rail height: {error}",
                    minecart.id()
                );
                return;
            }
        }

        let position = minecart.position();
        let (xn, zn) = (position.x.floor() as i32, position.z.floor() as i32);
        if xn != pos.x() || zn != pos.z() {
            let velocity = minecart.velocity();
            let horizontal = velocity.xz().length();
            minecart.set_velocity(DVec3::new(
                horizontal * f64::from(xn - pos.x()),
                velocity.y,
                horizontal * f64::from(zn - pos.z()),
            ));
        }

        if power_track {
            let velocity = minecart.velocity();
            let speed_length = velocity.xz().length();
            if speed_length > 0.01 {
                minecart.set_velocity(
                    velocity
                        + DVec3::new(
                            velocity.x / speed_length * POWERED_RAIL_ACCEL,
                            0.0,
                            velocity.z / speed_length * POWERED_RAIL_ACCEL,
                        ),
                );
            } else {
                let (mut dx, mut dz) = (velocity.x, velocity.z);
                match shape {
                    RailShape::EastWest => {
                        if minecart.is_redstone_conductor(world, pos.relative(Direction::West)) {
                            dx = POWERED_RAIL_KICKSTART;
                        } else if minecart
                            .is_redstone_conductor(world, pos.relative(Direction::East))
                        {
                            dx = -POWERED_RAIL_KICKSTART;
                        }
                    }
                    RailShape::NorthSouth => {
                        if minecart.is_redstone_conductor(world, pos.relative(Direction::North)) {
                            dz = POWERED_RAIL_KICKSTART;
                        } else if minecart
                            .is_redstone_conductor(world, pos.relative(Direction::South))
                        {
                            dz = -POWERED_RAIL_KICKSTART;
                        }
                    }
                    _ => return,
                }
                minecart.set_velocity(DVec3::new(dx, velocity.y, dz));
            }
        }
    }
}

impl OldMinecartBehavior {
    fn update_rotation_from_movement(minecart: &dyn AbstractMinecart) {
        minecart.set_rotation((minecart.rotation().0, 0.0));
        let (old_position, position) = (minecart.old_position(), minecart.position());
        let (x_diff, z_diff) = (old_position.x - position.x, old_position.z - position.z);
        if x_diff * x_diff + z_diff * z_diff > MIN_ROTATION_DISTANCE_SQUARED {
            let mut yaw = (atan2(z_diff, x_diff) * 180.0 / PI) as f32;
            if minecart.is_flipped() {
                yaw += DEGREE_180;
            }
            minecart.set_rotation((yaw, minecart.rotation().1));
        }

        let rot_diff = wrap_degrees(minecart.rotation().0 - minecart.base().old_rotation().0);
        if rot_diff < -170.0 || rot_diff >= 170.0 {
            minecart.minecart_base().toggle_flipped();
            minecart.set_rotation((minecart.rotation().0 + DEGREE_180, minecart.rotation().1));
        }
        minecart.set_rotation((
            minecart.rotation().0 % DEGREE_360,
            minecart.rotation().1 % DEGREE_360,
        ));
    }

    /// Returns the minecart's position with the correct rail offset, if there is a rail.
    #[must_use]
    fn get_pos(world: &World, x: f64, y: f64, z: f64) -> Option<DVec3> {
        let (xt, mut yt, zt) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        if world
            .get_block_state(BlockPos::new(xt, yt - 1, zt))
            .get_block()
            .has_tag(&BlockTag::RAILS)
        {
            yt -= 1;
        }
        let state = world.get_block_state(BlockPos::new(xt, yt, zt));
        if !BaseRailBlock::is_rail_state(state) {
            return None;
        }

        let shape = state.get_value(RAIL_SHAPE);
        let ((ex0, ey0, ez0), (ex1, ey1, ez1)) = rail_exits(shape);
        let (x0, y0, z0) = (
            f64::from(xt) + 0.5 + f64::from(ex0) * 0.5,
            f64::from(yt) + RAIL_HEIGHT_OFFSET + f64::from(ey0) * 0.5,
            f64::from(zt) + 0.5 + f64::from(ez0) * 0.5,
        );
        let (x1, y1, z1) = (
            f64::from(xt) + 0.5 + f64::from(ex1) * 0.5,
            f64::from(yt) + RAIL_HEIGHT_OFFSET + f64::from(ey1) * 0.5,
            f64::from(zt) + 0.5 + f64::from(ez1) * 0.5,
        );
        let (dx, dz) = (x1 - x0, z1 - z0);
        let dy = (y1 - y0) * 2.0;

        let progress = if dx == 0.0 {
            z - f64::from(zt)
        } else if dz == 0.0 {
            x - f64::from(xt)
        } else {
            ((x - x0) * dx + (z - z0) * dz) * 2.0
        };

        let mut projected = DVec3::new(x0 + dx * progress, y0 + dy * progress, z0 + dz * progress);
        if dy < 0.0 {
            projected.y += 1.0;
        } else if dy > 0.0 {
            projected.y += 0.5;
        }
        Some(projected)
    }
}
