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
