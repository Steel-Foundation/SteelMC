use steel_registry::{RegistryEntry, RegistryExt, item_stack::ItemStack};
use steel_utils::Identifier;

use super::behavior_registry::VANILLA_TABLES;
use super::output::StackSplitter;
use super::{
    GeneratedLoot, LOOT_TABLES, LootAsyncOutput, LootContext, LootOutput, OperationError,
    OperationResult,
};

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
    pub async fn emit_raw_async(
        &self,
        context: &mut LootContext<'_>,
        output: &mut dyn LootAsyncOutput,
    ) -> OperationResult<()> {
        let Some(_visit) = context.visited.enter(&self.key) else {
            log::warn!("Detected infinite loop in loot table {}", self.key);
            return Ok(());
        };
        let registry = context.tables.unwrap_or(&VANILLA_TABLES);
        let Some(function) = registry.async_functions.get(&self.key).copied() else {
            return Err(OperationError::MissingTable(self.key.clone()));
        };
        function(context, output).await
    }

    /// Collects and splits output into legal item stack sizes.
    pub async fn get_random_items_async(
        &self,
        context: &mut LootContext<'_>,
    ) -> OperationResult<Vec<ItemStack>> {
        // TODO: Filter items against the world's enabled feature flags before
        // stack splitting, as vanilla's createStackSplitter does.
        let mut output = StackSplitter { items: Vec::new() };
        self.emit_raw_async(context, &mut output).await?;
        Ok(output.items)
    }

    /// Generates all loot before acquiring a destination container's lock.
    pub async fn generate_for_container_async<'a>(
        &self,
        context: &'a mut LootContext<'_>,
    ) -> OperationResult<GeneratedLoot<'a>> {
        let items = self.get_random_items_async(context).await?;
        Ok(GeneratedLoot::new(items, context.rng))
    }
}

impl LootTable {
    /// Streams direct tables without I/O. World-dependent tables require the
    /// resumable entry point and fail here before producing output or RNG draws.
    pub fn emit_raw(
        &self,
        context: &mut LootContext<'_>,
        output: &mut LootOutput<'_>,
    ) -> OperationResult<()> {
        let registry = context.tables.unwrap_or(&VANILLA_TABLES);
        let Some(function) = registry.functions.get(&self.key).copied() else {
            return Err(if registry.async_functions.contains_key(&self.key) {
                OperationError::RequiresResumption(self.key.clone())
            } else {
                OperationError::MissingTable(self.key.clone())
            });
        };
        let Some(_visit) = context.visited.enter(&self.key) else {
            log::warn!("Detected infinite loop in loot table {}", self.key);
            return Ok(());
        };
        function(context, output)
    }

    /// Collects direct tables into legal stack sizes; world-dependent tables
    /// must be evaluated through a server loot operation.
    pub fn get_random_items(
        &self,
        context: &mut LootContext<'_>,
    ) -> OperationResult<Vec<ItemStack>> {
        let mut items = Vec::new();
        self.emit_raw(context, &mut |_, mut item| {
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

    /// Retains the direct table's borrowed RNG through container placement.
    pub fn generate_for_container<'a>(
        &self,
        context: &'a mut LootContext<'_>,
    ) -> OperationResult<GeneratedLoot<'a>> {
        let items = self.get_random_items(context)?;
        Ok(GeneratedLoot::new(items, context.rng))
    }
}
