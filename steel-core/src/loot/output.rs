use steel_registry::item_stack::ItemStack;

use super::{LootContext, LootResult};

/// Receives raw stacks in generation order, with access to the same context.
pub type LootOutput<'a> = dyn FnMut(&mut LootContext<'_>, ItemStack) -> LootResult<()> + 'a;

/// A compiled item modifier. Plugins can supply ordinary Rust functions.
pub type LootModifier = fn(&mut LootContext<'_>, &mut ItemStack) -> LootResult<()>;

/// Applies a modifier to each emitted stack before forwarding it.
pub fn with_modifier(
    context: &mut LootContext<'_>,
    output: &mut LootOutput<'_>,
    modifier: LootModifier,
    generate: impl FnOnce(&mut LootContext<'_>, &mut LootOutput<'_>) -> LootResult<()>,
) -> LootResult<()> {
    generate(context, &mut |context, mut item| {
        modifier(context, &mut item)?;
        output(context, item)
    })
}
