//! ABI-safe capability implementations supplied by extensions.
use super::types::{
    BonemealAction, DamageSource, FallingBlockEntityRef, LevelReader, Random,
    SharedFallingBlockEntity,
};
use crate::{BlockPos, BlockState, World};
/// Extension implementation of the Bonemealable capability.
#[stabby::stabby(checked)]
pub trait Bonemealable: Send + Sync {
    /// Get bonemeal age increase.
    extern "C" fn get_bonemeal_age_increase<'a>(&self, world: World<'a>, rng: Random<'a>) -> u8;
    /// Is valid bonemeal target.
    extern "C" fn is_valid_bonemeal_target(
        &self,
        state: BlockState,
        world: LevelReader<'_>,
        pos: BlockPos,
    ) -> bool;
    /// Is bonemeal success.
    extern "C" fn is_bonemeal_success<'a>(
        &self,
        state: BlockState,
        world: World<'a>,
        rng: Random<'a>,
        pos: BlockPos,
    ) -> bool;
    /// Perform bonemeal.
    extern "C" fn perform_bonemeal<'a>(
        &self,
        state: BlockState,
        world: World<'a>,
        rng: Random<'a>,
        pos: BlockPos,
    );
    /// Bonemeal action type.
    extern "C" fn bonemeal_action_type(&self) -> BonemealAction;
}
/// Owning, checked Bonemealable implementation.
pub type DynBonemealable = stabby::dynptr!(stabby::boxed::Box<dyn Bonemealable + Send + Sync>);
/// A registered capability that may be retained by extension behavior objects.
#[stabby::stabby]
#[derive(Clone, Copy)]
pub struct RegisteredBonemealable {
    pub(crate) id: usize,
}
/// Extension implementation of the Fallable capability.
#[stabby::stabby(checked)]
pub trait Fallable: Send + Sync {
    /// On land.
    extern "C" fn on_land<'a>(
        &self,
        world: World<'a>,
        pos: BlockPos,
        state: BlockState,
        replaced_state: BlockState,
        entity: FallingBlockEntityRef<'a>,
    );
    /// On broken after fall.
    extern "C" fn on_broken_after_fall<'a>(
        &self,
        world: World<'a>,
        pos: BlockPos,
        entity: FallingBlockEntityRef<'a>,
    );
    /// Get fall damage source.
    extern "C" fn get_fall_damage_source(
        &self,
        entity: SharedFallingBlockEntity<'_>,
    ) -> DamageSource;
    /// Is concrete powder.
    extern "C" fn is_concrete_powder(&self) -> bool;
}
/// Owning, checked Fallable implementation.
pub type DynFallable = stabby::dynptr!(stabby::boxed::Box<dyn Fallable + Send + Sync>);
/// A registered capability that may be retained by extension behavior objects.
#[stabby::stabby]
#[derive(Clone, Copy)]
pub struct RegisteredFallable {
    pub(crate) id: usize,
}
/// Extension implementation of the `GrowingPlantHeadBehavior` capability.
#[stabby::stabby(checked)]
pub trait GrowingPlantHeadBehavior: Send + Sync {
    /// Is max age.
    extern "C" fn is_max_age(&self, state: BlockState) -> bool;
    /// Get max age state.
    extern "C" fn get_max_age_state(&self, state: BlockState) -> BlockState;
}
/// Owning, checked `GrowingPlantHeadBehavior` implementation.
pub type DynGrowingPlantHeadBehavior =
    stabby::dynptr!(stabby::boxed::Box<dyn GrowingPlantHeadBehavior + Send + Sync>);
/// A registered capability that may be retained by extension behavior objects.
#[stabby::stabby]
#[derive(Clone, Copy)]
pub struct RegisteredGrowingPlantHeadBehavior {
    pub(crate) id: usize,
}
/// Extension implementation of the `RailBehavior` capability.
#[stabby::stabby(checked)]
pub trait RailBehavior: Send + Sync {
    /// Is straight.
    extern "C" fn is_straight(&self) -> bool;
}
/// Owning, checked `RailBehavior` implementation.
pub type DynRailBehavior = stabby::dynptr!(stabby::boxed::Box<dyn RailBehavior + Send + Sync>);
/// A registered capability that may be retained by extension behavior objects.
#[stabby::stabby]
#[derive(Clone, Copy)]
pub struct RegisteredRailBehavior {
    pub(crate) id: usize,
}
