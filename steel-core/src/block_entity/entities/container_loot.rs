//! Shared seeded container loot generation.
//!
//! Mirrors vanilla `RandomizableContainer`: structure generation writes a
//! `LootTable` (and a nonzero `LootTableSeed`) into a container block entity's
//! NBT, and the contents are rolled once on first access. The table reference
//! is cleared after rolling so the rolled stack list is what persists.


use rand::RngExt as _;
use rand::SeedableRng;
use rand::rngs::StdRng;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::{REGISTRY, RegistryExt};
use steel_registry::loot_table::LootContext;
use steel_utils::{BlockPos, Identifier};

use crate::inventory::container::Container;

const LOOT_TABLE_TAG: &str = "LootTable";
const LOOT_TABLE_SEED_TAG: &str = "LootTableSeed";

/// Pending loot-table state for a container block entity.
#[derive(Debug, Default, Clone)]
pub struct ContainerLoot {
    loot_table: Option<Identifier>,
    seed: Option<i64>,
}

impl ContainerLoot {
    /// Reads `LootTable`/`LootTableSeed` from block entity NBT.
    #[must_use]
    pub fn load(nbt: &BorrowedNbtCompound<'_>) -> Self {
        let view: NbtCompoundView<'_, '_> = nbt.into();
        let loot_table = view
            .string(LOOT_TABLE_TAG)
            .and_then(|s| s.to_string().parse::<Identifier>().ok());
        let seed = view.long(LOOT_TABLE_SEED_TAG).filter(|&seed| seed != 0);
        Self { loot_table, seed }
    }

    /// Saves `LootTable`/`LootTableSeed` back into block entity NBT. Both tags
    /// are dropped after the loot was rolled, mirroring vanilla.
    pub fn save(&self, nbt: &mut NbtCompound) {
        if let Some(loot_table) = &self.loot_table {
            nbt.insert(LOOT_TABLE_TAG, loot_table.to_string());
        }
        if let Some(seed) = self.seed {
            nbt.insert(LOOT_TABLE_SEED_TAG, seed);
        }
    }

    /// Whether this container still has an unrolled loot table.
    #[must_use]
    pub fn has_pending_loot(&self) -> bool {
        self.loot_table.is_some()
    }

    /// The seed to roll with: the stored `LootTableSeed` if present, otherwise
    /// a world-seed + position derived fallback (vanilla draws from the live
    /// level random here, which would re-roll on every load).
    #[must_use]
    pub fn loot_seed(&self, world_seed: i64, pos: BlockPos) -> i64 {
        self.seed.unwrap_or_else(|| {
            world_seed
                .wrapping_add(i64::from(pos.x()) * 3_128_874_617)
                .wrapping_add(i64::from(pos.y()) * 6_364_136_223_846_793_005)
                .wrapping_add(i64::from(pos.z()) * 1_442_695_040_888_963_407)
        })
    }

    /// Rolls the pending loot table into `container`, mirroring vanilla
    /// `LootTable.fill`: generated stacks are shuffled into empty slots and the
    /// loot-table reference is cleared. Returns whether the container changed
    /// and callers should persist it.
    ///
    /// `loot_seed` must be deterministic per container: callers use the stored
    /// `LootTableSeed`, or a world-seed + position derived fallback instead of
    /// vanilla's live level random so repeated reloads never re-roll contents.
    pub fn populate(&mut self, loot_seed: i64, pos: BlockPos, container: &mut dyn Container) -> bool {
        let Some(loot_table) = self.loot_table.take() else {
            return false;
        };
        let Some(table) = REGISTRY.loot_tables.by_key(&loot_table) else {
            return false;
        };
        self.seed = None;

        let mut rng = StdRng::seed_from_u64(loot_seed as u64);

        let mut context = LootContext::new(&mut rng).with_origin(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        let mut stacks = table.get_random_items(&mut context);
        stacks.retain(|stack| !stack.is_empty());
        if stacks.is_empty() {
            return false;
        }

        // Vanilla fills a shuffled list of slot indices so stacks land in a
        // random selection of empty slots.
        let size = container.get_container_size();
        let mut slots: Vec<usize> = (0..size)
            .filter(|&slot| container.items()[slot].is_empty())
            .collect();
        for index in (1..slots.len()).rev() {
            slots.swap(index, rng.random_range(0..=index));
        }

        let mut changed = false;
        for (slot, stack) in slots.into_iter().zip(stacks) {
            container.set_item(slot, stack);
            changed = true;
        }
        changed
    }
}
