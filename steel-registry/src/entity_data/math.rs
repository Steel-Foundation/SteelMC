use glam::{Mat4, Quat, Vec3};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};

type BorrowedNbtTag<'a, 'tape> = simdnbt::borrow::NbtTag<'a, 'tape>;

/// A 3D vector (for display entities).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vector3f {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const ONE: Self = Self {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };

    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

impl From<Vector3f> for Vec3 {
    fn from(value: Vector3f) -> Self {
        Vec3::new(value.x, value.y, value.z)
    }
}

impl ToNbtTag for Vector3f {
    fn to_nbt_tag(self) -> NbtTag {
        NbtList::Float(vec![self.x, self.y, self.z]).into()
    }
}

impl FromNbtTag for Vector3f {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(l) = tag.list()
            && let Some(floats) = l.floats()
            && floats.len() == 3
        {
            Some(Vector3f::new(floats[0], floats[1], floats[2]))
        } else {
            None
        }
    }
}

/// A rotation storing an angle and axis (in 3 components).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisAngle4f {
    pub angle: f32,
    pub axis: Vector3f,
}

impl ToNbtTag for AxisAngle4f {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("angle", self.angle);
        compound.insert("axis", self.axis);
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for AxisAngle4f {
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        if let Some(compound) = tag.compound()
            && let Some(angle) = compound.get("angle")
            && let Some(axis) = compound.get("axis")
        {
            Some(Self {
                angle: f32::from_nbt_tag(angle)?,
                axis: Vector3f::from_nbt_tag(axis)?,
            })
        } else {
            None
        }
    }
}

/// A quaternion rotation (for display entities).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternionf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// A quaternion of 4 elements.
impl Quaternionf {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
}
impl From<Quaternionf> for Quat {
    fn from(value: Quaternionf) -> Self {
        Quat::from_xyzw(value.x, value.y, value.z, value.w)
    }
}

impl From<AxisAngle4f> for Quaternionf {
    fn from(value: AxisAngle4f) -> Self {
        let half_angle = value.angle / 2.0;
        let sin = half_angle.sin();
        let cos = half_angle.cos();
        Self {
            x: value.axis.x * sin,
            y: value.axis.y * sin,
            z: value.axis.z * sin,
            w: cos,
        }
    }
}

impl ToNbtTag for Quaternionf {
    fn to_nbt_tag(self) -> NbtTag {
        NbtList::Float(vec![self.x, self.y, self.z, self.w]).into()
    }
}

impl FromNbtTag for Quaternionf {
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        // One of the two: 4 floats or AxisAngle4f
        if let Some(l) = tag.list()
            && let Some(floats) = l.floats()
            && floats.len() == 4
        {
            return Some(Quaternionf::new(floats[0], floats[1], floats[2], floats[3]));
        }
        Some(AxisAngle4f::from_nbt_tag(tag)?.into())
    }
}

/// A 4D matrix (for display entities).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix4f(pub Mat4);

impl FromNbtTag for Matrix4f {
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        let floats = tag.list()?.floats()?;
        let elements = floats.into_boxed_slice().as_ref().try_into().ok()?;
        Some(Matrix4f(Mat4::from_cols_array(&elements).transpose()))
    }
}

impl ToNbtTag for Matrix4f {
    fn to_nbt_tag(self) -> NbtTag {
        let elements = self.0.transpose().to_cols_array();
        NbtList::Float(elements.to_vec()).into()
    }
}
