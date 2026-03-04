use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::blocks::properties::Direction;
use steel_registry::items::ItemRef;
use steel_utils::{BlockPos, BlockStateId};
use crate::world::World;
use crate::entity::Entity;

/// Trait for fluid behavior implementations.
/// Conceptual equivalent of Minecraft's `Fluid` class.
pub trait FluidBehavior: Send + Sync {

    // === Identity ===

    /// Gets the fluid type for this behaviour.
    fn fluid_type(&self) -> FluidRef;

    /// Checks if this fluid is the same type as another fluid ref.
    /// This is used to determine if fluids can flow into each other, etc.
    /// By default, it compares the fluid refs by pointer equality, which works for registry fluids.
    fn is_same(&self, other: FluidRef) -> bool {
        std::ptr::eq(self.fluid_type(), other)
    }

    /// Gets the number of ticks between fluid updates.
    fn tick_delay(&self) -> u32;
    /// Gets the amount of fluid level drop when flowing horizontally.
    fn drop_off(&self) -> u8;
    /// Gets the distance from the source block for this fluid.
    fn slope_find_distance(&self) -> u8;

    /// Called every tick for fluid blocks.
    fn tick(&self, world: &World, pos: BlockPos, current_tick: u64);
    /// Called to calculate fluid spreading each tick.
    fn spread(&self, world: &World, pos: BlockPos, fluid_state: FluidState, current_tick: u64);

    /// Checks if this fluid can be replaced by another fluid.
    /// This is used to determine if a fluid can flow into a block occupied by another fluid.
    fn can_be_replaced_with(
        &self,
        fluid_state: FluidState,
        world: &World,
        pos: BlockPos,
        other_fluid: FluidRef,
        direction: Direction,
    ) -> bool;

    /// Gets the sound event ID for when this fluid is picked up with a bucket.
    fn pickup_sound(&self) -> Option<i32> {
        None
    }

    /// Gets the item that is dropped when this fluid is picked up with a bucket.
    fn bucket_item(&self) -> Option<ItemRef> {
        None
    }

    /// Called before a block is destroyed by this fluid.
    fn before_destroying_block(
        &self,
        _world: &mut World,
        _pos: BlockPos,
        _replaced: BlockStateId,
    ) {
        // default: do nothing
    }

    /// Checks if this fluid can convert to a source block at the given position.
    fn can_convert_to_source(&self, _world: &World) -> bool {
        false
    }

    /// Called when an entity is inside this fluid.
    fn entity_inside(
        &self,
        _world: &mut World,
        _pos: BlockPos,
        _entity: &mut dyn Entity,
    ) {}

    /// Gets the explosion resistance of this fluid.
    fn explosion_resistance(&self) -> f32 {
        0.0
    }
}