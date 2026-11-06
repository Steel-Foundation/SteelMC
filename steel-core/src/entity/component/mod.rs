//! Entity components: per-concern state shared by many entity types.
//!
//! Vanilla models entities as a deep class hierarchy (`Entity` → `LivingEntity`
//! → `Mob` → `AgeableMob` → `Animal` → `Chicken`). Steel splits what those
//! classes do into three mechanisms instead of mirroring the hierarchy:
//!
//! - **Components** hold one concern's state (age, love, …) and save/load it
//!   themselves, so a layer's persistent data can never be forgotten.
//! - **Pipelines** ([`pipeline`]) run behavior chains such as `aiStep` as an
//!   explicit, ordered list of stages. Layer presets encode vanilla's chain;
//!   entity types only list their deviations.
//! - **Properties** are per-type values and hooks that vanilla subclasses
//!   override without calling `super` (sounds, food, offspring). They live in
//!   `&'static` property structs referenced by the component that uses them.
//!
//! Layer modules expose borrowed views (for example `Ageable`) that pair an
//! entity with its components and carry the layer's behavior.

pub mod definition;
pub mod entries;
pub mod pipeline;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_utils::{Downcast as _, DowncastType};

use crate::entity::Entity;

/// State for one concern of an entity, found by its downcast key.
///
/// Components lock their own state (typically through
/// [`steel_utils::locks::CheckedMutex`]), so a component reference can be held
/// while calling other entities. Never hold a component's guard across such a
/// call: vanilla logic re-enters entities constantly, and a held guard turns
/// that into a panic.
pub trait Component: DowncastType + Send + Sync {
    /// Writes this component's persistent fields.
    fn save(&self, _owner: &dyn Entity, _nbt: &mut NbtCompound) {}

    /// Reads this component's persistent fields.
    fn load(&self, _owner: &dyn Entity, _nbt: BorrowedNbtCompoundView<'_, '_>) {}
}

/// Static access to a component the entity type always has.
pub trait Has<C: Component> {
    /// Returns the component.
    fn component(&self) -> &C;
}

/// Component access by type for entities with [`Has`] implementations.
pub trait Components {
    /// Returns component `C`, which this entity type always has.
    fn get<C: Component>(&self) -> &C
    where
        Self: Has<C>,
    {
        Has::<C>::component(self)
    }

    /// Returns component `C` when this entity has it: the component
    /// equivalent of vanilla's `instanceof` checks.
    fn query<C: Component>(&self) -> Option<&C>
    where
        Self: Entity,
    {
        self.erased_component(C::TYPE_KEY)?.downcast_ref::<C>()
    }
}

impl<T: Entity + ?Sized> Components for T {}
