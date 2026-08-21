//! Shared loot-table state for container block entities.
//!
//! Mirrors vanilla `RandomizableContainer` / `RandomizableContainerBlockEntity`:
//! a container can carry a pending loot table that replaces its saved items
//! until first access unpacks it. Wired into the hopper; barrel, chest, and the
//! other randomizable containers adopt this when they gain loot support.

use std::mem;
use std::str::FromStr as _;

use rand::{Rng, RngExt as _};
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::{LootContext, LootTableRef};
use steel_registry::{REGISTRY, RegistryExt as _};
use steel_utils::random::{RandBridge, legacy_random::LegacyRandom};
use steel_utils::{BlockPos, Identifier};

use crate::inventory::container::Container;

/// Pending loot-table state persisted as `LootTable` / `LootTableSeed`.
#[derive(Default)]
pub struct RandomizableContainerLoot {
    loot_table: Option<Identifier>,
    loot_table_seed: i64,
}

/// A loot table taken out of [`RandomizableContainerLoot`] ready to fill a container.
pub struct PendingLoot {
    loot_table: Identifier,
    seed: i64,
}

impl RandomizableContainerLoot {
    /// Creates empty loot state with no pending table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            loot_table: None,
            loot_table_seed: 0,
        }
    }

    /// Mirrors vanilla `tryLoadLootTable`; returns true when a table is present.
    pub fn try_load(&mut self, nbt: &NbtCompoundView<'_, '_>) -> bool {
        self.loot_table = nbt
            .string("LootTable")
            .and_then(|value| Identifier::from_str(&value.to_str()).ok());
        self.loot_table_seed = nbt.long("LootTableSeed").unwrap_or(0);
        self.loot_table.is_some()
    }

    /// Mirrors vanilla `trySaveLootTable`; returns true when a table was written.
    pub fn try_save(&self, nbt: &mut NbtCompound) -> bool {
        let Some(loot_table) = &self.loot_table else {
            return false;
        };
        nbt.insert("LootTable", loot_table.to_string());
        if self.loot_table_seed != 0 {
            nbt.insert("LootTableSeed", self.loot_table_seed);
        }
        true
    }

    /// Takes the pending table, leaving no loot to unpack (vanilla `setLootTable(null)`).
    pub fn take_pending(&mut self) -> Option<PendingLoot> {
        let loot_table = self.loot_table.take()?;
        Some(PendingLoot {
            loot_table,
            seed: self.loot_table_seed,
        })
    }
}

impl PendingLoot {
    /// Mirrors vanilla `LootTable.fill`: rolls the table and spreads the result
    /// over the container's empty slots.
    ///
    /// Seed 0 rolls with runtime randomness; any other seed is deterministic.
    /// Seeded rolls draw from the vanilla legacy LCG; the pipeline still
    /// consumes them through `rand`'s range sampling, so slot arrangement is
    /// deterministic per seed but not yet bit-identical to vanilla saves
    /// (tracked on `LootTable::get_random_items`).
    pub fn fill_container(self, origin: BlockPos, container: &mut dyn Container) {
        if self.seed == 0 {
            // Vanilla falls back to random sequences / the level random here;
            // Steel has neither yet, so runtime randomness stands in.
            self.fill_with_rng(origin, container, &mut rand::rng());
        } else {
            // Vanilla `RandomSource.create(seed)`.
            let mut rng = RandBridge(LegacyRandom::from_seed(self.seed as u64));
            self.fill_with_rng(origin, container, &mut rng);
        }
    }

    fn fill_with_rng<R: Rng>(&self, origin: BlockPos, container: &mut dyn Container, rng: &mut R) {
        let table: Option<LootTableRef> = REGISTRY.loot_tables.by_key(&self.loot_table);
        let mut items = if let Some(table) = table {
            let mut ctx = LootContext::new(rng).with_origin(
                f64::from(origin.x()) + 0.5,
                f64::from(origin.y()) + 0.5,
                f64::from(origin.z()) + 0.5,
            );
            table.get_random_items(&mut ctx)
        } else {
            log::warn!("Unknown loot table {}", self.loot_table);
            Vec::new()
        };

        let mut slots: Vec<usize> = (0..container.get_container_size())
            .filter(|&slot| container.get_item(slot).is_empty())
            .collect();
        shuffle(&mut slots, rng);
        shuffle_and_split_items(&mut items, slots.len(), rng);

        for item in items {
            let Some(slot) = slots.pop() else {
                log::warn!("Tried to over-fill a container with {}", self.loot_table);
                return;
            };
            container.set_item(slot, item);
        }
    }
}

/// Fisher-Yates shuffle driven by the loot RNG (vanilla `Util.shuffle`).
fn shuffle<T, R: Rng>(values: &mut [T], rng: &mut R) {
    for i in (1..values.len()).rev() {
        values.swap(i, rng.random_range(0..=i));
    }
}

/// Mirrors vanilla `LootTable.shuffleAndSplitItems`: splits stackable results
/// until the item count approaches the free slot count, then shuffles.
fn shuffle_and_split_items<R: Rng>(
    stacks: &mut Vec<ItemStack>,
    empty_slot_count: usize,
    rng: &mut R,
) {
    let mut splittable: Vec<ItemStack> = Vec::new();
    stacks.retain_mut(|stack| {
        if stack.is_empty() {
            false
        } else if stack.count() > 1 {
            splittable.push(mem::take(stack));
            false
        } else {
            true
        }
    });

    while (empty_slot_count as i32 - stacks.len() as i32 - splittable.len() as i32) > 0
        && !splittable.is_empty()
    {
        let mut stack = splittable.remove(rng.random_range(0..splittable.len()));
        let split = stack.split(rng.random_range(1..=stack.count() / 2));
        if stack.count() > 1 && rng.random_bool(0.5) {
            splittable.push(stack);
        } else {
            stacks.push(stack);
        }
        if split.count() > 1 && rng.random_bool(0.5) {
            splittable.push(split);
        } else {
            stacks.push(split);
        }
    }

    stacks.append(&mut splittable);
    shuffle(stacks, rng);
}
