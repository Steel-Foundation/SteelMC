use super::{
    LootAsyncModifier, LootAsyncOutput, LootContext, LootFuture, ModifiedAsyncLootOutput,
    OperationResult,
};

type CreateItems = dyn for<'a> Fn(&'a mut LootContext<'_>, &'a mut dyn LootAsyncOutput) -> LootFuture<'a>
    + Send
    + Sync;

/// One expanded weighted entry. Its implementation is Rust code, not a loot AST.
pub struct AsyncLootCandidate {
    weight: i32,
    create: Box<CreateItems>,
    modifiers: Vec<LootAsyncModifier>,
}

impl AsyncLootCandidate {
    /// Creates a candidate using vanilla's luck-adjusted weight.
    pub fn new(
        weight: i32,
        quality: i32,
        luck: f32,
        create: impl for<'a> Fn(&'a mut LootContext<'_>, &'a mut dyn LootAsyncOutput) -> LootFuture<'a>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            weight: (weight as f32 + quality as f32 * luck).floor().max(0.0) as i32,
            create: Box::new(create),
            modifiers: Vec::new(),
        }
    }

    /// Adds an enclosing entry's modifier, after its child's modifiers.
    pub fn add_modifier(&mut self, modifier: LootAsyncModifier) {
        self.modifiers.push(modifier);
    }

    async fn create_items(
        &self,
        context: &mut LootContext<'_>,
        output: &mut dyn LootAsyncOutput,
    ) -> OperationResult<()> {
        let mut output = ModifiedAsyncLootOutput::new(output, &self.modifiers);
        (self.create)(context, &mut output).await
    }

    /// Selects one expanded entry. A single positive entry consumes no random value.
    pub async fn select(
        candidates: &[Self],
        context: &mut LootContext<'_>,
        output: &mut dyn LootAsyncOutput,
    ) -> OperationResult<()> {
        let mut positive = candidates.iter().filter(|candidate| candidate.weight > 0);
        let Some(first) = positive.next() else {
            return Ok(());
        };
        let Some(second) = positive.next() else {
            return first.create_items(context, output).await;
        };
        let total = positive.fold(first.weight.wrapping_add(second.weight), |sum, entry| {
            sum.wrapping_add(entry.weight)
        });
        if total == 0 {
            return Ok(());
        }
        let mut choice = context.rng.next_i32_bounded(total);
        for candidate in candidates.iter().filter(|candidate| candidate.weight > 0) {
            choice -= candidate.weight;
            if choice < 0 {
                return candidate.create_items(context, output).await;
            }
        }
        Ok(())
    }
}
