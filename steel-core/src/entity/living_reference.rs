use crate::entity::{LivingEntity, SharedEntity};

/// A living entity capability tied to the same shared allocation used for attribution.
#[derive(Clone, Copy)]
pub struct LivingEntityRef<'a> {
    entity: &'a SharedEntity,
    living: &'a dyn LivingEntity,
}

impl<'a> LivingEntityRef<'a> {
    /// Borrows the living capability, if the entity provides it.
    #[must_use]
    pub fn new(entity: &'a SharedEntity) -> Option<Self> {
        Some(Self {
            entity,
            living: entity.as_living_entity()?,
        })
    }

    /// The allocation to retain when constructing a damage source.
    #[must_use]
    pub const fn entity(self) -> &'a SharedEntity {
        self.entity
    }

    /// The living capability of this allocation.
    #[must_use]
    pub const fn living(self) -> &'a dyn LivingEntity {
        self.living
    }
}
