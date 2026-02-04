//! A 3D vector.
use std::{
    io::{Cursor, Result, Write},
    ops::{Add, AddAssign, Div, Mul, Sub},
};

use num_traits::{Float, Num};

use crate::{
    math::vector2::Vector2,
    serial::{ReadFrom, WriteTo},
    types::BlockPos,
};

/// A 3D vector.
#[derive(Clone, Copy, Debug, PartialEq, Hash, Eq, Default)]
#[allow(missing_docs)]
pub struct Vector3<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

/// An axis in 3D space.
#[derive(Copy, Clone, Debug, Eq)]
#[derive_const(PartialEq)]
#[allow(missing_docs)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[allow(missing_docs)]
impl Axis {
    #[must_use]
    pub const fn as_str(&self) -> &str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
    }
}

#[allow(missing_docs)]
impl<T: Math + PartialOrd + Copy> Vector3<T> {
    pub const fn new(x: T, y: T, z: T) -> Self {
        Vector3 { x, y, z }
    }

    #[must_use]
    pub fn length_squared(&self) -> T {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    #[must_use]
    pub fn horizontal_length_squared(&self) -> T {
        self.x * self.x + self.z * self.z
    }

    #[must_use]
    pub fn add(&self, other: &Vector3<T>) -> Self {
        Vector3 {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    #[must_use]
    pub fn add_raw(&self, x: T, y: T, z: T) -> Self {
        Vector3 {
            x: self.x + x,
            y: self.y + y,
            z: self.z + z,
        }
    }

    #[must_use]
    pub fn sub(&self, other: &Vector3<T>) -> Self {
        Vector3 {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    #[must_use]
    pub fn sub_raw(&self, x: T, y: T, z: T) -> Self {
        Vector3 {
            x: self.x - x,
            y: self.y - y,
            z: self.z - z,
        }
    }

    #[must_use]
    pub fn multiply(self, x: T, y: T, z: T) -> Self {
        Self {
            x: self.x * x,
            y: self.y * y,
            z: self.z * z,
        }
    }

    #[must_use]
    pub fn lerp(&self, other: &Vector3<T>, t: T) -> Self {
        Vector3 {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            z: self.z + (other.z - self.z) * t,
        }
    }

    #[must_use]
    pub fn sign(&self) -> Vector3<i32>
    where
        T: Num + PartialOrd + Copy,
    {
        Vector3 {
            x: if self.x > T::zero() {
                1
            } else if self.x < T::zero() {
                -1
            } else {
                0
            },
            y: if self.y > T::zero() {
                1
            } else if self.y < T::zero() {
                -1
            } else {
                0
            },
            z: if self.z > T::zero() {
                1
            } else if self.z < T::zero() {
                -1
            } else {
                0
            },
        }
    }

    #[must_use]
    pub fn squared_distance_to_vec(&self, other: Self) -> T {
        self.squared_distance_to(other.x, other.y, other.z)
    }

    #[must_use]
    pub fn squared_distance_to(&self, x: T, y: T, z: T) -> T {
        let delta_x = self.x - x;
        let delta_y = self.y - y;
        let delta_z = self.z - z;
        delta_x * delta_x + delta_y * delta_y + delta_z * delta_z
    }

    #[must_use]
    pub fn is_within_bounds(&self, block_pos: Self, x: T, y: T, z: T) -> bool {
        let min_x = block_pos.x - x;
        let max_x = block_pos.x + x;
        let min_y = block_pos.y - y;
        let max_y = block_pos.y + y;
        let min_z = block_pos.z - z;
        let max_z = block_pos.z + z;

        self.x >= min_x
            && self.x <= max_x
            && self.y >= min_y
            && self.y <= max_y
            && self.z >= min_z
            && self.z <= max_z
    }
}

#[allow(missing_docs)]
impl<T: Math + Copy + Float> Vector3<T> {
    #[must_use]
    pub fn length(&self) -> T {
        self.length_squared().sqrt()
    }

    #[must_use]
    pub fn horizontal_length(&self) -> T {
        self.horizontal_length_squared().sqrt()
    }

    #[must_use]
    pub fn normalize(&self) -> Self {
        let length = self.length();
        Vector3 {
            x: self.x / length,
            y: self.y / length,
            z: self.z / length,
        }
    }

    #[must_use]
    pub fn rotation_vector(pitch: T, yaw: T) -> Self {
        let h = pitch.to_radians();
        let i = (-yaw).to_radians();

        let l = h.cos();
        Self {
            x: i.sin() * l,
            y: -h.sin(),
            z: i.cos() * l,
        }
    }
}

#[allow(missing_docs)]
impl<T: Math + Copy> Mul<T> for Vector3<T> {
    type Output = Self;

    fn mul(self, scalar: T) -> Self {
        Self {
            x: self.x * scalar,
            y: self.y * scalar,
            z: self.z * scalar,
        }
    }
}

#[allow(missing_docs)]
impl<T: Math + Copy> Add for Vector3<T> {
    type Output = Vector3<T>;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

#[allow(missing_docs)]
impl<T: Math + Copy> AddAssign for Vector3<T> {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
        self.z += other.z;
    }
}

/*
impl<T: Math + Copy> Neg for Vector3<T> {
    type Output = Self;

    fn neg(self) -> Self {
        Vector3 {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}
*/

#[allow(missing_docs)]
impl<T> From<(T, T, T)> for Vector3<T> {
    fn from((x, y, z): (T, T, T)) -> Self {
        Vector3 { x, y, z }
    }
}

#[allow(missing_docs)]
impl<T> From<Vector3<T>> for (T, T, T) {
    fn from(vector: Vector3<T>) -> Self {
        (vector.x, vector.y, vector.z)
    }
}

#[allow(missing_docs)]
impl<T: Math + Copy + Into<f64>> Vector3<T> {
    #[must_use]
    pub fn to_f64(&self) -> Vector3<f64> {
        Vector3 {
            x: self.x.into(),
            y: self.y.into(),
            z: self.z.into(),
        }
    }
}

#[allow(missing_docs)]
impl<T: Math + Copy + Into<f64>> Vector3<T> {
    #[must_use]
    pub fn to_i32(&self) -> Vector3<i32> {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        let z: f64 = self.z.into();
        Vector3 {
            x: x.round() as i32,
            y: y.round() as i32,
            z: z.round() as i32,
        }
    }

    #[must_use]
    pub fn to_vec2_i32(&self) -> Vector2<i32> {
        let x: f64 = self.x.into();
        let z: f64 = self.z.into();
        Vector2 {
            x: x.round() as i32,
            y: z.round() as i32,
        }
    }
}

#[allow(missing_docs)]
impl<T: Math + Copy + Into<f64>> Vector3<T> {
    #[must_use]
    pub fn to_block_pos(&self) -> BlockPos {
        BlockPos(self.to_i32())
    }
}

#[allow(missing_docs)]
impl<T: WriteTo> WriteTo for Vector3<T> {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.x.write(writer)?;
        self.y.write(writer)?;
        self.z.write(writer)
    }
}

#[allow(missing_docs)]
impl<T: ReadFrom> ReadFrom for Vector3<T> {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            x: T::read(data)?,
            y: T::read(data)?,
            z: T::read(data)?,
        })
    }
}

/// A trait for types that can be used in a `Vector3`.
#[allow(missing_docs)]
pub trait Math:
    Mul<Output = Self>
    //+ Neg<Output = Self>
    + Add<Output = Self>
    + AddAssign<>
    + Div<Output = Self>
    + Sub<Output = Self>
    + Sized
{
}
impl Math for i16 {}
impl Math for f64 {}
impl Math for f32 {}
impl Math for i32 {}
impl Math for i64 {}
impl Math for u8 {}
