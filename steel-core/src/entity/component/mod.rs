//! Entity components: per-concern state shared by many entity types.
//!
//! Vanilla models entities as a deep class hierarchy (`Entity` → `LivingEntity`
//! → `Mob` → `AgeableMob` → `Animal` → `Chicken`). Steel splits what those
//! classes do into two mechanisms instead of mirroring the hierarchy:
//!
//! - **Components** hold one concern's state (age, love, …) and save/load it
//!   themselves, so a layer's persistent data can never be forgotten.
//! - **Properties** are the methods vanilla subclasses override, one table of
//!   `fn` pointers per vanilla class that declares them (`EntityProps`,
//!   `LivingProps`, `MobProps`, …). A method goes in the table of the class
//!   that declares it. Each layer has a preset per table, built on its parent
//!   layer's, listing the layer's overrides: functions of the layer's module
//!   named after the method. One that extends its parent the way vanilla calls
//!   `super` calls the parent layer's function:
//!
//!   ```ignore
//!   pub fn ai_step<E: Parts>(animal: &E) -> Option<MoveResult> {
//!       let movement = ageable::ai_step(animal);
//!       animal.animal().tick_love();
//!       movement
//!   }
//!   ```
//!
//! Layer modules expose borrowed views (for example `Ageable`) that pair an
//! entity with its components and carry the layer's behavior.

pub(crate) mod props;

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
