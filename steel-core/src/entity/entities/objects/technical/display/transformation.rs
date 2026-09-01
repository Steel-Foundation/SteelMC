//! Contains code for transformations, composing and decomposing them,
//! and to serialize and deserialize them.

use glam::{Mat3, Mat4, Quat, Vec3, Vec4Swizzles};
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use std::f32::consts;
use std::mem;
use steel_utils::nbt::{
    mat4_from_nbt_tag, quat_from_nbt_tag, quat_to_nbt_tag, vec3_from_nbt_tag, vec3_to_nbt_tag,
};

/// A structure describing an affine transformation in 3D space.
///
/// Transformations are applied in the following order:
/// `translation` -> `left_rotation` -> `scale` -> `right_rotation`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transformation {
    /// The translation (displacement) applied by this transformation.
    pub translation: Vec3,
    /// The left rotation applied by this transformation.
    pub left_rotation: Quat,
    /// The scale applied by this transformation.
    pub scale: Vec3,
    /// The right rotation applied by this transformation.
    pub right_rotation: Quat,
}

impl Transformation {
    /// The identity [`Transformation`].
    pub const IDENTITY: Self = Transformation {
        translation: Vec3::ZERO,
        left_rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
        right_rotation: Quat::IDENTITY,
    };

    /// Composes a [`Mat4`] from this transformation.
    #[must_use]
    pub fn compose(self) -> Mat4 {
        Mat4::from_translation(self.translation)
            * Mat4::from_quat(self.left_rotation)
            * Mat4::from_scale(self.scale)
            * Mat4::from_quat(self.right_rotation)
    }

    fn approx_givens_quat(a11: f32, a12: f32, a22: f32) -> Givens {
        let cos_half = 2.0 * (a11 - a22);
        let sin_half = a12;

        if Givens::G * sin_half * sin_half < cos_half * cos_half {
            Givens::from_unnormalized(sin_half, cos_half)
        } else {
            Givens::PI_4
        }
    }

    fn qr_givens_quat(a: f32, b: f32) -> Givens {
        let p = a.hypot(b);

        let mut sin_half = if p > 1.0e-6 { b } else { 0.0 };
        let mut cos_half = a.abs() + p.max(1.0e-6);
        if a < 0.0 {
            mem::swap(&mut sin_half, &mut cos_half);
        }
        Givens::from_unnormalized(sin_half, cos_half)
    }

    fn similarity_transform(a: &mut Mat3, q: Mat3) {
        *a = q.transpose() * *a * q;
    }

    fn step_jacobi(m: &mut Mat3, result: &mut Quat) {
        if m.col(0)[1] * m.col(0)[1] + m.col(1)[0] * m.col(1)[0] > 1.0e-6 {
            let g = Self::approx_givens_quat(
                m.col(0)[0],
                f32::midpoint(m.col(0)[1], m.col(1)[0]),
                m.col(1)[1],
            );

            *result *= g.around_z_quat();
            Self::similarity_transform(m, g.around_z_mat());
        }

        if m.col(0)[2] * m.col(0)[2] + m.col(2)[0] * m.col(2)[0] > 1.0e-6 {
            let g = Self::approx_givens_quat(
                m.col(0)[0],
                f32::midpoint(m.col(0)[2], m.col(2)[0]),
                m.col(2)[2],
            )
            .inverse();

            *result *= g.around_y_quat();
            Self::similarity_transform(m, g.around_y_mat());
        }

        if m.col(1)[2] * m.col(1)[2] + m.col(2)[1] * m.col(2)[1] > 1.0e-6 {
            let g = Self::approx_givens_quat(
                m.col(1)[1],
                f32::midpoint(m.col(1)[2], m.col(2)[1]),
                m.col(2)[2],
            );

            *result *= g.around_x_quat();
            Self::similarity_transform(m, g.around_x_mat());
        }
    }

