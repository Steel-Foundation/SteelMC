//! Immutable block-state data from generated resolvers, owned state tables, or a fixed value.

/// Generated vanilla blocks resolve data by local state offset. Independent
/// custom blocks can retain one fixed value or an owned table for their state family.
#[derive(Debug, Clone, Copy)]
pub enum BlockStateData<T: 'static> {
    Fixed(T),
    ByState(fn(u16) -> T),
    /// Registry-owned values in block-local state order.
    Values(&'static [T]),
}

impl<T: Copy + 'static> BlockStateData<T> {
    #[must_use]
    pub fn value(&self, offset: u16) -> T {
        match self {
            Self::Fixed(value) => *value,
            Self::ByState(resolve) => resolve(offset),
            Self::Values(values) => values[usize::from(offset)],
        }
    }
}
