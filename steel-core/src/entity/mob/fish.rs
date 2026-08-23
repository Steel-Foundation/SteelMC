use std::sync::OnceLock;

use glam::DVec3;
use steel_registry::vanilla_attributes;

use crate::entity::ai::control::{MoveControlOperation, MobControls};
use crate::entity::ai::path::PathType;
use crate::entity::mob::{Mob, MobBase, rotlerp};
use crate::physics::{MoveResult, MoverType};

const TRAVEL_SPEED: f32 = 0.01;
const DRAG: f64 = 0.9;
const SINK: f64 = -0.005;
const BUOYANCY: f64 = 0.005;

static STEEL_FISH_DEBUG: OnceLock<bool> = OnceLock::new();

fn is_debug_enabled() -> bool {
    *STEEL_FISH_DEBUG.get_or_init(|| std::env::var("STEEL_FISH_DEBUG").is_ok())
}

pub fn tick_move_control<M: Mob + ?Sized>(mob: &M) {
    let (move_control, is_done) = {
        let mut controls = mob.mob_base().controls().lock();
        let operation = controls.move_control.operation();
        let wanted = controls.move_control.wanted_position();
        let speed = controls.move_control.speed_modifier();
        if matches!(operation, MoveControlOperation::MoveTo) {
            controls.move_control.set_wait();
        }
        let is_done = mob.mob_base().navigation().lock().is_done();
        ((operation, wanted, speed), is_done)
    };

    let (op, wanted, speed_modifier) = move_control;

    if mob.is_effective_ai() && mob.is_in_water() && mob.get_eye_y() > mob.position().y {
        let mut vel = mob.velocity();
        vel.y += BUOYANCY;
        mob.set_velocity(vel);
    }

    if matches!(op, MoveControlOperation::MoveTo) && !is_done {
        let movement_speed = mob
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED);
        let target_speed = (speed_modifier * movement_speed) as f32;
        let current_speed = mob.get_speed();
        let speed = current_speed + (target_speed - current_speed) * 0.125;
        mob.set_mob_speed(speed);

        let pos = mob.position();
        let xd = wanted.x - pos.x;
        let yd = wanted.y - pos.y;
        let zd = wanted.z - pos.z;
        let dd = (xd * xd + yd * yd + zd * zd) as f32;
        let dd_sqrt = dd.sqrt();
        if dd_sqrt > 0.0 {
            if yd != 0.0 {
                let mut vel = mob.velocity();
                vel.y += (speed as f64) * (yd as f64 / dd_sqrt as f64) * 0.1;
                mob.set_velocity(vel);
            }

            let target_yaw = (zd.atan2(xd) as f32 * 180.0 / std::f32::consts::PI) - 90.0;
            let yaw = rotlerp(mob.rotation().0, target_yaw, 90.0);
            let (_, pitch) = mob.rotation();
            mob.set_rotation((yaw, pitch));
            mob.set_y_body_rot(yaw);
        }
    } else {
        mob.set_mob_speed(0.0);
    }
}

pub fn travel<M: Mob + ?Sized>(mob: &M, input: DVec3) -> Option<MoveResult> {
    let debug = is_debug_enabled() && mob.id() % 100 == 0;

    if debug {
        let pos = mob.position();
        let vel = mob.velocity();
        eprintln!(
            "[FISH-{}] tick={} pos=({:.2},{:.2},{:.2}) vel=({:.4},{:.4},{:.4}) in_water={} on_ground={} nav_done={}",
            mob.id(),
            mob.tick_count(),
            pos.x, pos.y, pos.z,
            vel.x, vel.y, vel.z,
            mob.is_in_water(),
            mob.on_ground(),
            mob.mob_base().navigation().lock().is_done(),
        );
    }

    mob.move_relative(TRAVEL_SPEED, input);

    if debug {
        let vel = mob.velocity();
        eprintln!("[FISH-{}] after move_relative vel=({:.4},{:.4},{:.4})", mob.id(), vel.x, vel.y, vel.z);
    }

    let result = mob.move_entity(MoverType::SelfMovement, mob.velocity())?;

    if debug {
        let vel = mob.velocity();
        eprintln!("[FISH-{}] after move_entity vel=({:.4},{:.4},{:.4})", mob.id(), vel.x, vel.y, vel.z);
    }

    let mut vel = mob.velocity();
    vel *= DRAG;

    if mob.target().is_none() {
        vel.y += SINK;
    }

    if debug {
        eprintln!("[FISH-{}] after drag/sink vel=({:.4},{:.4},{:.4})", mob.id(), vel.x, vel.y, vel.z);
    }

    mob.set_velocity(vel);
    Some(result)
}

pub fn init_mob_base(mob_base: &MobBase) {
    mob_base.navigation().lock().set_water_bound(true);
    mob_base
        .pathfinding_malus()
        .lock()
        .set(PathType::Water, 0.0);
}