    /// Decomposes a [`Matrix4f`] to form a transformation.
    ///
    /// This function uses *singular value decomposition* (or SVD) to split the matrix
    /// into a set of transformation values.
    #[expect(clippy::similar_names, reason = "matches the stages of decomposition")]
    #[must_use]
    pub fn decompose(matrix: Mat4) -> Self {
        let scale_factor = 1.0 / matrix.col(3)[3];
        let input = matrix * scale_factor;

        // Extract the translation.
        let translation = input.w_axis.xyz() * scale_factor;

        let mat = Mat3::from_mat4(input);

        // Calculate transpose(A) * A.
        let mut ata = mat.transpose() * mat;

        // Approximate the eigendecomposition of ata.
        // This is achieved by using 5 Jacobi iterations.
        let mut right_rotation = Quat::IDENTITY;
        for _ in 0..5 {
            Self::step_jacobi(&mut ata, &mut right_rotation);
        }
        // Use the resulting quaternion to get the right rotation's conjugate.
        right_rotation = right_rotation.normalize();

        let zero_column_0 = ata.col(0)[0] < 1.0e-6;
        let zero_column_1 = ata.col(1)[1] < 1.0e-6;

        // Get a matrix that can be reduced to a diagonal form.
        let u012s = mat * Mat3::from_quat(right_rotation);
        let mut left_rotation = Quat::IDENTITY;

        // Now, calculate the 3 Givens rotations.
        // This gives us the left rotation.

        // 1) Along the Z axis
        let givens = if zero_column_0 {
            Self::qr_givens_quat(u012s.col(1)[1], -u012s.col(1)[0])
        } else {
            Self::qr_givens_quat(u012s.col(0)[0], u012s.col(0)[1])
        };
        let mut u12s = givens.around_z_mat();
        left_rotation *= givens.around_z_quat();
        u12s = u12s.transpose() * u012s;

        // 2) Along the Y axis
        let givens = if zero_column_0 {
            Self::qr_givens_quat(u12s.col(2)[2], -u12s.col(2)[0])
        } else {
            Self::qr_givens_quat(u12s.col(0)[0], u12s.col(0)[2])
        }
        .inverse();
        let mut u2s = givens.around_y_mat();
        left_rotation *= givens.around_y_quat();
        u2s = u2s.transpose() * u12s;

        // 3) Along the X axis
        let givens = if zero_column_1 {
            Self::qr_givens_quat(u2s.col(2)[2], -u2s.col(2)[1])
        } else {
            Self::qr_givens_quat(u2s.col(1)[1], u2s.col(1)[2])
        };
        let mut sm = givens.around_x_mat();
        left_rotation *= givens.around_x_quat();
        sm = sm.transpose() * u2s;

        // Get the scale components from sm, which should approximately be a diagonal matrix.
        let scale = Vec3::new(sm.col(0)[0], sm.col(1)[1], sm.col(2)[2]);

        // Finally, take the conjugate of the right rotation to get its final value.
        right_rotation = right_rotation.conjugate();

        Transformation {
            translation,
            left_rotation,
            scale,
            right_rotation,
        }
    }
}

#[derive(Clone, Copy)]
struct Givens {
    sin_half: f32,
    cos_half: f32,
}

impl Givens {
    pub const G: f32 = 3.0 + 2.0 * consts::SQRT_2;
    const SIN_PI_8: f32 = 0.382_683_43;
    const COS_PI_8: f32 = 0.923_879_5;

    pub const PI_4: Self = Self::new(Self::SIN_PI_8, Self::COS_PI_8);

    pub const fn new(sin_half: f32, cos_half: f32) -> Self {
        Self { sin_half, cos_half }
    }

    fn from_unnormalized(sin_half: f32, cos_half: f32) -> Self {
        let inv_sqrt = 1.0 / sin_half.hypot(cos_half);
        Self::new(inv_sqrt * sin_half, inv_sqrt * cos_half)
    }

    fn inverse(self) -> Self {
        Self::new(-self.sin_half, self.cos_half)
    }

    fn cos(self) -> f32 {
        self.cos_half * self.cos_half - self.sin_half * self.sin_half
    }

    fn sin(self) -> f32 {
        2.0 * self.sin_half * self.cos_half
    }

    const fn around_x_quat(self) -> Quat {
        Quat::from_xyzw(self.sin_half, 0.0, 0.0, self.cos_half)
    }
    const fn around_y_quat(self) -> Quat {
        Quat::from_xyzw(0.0, self.sin_half, 0.0, self.cos_half)
    }
    const fn around_z_quat(self) -> Quat {
        Quat::from_xyzw(0.0, 0.0, self.sin_half, self.cos_half)
    }

    fn around_x_mat(self) -> Mat3 {
        let cos = self.cos();
        let sin = self.sin();
        Mat3::from_cols(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, cos, sin),
            Vec3::new(0.0, -sin, cos),
        )
    }

    fn around_y_mat(self) -> Mat3 {
        let cos = self.cos();
        let sin = self.sin();
        Mat3::from_cols(
            Vec3::new(cos, 0.0, -sin),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(sin, 0.0, cos),
        )
    }

    fn around_z_mat(self) -> Mat3 {
        let cos = self.cos();
        let sin = self.sin();
        Mat3::from_cols(
            Vec3::new(cos, sin, 0.0),
            Vec3::new(-sin, cos, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        )
    }
}

