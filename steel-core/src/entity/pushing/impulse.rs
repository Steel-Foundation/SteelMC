//! shared vanilla push math

use std::simd::f64x2;

use glam::DVec3;

use super::EntityPush;

const MIN_PUSH_DISTANCE: f32 = 0.01;
const PUSH_STRENGTH: f32 = 0.05;

impl EntityPush {
    pub(crate) fn impulse(source: DVec3, target: DVec3) -> Option<DVec3> {
        let x = target.x - source.x;
        let z = target.z - source.z;

        let distance = x.abs().max(z.abs());

        if distance < f64::from(MIN_PUSH_DISTANCE) {
            return None;
        }

        let distance = distance.sqrt();
        let mut horizontal = f64x2::from_array([x, z]) / f64x2::splat(distance);

        let scale = if distance > 1.0 { 1.0 / distance } else { 1.0 };
        horizontal *= f64x2::splat(scale);
        horizontal *= f64x2::splat(f64::from(PUSH_STRENGTH));
        let [x, z] = horizontal.to_array();

        Some(DVec3::new(x, 0.0, z))
    }
}
