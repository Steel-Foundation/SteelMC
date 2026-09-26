//! Port of Mojang `net.minecraft.world.phys.Vec3`'s axis rotations.
//!
//! Named after the vanilla methods rather than `rotate_x` / `rotate_y`, because
//! [`glam::DVec3`] has inherent methods by those names that rotate the opposite
//! way around X. `yRot` agrees between the two, so the mismatch hides easily.
//!
//! # Vanilla reference
//! ```java
//! public Vec3 xRot(final float radians) {
//!     float cos = Mth.cos(radians);
//!     float sin = Mth.sin(radians);
//!     double xx = this.x;
//!     double yy = this.y * cos + this.z * sin;
//!     double zz = this.z * cos - this.y * sin;
//!     return new Vec3(xx, yy, zz);
//! }
//! ```
//! `yRot` and `zRot` have the same shape, each holding its own axis fixed.

use glam::DVec3;

use crate::trig;

/// Returns the table-backed cosine and sine vanilla's rotations index.
fn cos_sin(radians: f32) -> (f64, f64) {
    let angle = f64::from(radians);
    (f64::from(trig::cos(angle)), f64::from(trig::sin(angle)))
}

/// Vanilla `Vec3.xRot`.
#[inline]
#[must_use]
pub fn x_rot(vector: DVec3, radians: f32) -> DVec3 {
    let (cos, sin) = cos_sin(radians);
    DVec3::new(
        vector.x,
        vector.y * cos + vector.z * sin,
        vector.z * cos - vector.y * sin,
    )
}

/// Vanilla `Vec3.yRot`.
#[inline]
#[must_use]
pub fn y_rot(vector: DVec3, radians: f32) -> DVec3 {
    let (cos, sin) = cos_sin(radians);
    DVec3::new(
        vector.x * cos + vector.z * sin,
        vector.y,
        vector.z * cos - vector.x * sin,
    )
}

/// Vanilla `Vec3.zRot`.
#[inline]
#[must_use]
pub fn z_rot(vector: DVec3, radians: f32) -> DVec3 {
    let (cos, sin) = cos_sin(radians);
    DVec3::new(
        vector.x * cos + vector.y * sin,
        vector.y * cos - vector.x * sin,
        vector.z,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    /// The sine table steps by 2π / 65536, bounding the error on a unit vector.
    const TABLE_EPSILON: f64 = 1e-4;

    fn assert_close(actual: DVec3, expected: DVec3) {
        assert!(
            (actual - expected).length() < TABLE_EPSILON,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn rotating_by_zero_returns_the_input() {
        let vector = DVec3::new(1.0, 2.0, 3.0);

        assert_eq!(x_rot(vector, 0.0).to_array(), vector.to_array());
        assert_eq!(y_rot(vector, 0.0).to_array(), vector.to_array());
        assert_eq!(z_rot(vector, 0.0).to_array(), vector.to_array());
    }

    #[test]
    fn quarter_turns_match_vanilla_axis_mapping() {
        assert_close(x_rot(DVec3::Y, FRAC_PI_2), DVec3::new(0.0, 0.0, -1.0));
        assert_close(y_rot(DVec3::X, FRAC_PI_2), DVec3::new(0.0, 0.0, -1.0));
        assert_close(z_rot(DVec3::X, FRAC_PI_2), DVec3::new(0.0, -1.0, 0.0));
    }

    /// Pins the sign convention that reaching for `DVec3::rotate_x` breaks.
    #[test]
    fn x_rot_opposes_glam_rotate_x_while_y_rot_agrees() {
        let down = DVec3::new(0.0, -1.0, 0.0);
        let radians = 30.0_f32.to_radians();

        assert_close(x_rot(down, radians), DVec3::new(0.0, -0.866_025, 0.5));
        assert_close(
            down.rotate_x(f64::from(radians)),
            DVec3::new(0.0, -0.866_025, -0.5),
        );

        let vector = DVec3::new(1.0, 0.0, 0.5);
        assert_close(y_rot(vector, radians), vector.rotate_y(f64::from(radians)));
    }
}
