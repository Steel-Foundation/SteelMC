mod storage;
pub(crate) use storage::RandomizableContainerStorage;

use steel_registry::data_components::vanilla_components::SeededContainerLoot;

use crate::{loot::LootResult, player::Player};

// TODO: Integrate other vanilla containers beyond barrels and normal chests,
// preserving their persistence formats, including decorated pots' single item.
/// Lazy loot capability, separate from independently locked inventory storage.
/// Call unpacking without holding any container locks.
pub trait RandomizableContainer: Send + Sync {
    /// Pending table and saved seed, or `None` once the contents are generated.
    fn loot_table(&self) -> Option<SeededContainerLoot>;

    /// Replaces pending loot without evaluating it.
    fn set_loot_table(&self, loot: Option<SeededContainerLoot>);

    /// Generates pending contents; player opening supplies luck and `THIS_ENTITY`.
    fn unpack_loot_table(&self, player: Option<&Player>) -> LootResult<()>;
}