impl From<Mat4> for Transformation {
    /// Decomposes a [`Matrix4f`] to form a [`Transformation`].
    fn from(matrix: Mat4) -> Self {
        Transformation::decompose(matrix)
    }
}

impl From<Transformation> for Mat4 {
    /// Composes a [`Transformation`] to form a matrix.
    fn from(t: Transformation) -> Self {
        Transformation::compose(t)
    }
}

struct NormalTransformation(Transformation);
impl From<Transformation> for NormalTransformation {
    fn from(t: Transformation) -> Self {
        Self(t)
    }
}
impl From<NormalTransformation> for Transformation {
    fn from(t: NormalTransformation) -> Self {
        t.0
    }
}

// Recreates Vanilla's `Transformation.CODEC`.
impl ToNbtTag for NormalTransformation {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("translation", vec3_to_nbt_tag(self.0.translation));
        compound.insert("left_rotation", quat_to_nbt_tag(self.0.left_rotation));
        compound.insert("scale", vec3_to_nbt_tag(self.0.scale));
        compound.insert("right_rotation", quat_to_nbt_tag(self.0.right_rotation));
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for NormalTransformation {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self(Transformation {
            translation: vec3_from_nbt_tag(compound.get("translation")?)?,
            left_rotation: quat_from_nbt_tag(compound.get("left_rotation")?)?,
            scale: vec3_from_nbt_tag(compound.get("scale")?)?,
            right_rotation: quat_from_nbt_tag(compound.get("right_rotation")?)?,
        }))
    }
}

// Recreates Vanilla's `Transformation.EXTENDED_CODEC`.
// This codec prefers using the ordinary codec created above, but it does also accept a matrix.
impl FromNbtTag for Transformation {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(NormalTransformation(transformation)) = NormalTransformation::from_nbt_tag(tag)
        {
            return Some(transformation);
        }
        Some(mat4_from_nbt_tag(tag)?.into())
    }
}

impl ToNbtTag for Transformation {
    fn to_nbt_tag(self) -> NbtTag {
        NormalTransformation(self).to_nbt_tag()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    /// Asserts that after decomposing transformation and composing it back (to create a new transformation `d`),
    /// `d` is approximately equal to `transformation`, and also checks if `d.compose()` is approximately equal to
    /// the given matrix.
    fn check_transformation(transformation: Transformation, given_mat: Mat4) {
        let new = Transformation::decompose(transformation.compose());
        assert!(
            transformation.compose().abs_diff_eq(new.compose(), 1.0e-4),
            "Transformation matrices are not approximately equal: {transformation:?} {new:?}"
        );
        assert!(
            transformation.compose().abs_diff_eq(given_mat, 1.0e-2),
            "The new matrix and the given matrix are not approximately equal: {:?} {given_mat:?}",
            transformation.compose()
        );
    }

    #[test]
    fn compose_and_decompose() {
        // The matrices given are the Matrix4fs taken from Minecraft
        // after converting a Transformation to its matrix in Java.

        check_transformation(
            Transformation::IDENTITY,
            Mat4::from_cols(
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ),
        );

        check_transformation(
            Transformation {
                translation: Vec3::new(1.0, 2.0, 3.0),
                left_rotation: Quat::from_axis_angle(Vec3::new(4.0, 5.0, 6.0).normalize(), 0.45),
                scale: Vec3::new(3.0, 3.0, 3.0),
                right_rotation: Quat::from_axis_angle(Vec3::new(7.0, 8.0, 9.0).normalize(), 0.22),
            },
            Mat4::from_cols(
                Vec4::new(2.495, 1.423, -0.8674, 0.0),
                Vec4::new(-1.073, 2.566, 1.124, 0.0),
                Vec4::new(1.275, -0.6245, 2.643, 0.0),
                Vec4::new(1.0, 2.0, 3.0, 1.0),
            ),
        );

        check_transformation(
            Transformation {
                translation: Vec3::new(-99.0, 25.0, 3000.0),
                left_rotation: Quat::from_axis_angle(Vec3::new(4.0, -55.0, 6.0).normalize(), 0.2),
                scale: Vec3::new(33.0, 32.0, 31.0),
                right_rotation: Quat::from_axis_angle(
                    Vec3::new(7.0, 8.0, -99.0).normalize(),
                    -1.24,
                ),
            },
            Mat4::from_cols(
                Vec4::new(9.746, 30.41, 3.378, 0.0),
                Vec4::new(-29.80, 9.971, -9.624, 0.0),
                Vec4::new(-10.05, -0.1865, 29.36, 0.0),
                Vec4::new(-99.0, 25.0, 3000.0, 1.0),
            ),
        );
    }
}
