//! 3D affine transforms

use glam::{Mat3, Mat4, Quat, Vec3, Vec4Swizzles};
use std::f32::consts;
use std::mem;

const DECOMPOSITION_EPSILON: f32 = 1.0e-6;
const DECOMPOSITION_EPSILON_F64: f64 = 1.0e-6;
const JACOBI_ITERATIONS: usize = 5;

/// Affine transform `T * L * S * R`, right rotation acts first
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transformation {
    /// Translation
    pub translation: Vec3,
    /// Rotation after scaling
    pub left_rotation: Quat,
    /// Scale
    pub scale: Vec3,
    /// Rotation before scaling
    pub right_rotation: Quat,
}

impl Transformation {
    /// Identity transform
    pub const IDENTITY: Self = Transformation {
        translation: Vec3::ZERO,
        left_rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
        right_rotation: Quat::IDENTITY,
    };

    /// Compose the matrix
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
        let p = f64::from(a).hypot(f64::from(b)) as f32;

        let mut sin_half = if p > DECOMPOSITION_EPSILON { b } else { 0.0 };
        let mut cos_half = a.abs() + p.max(DECOMPOSITION_EPSILON);
        if a < 0.0 {
            mem::swap(&mut sin_half, &mut cos_half);
        }

        Givens::from_unnormalized(sin_half, cos_half)
    }

    fn joml_mul(left: Mat3, right: Mat3) -> Mat3 {
        let column = |v: Vec3| left.x_axis * v.x + (left.y_axis * v.y + left.z_axis * v.z);
        Mat3::from_cols(
            column(right.x_axis),
            column(right.y_axis),
            column(right.z_axis),
        )
    }

    fn similarity_transform(a: &mut Mat3, q: Mat3) {
        *a = Self::joml_mul(q.transpose(), Self::joml_mul(*a, q));
    }

    fn step_jacobi(m: &mut Mat3, result: &mut Quat) {
        if m.col(0)[1] * m.col(0)[1] + m.col(1)[0] * m.col(1)[0] > DECOMPOSITION_EPSILON {
            let g = Self::approx_givens_quat(
                m.col(0)[0],
                f32::midpoint(m.col(0)[1], m.col(1)[0]),
                m.col(1)[1],
            );

            *result *= g.around_z_quat();
            Self::similarity_transform(m, g.around_z_mat());
        }

        if m.col(0)[2] * m.col(0)[2] + m.col(2)[0] * m.col(2)[0] > DECOMPOSITION_EPSILON {
            let g = Self::approx_givens_quat(
                m.col(0)[0],
                f32::midpoint(m.col(0)[2], m.col(2)[0]),
                m.col(2)[2],
            )
            .inverse();

            *result *= g.around_y_quat();
            Self::similarity_transform(m, g.around_y_mat());
        }

        if m.col(1)[2] * m.col(1)[2] + m.col(2)[1] * m.col(2)[1] > DECOMPOSITION_EPSILON {
            let g = Self::approx_givens_quat(
                m.col(1)[1],
                f32::midpoint(m.col(1)[2], m.col(2)[1]),
                m.col(2)[2],
            );

            *result *= g.around_x_quat();
            Self::similarity_transform(m, g.around_x_mat());
        }
    }

    /// Split the matrix into translation, rotations and scale using SVD
    #[expect(clippy::similar_names, reason = "matches the stages of decomposition")]
    #[must_use]
    pub fn decompose(matrix: Mat4) -> Self {
        let scale_factor = 1.0 / matrix.col(3)[3];
        let input = matrix * scale_factor;

        let translation = input.w_axis.xyz();

        let mat = Mat3::from_mat4(input);

        let mut ata = Self::joml_mul(mat.transpose(), mat);

        let mut right_rotation = Quat::IDENTITY;
        for _ in 0..JACOBI_ITERATIONS {
            Self::step_jacobi(&mut ata, &mut right_rotation);
        }
        let [x, y, z, w] = right_rotation.to_array();
        let length_squared = x * x + (y * y + (z * z + w * w));
        let inv_sqrt = 1.0 / (f64::from(length_squared).sqrt() as f32);
        let [x, y, z, w] = [x, y, z, w].map(|component| component * inv_sqrt);

        right_rotation = Quat::from_xyzw(x, y, z, w);

        let zero_column_0 = f64::from(ata.col(0)[0]) < DECOMPOSITION_EPSILON_F64;
        let zero_column_1 = f64::from(ata.col(1)[1]) < DECOMPOSITION_EPSILON_F64;

        let (ww, xx, yy, zz) = (w * w, x * x, y * y, z * z);
        let (zw, xy, xz, yw, yz, xw) = (
            2.0 * (z * w),
            2.0 * (x * y),
            2.0 * (x * z),
            2.0 * (y * w),
            2.0 * (y * z),
            2.0 * (x * w),
        );

        let rotation = Mat3::from_cols(
            Vec3::new(ww + xx - zz - yy, xy + zw, xz - yw),
            Vec3::new(xy - zw, yy - zz + ww - xx, yz + xw),
            Vec3::new(yw + xz, yz - xw, zz - yy - xx + ww),
        );

        let column = |v: Vec3| (mat.x_axis * v.x + mat.y_axis * v.y) + mat.z_axis * v.z;
        let u012s = Mat3::from_cols(
            column(rotation.x_axis),
            column(rotation.y_axis),
            column(rotation.z_axis),
        );

        let mut left_rotation = Quat::IDENTITY;

        // QR rotations in Z, Y, X order
        let givens = if zero_column_0 {
            Self::qr_givens_quat(u012s.col(1)[1], -u012s.col(1)[0])
        } else {
            Self::qr_givens_quat(u012s.col(0)[0], u012s.col(0)[1])
        };

        let mut u12s = givens.around_z_mat();
        left_rotation *= givens.around_z_quat();
        u12s = Self::joml_mul(u12s.transpose(), u012s);

        let givens = if zero_column_0 {
            Self::qr_givens_quat(u12s.col(2)[2], -u12s.col(2)[0])
        } else {
            Self::qr_givens_quat(u12s.col(0)[0], u12s.col(0)[2])
        }
        .inverse();
        let mut u2s = givens.around_y_mat();
        left_rotation *= givens.around_y_quat();
        u2s = Self::joml_mul(u2s.transpose(), u12s);

        let givens = if zero_column_1 {
            Self::qr_givens_quat(u2s.col(2)[2], -u2s.col(2)[1])
        } else {
            Self::qr_givens_quat(u2s.col(1)[1], u2s.col(1)[2])
        };
        let mut sm = givens.around_x_mat();
        left_rotation *= givens.around_x_quat();
        sm = Self::joml_mul(sm.transpose(), u2s);

        let scale = Vec3::new(sm.col(0)[0], sm.col(1)[1], sm.col(2)[2]);

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
    const SIN_PI_8: f32 = 0.382_683_46;
    const COS_PI_8: f32 = 0.923_879_5;

    pub const PI_4: Self = Self::new(Self::SIN_PI_8, Self::COS_PI_8);

    pub const fn new(sin_half: f32, cos_half: f32) -> Self {
        Self { sin_half, cos_half }
    }

    fn from_unnormalized(sin_half: f32, cos_half: f32) -> Self {
        let length_squared = sin_half * sin_half + cos_half * cos_half;
        let inv_sqrt = 1.0 / (f64::from(length_squared).sqrt() as f32);
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
    fn from(matrix: Mat4) -> Self {
        Transformation::decompose(matrix)
    }
}

impl From<Transformation> for Mat4 {
    fn from(t: Transformation) -> Self {
        Transformation::compose(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    fn check_transformation(transformation: Transformation, given_mat: Mat4) {
        let new = Transformation::decompose(transformation.compose());

        assert!(
            transformation.compose().abs_diff_eq(new.compose(), 1.0e-5),
            "Transformation matrices are not approximately equal: {transformation:?} {new:?}"
        );
        assert!(
            transformation.compose().abs_diff_eq(given_mat, 1.0e-5),
            "The new matrix and the given matrix are not approximately equal: {:?} {given_mat:?}",
            transformation.compose()
        );
    }

    #[test]
    fn compose_and_decompose() {
        // matrices from vanilla composition
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
                Vec4::new(2.494_755_3, 1.422_634_6, -0.867_355_7, 0.0),
                Vec4::new(-1.072_713_7, 2.566_348_3, 1.123_895_5, 0.0),
                Vec4::new(1.274_943_1, -0.624_473_33, 2.642_830_4, 0.0),
                Vec4::new(1.0, 2.0, 3.0, 1.0),
            ),
        );

        let transformation = Transformation {
            translation: Vec3::new(-99.0, 25.0, 3000.0),
            left_rotation: Quat::from_axis_angle(Vec3::new(4.0, -55.0, 6.0).normalize(), 0.2),
            scale: Vec3::new(33.0, 32.0, 31.0),
            right_rotation: Quat::from_axis_angle(Vec3::new(7.0, 8.0, -99.0).normalize(), -1.24),
        };
        let vanilla_matrix = Mat4::from_cols(
            Vec4::new(9.745_661, 30.406_546, 3.377_947_8, 0.0),
            Vec4::new(-29.800_283, 9.970_775, -9.624_349, 0.0),
            Vec4::new(-10.048_495, -0.186_506_54, 29.358_908, 0.0),
            Vec4::new(-99.0, 25.0, 3000.0, 1.0),
        );
        assert!(transformation.compose().abs_diff_eq(vanilla_matrix, 1.0e-5));

        let decoded = Transformation::decompose(vanilla_matrix);
        assert_eq!(decoded.translation, transformation.translation);
        assert_eq!(
            decoded.left_rotation.to_array().map(f32::to_bits),
            [0xbd84_ebff, 0xbd99_c5d9, 0x3f36_11d7, 0x3f32_290b],
        );
        assert_eq!(
            decoded.scale.to_array().map(f32::to_bits),
            [0x4200_0001, 0x4204_0001, 0x41f8_0006],
        );
        assert_eq!(
            decoded.right_rotation.to_array().map(f32::to_bits),
            [0x3d7d_8581, 0x3b87_3545, 0x3e2b_0033, 0xbf7b_e7bd],
        );
    }
}
