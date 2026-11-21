use std::{future::Future, pin::Pin};

use steel_registry::item_stack::ItemStack;

use super::{LootContext, OperationResult};

/// Rust continuation retained when a loot operation waits for chunk I/O.
pub type LootFuture<'a, T = ()> = Pin<Box<dyn Future<Output = OperationResult<T>> + Send + 'a>>;

/// Receives raw stacks in generation order, completing each stack's modifiers
/// before the producer proceeds to its next item or random draw.
pub trait LootAsyncOutput: Send {
    /// Applies this output stage to one raw stack.
    fn emit<'a>(&'a mut self, context: &'a mut LootContext<'_>, item: ItemStack) -> LootFuture<'a>;
}

/// A compiled modifier may retain its current item across chunk I/O.
pub type LootAsyncModifier =
    for<'a> fn(&'a mut LootContext<'_>, &'a mut ItemStack) -> LootFuture<'a>;

/// An output stage that applies entry, pool, or table modifiers in order.
pub struct ModifiedAsyncLootOutput<'a> {
    output: &'a mut dyn LootAsyncOutput,
    modifiers: &'a [LootAsyncModifier],
}

impl<'a> ModifiedAsyncLootOutput<'a> {
    /// Creates an output stage borrowing its parent and ordered modifier list.
    pub const fn new(
        output: &'a mut dyn LootAsyncOutput,
        modifiers: &'a [LootAsyncModifier],
    ) -> Self {
        Self { output, modifiers }
    }
}

impl LootAsyncOutput for ModifiedAsyncLootOutput<'_> {
    fn emit<'a>(
        &'a mut self,
        context: &'a mut LootContext<'_>,
        mut item: ItemStack,
    ) -> LootFuture<'a> {
        Box::pin(async move {
            for modifier in self.modifiers {
                modifier(context, &mut item).await?;
            }
            self.output.emit(context, item).await
        })
    }
}

/// Collects legal stack sizes after every raw modifier has completed.
pub(super) struct StackSplitter {
    pub(super) items: Vec<ItemStack>,
}

impl LootAsyncOutput for StackSplitter {
    fn emit<'a>(
        &'a mut self,
        _context: &'a mut LootContext<'_>,
        mut item: ItemStack,
    ) -> LootFuture<'a> {
        Box::pin(async move {
            // Vanilla forwards sub-maximal stacks, including empty ones. Fill
            // removes empties after collecting and shuffling available slots.
            let max = item.max_stack_size();
            if item.count() < max {
                self.items.push(item);
            } else {
                while item.count() > 0 {
                    self.items.push(item.split(max));
                }
            }
            Ok(())
        })
    }
}

/// Receives raw stacks in generation order, with access to the same context.
pub type LootOutput<'a> = dyn FnMut(&mut LootContext<'_>, ItemStack) -> OperationResult<()> + 'a;

/// A compiled item modifier. Plugins can supply ordinary Rust functions.
pub type LootModifier = fn(&mut LootContext<'_>, &mut ItemStack) -> OperationResult<()>;

/// Applies a modifier to each emitted stack before forwarding it.
pub fn with_modifier(
    context: &mut LootContext<'_>,
    output: &mut LootOutput<'_>,
    modifier: LootModifier,
    generate: impl FnOnce(&mut LootContext<'_>, &mut LootOutput<'_>) -> OperationResult<()>,
) -> OperationResult<()> {
    generate(context, &mut |context, mut item| {
        modifier(context, &mut item)?;
        output(context, item)
    })
}
