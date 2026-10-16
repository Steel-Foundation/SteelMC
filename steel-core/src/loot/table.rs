use steel_registry::{RegistryEntry, RegistryExt, item_stack::ItemStack};
use steel_utils::Identifier;

use super::behavior_registry::VANILLA_TABLES;
use super::{GeneratedLoot, LOOT_TABLES, LootContext, LootError, LootOutput, LootResult};

/// A registered loot table's identity and execution metadata.
#[derive(Debug)]
pub struct LootTable {
    /// Registry identity used for saves and compiled implementations.
    pub key: Identifier,
    /// Vanilla context parameter-set identity.
    pub param_set: Identifier,
    /// Optional named random sequence used by world-aware callers.
    pub random_sequence: Option<Identifier>,
}

/// A stable reference to a registered loot table.
pub type LootTableRef = &'static LootTable;

steel_registry::impl_registry_entry_eq!(LootTable);

impl RegistryEntry for LootTable {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        LOOT_TABLES.id_from_key(&self.key)
    }
}

impl LootTable {
    /// Streams raw stacks. A failing callable may already have emitted output.
    /// Use `get_random_items` to collect before publishing gameplay output.
    pub fn emit_raw(
        &self,
        context: &mut LootContext<'_>,
        output: &mut LootOutput<'_>,
    ) -> LootResult<()> {
        if context.visited.contains(&self.key) {
            log::warn!("Detected infinite loop in loot table {}", self.key);
            return Ok(());
        }
        let registry = context.tables.unwrap_or(&VANILLA_TABLES);
        let Some(function) = registry.functions.get(&self.key).copied() else {
            return Err(LootError::MissingTable(self.key.clone()));
        };
        context.visited.push(self.key.clone());
        let result = function(context, output);
        context.visited.pop();
        result
    }

    /// Collects raw output without normalizing stack sizes.
    pub fn get_random_items_raw(
        &self,
        context: &mut LootContext<'_>,
    ) -> LootResult<Vec<ItemStack>> {
        let mut items = Vec::new();
        self.emit_raw(context, &mut |_, item| {
            items.push(item);
            Ok(())
        })?;
        Ok(items)
    }

    /// Collects and splits output into legal item stack sizes.
    pub fn get_random_items(&self, context: &mut LootContext<'_>) -> LootResult<Vec<ItemStack>> {
        // TODO: Filter items against the world's enabled feature flags before
        // stack splitting, as vanilla's createStackSplitter does.
        let mut items = Vec::new();
        self.emit_raw(context, &mut |_, mut item| {
            // Vanilla forwards sub-maximal stacks, including empty ones. Fill
            // removes empties later, after collecting and shuffling available slots.
            let max = item.max_stack_size();
            if item.count() < max {
                items.push(item);
            } else {
                while item.count() > 0 {
                    items.push(item.split(max));
                }
            }
            Ok(())
        })?;
        Ok(items)
    }

    /// Generates all loot before acquiring a destination container's lock.
    pub fn generate_for_container<'a>(
        &self,
        context: &'a mut LootContext<'_>,
    ) -> LootResult<GeneratedLoot<'a>> {
        let items = self.get_random_items(context)?;
        Ok(GeneratedLoot::new(items, context.rng))
    }
}
