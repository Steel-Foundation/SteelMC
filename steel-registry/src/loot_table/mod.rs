pub use crate::{DyeColor, equipment::EquipmentSlotGroup};
use crate::{
    REGISTRY, RegistryExt, TaggedRegistryExt, blocks::block_state_ext::BlockStateExt,
    instrument::InstrumentRef, item_stack::ItemStack,
};
use rustc_hash::FxHashMap;
use steel_utils::random::Random;
use steel_utils::{BlockStateId, Identifier};

/// Picks a uniformly random index below `len`, mirroring Vanilla's
/// `random.nextInt(size)` element selection.
fn random_index<R: Random>(rng: &mut R, len: usize) -> usize {
    let bound = i32::try_from(len).unwrap_or(i32::MAX);
    rng.next_i32_bounded(bound) as usize
}

mod conditions;
mod context;
mod entries;
mod functions;
mod registry;

pub use conditions::*;
pub use context::*;
pub use entries::*;
pub use functions::*;
pub use registry::*;

#[cfg(test)]
mod tests;
