use steel_utils::random::Random;

use super::{LootContext, LootModifier, LootOutput, LootResult};

type CreateItems =
    dyn Fn(&mut LootContext<'_>, &mut LootOutput<'_>) -> LootResult<()> + Send + Sync;

/// One expanded weighted entry. Its implementation is Rust code, not a loot AST.
pub struct LootCandidate {
    weight: i32,
    create: Box<CreateItems>,
    modifiers: Vec<LootModifier>,
}

impl LootCandidate {
    /// Creates a candidate using vanilla's luck-adjusted weight.
    pub fn new(
        weight: i32,
        quality: i32,
        luck: f32,
        create: impl Fn(&mut LootContext<'_>, &mut LootOutput<'_>) -> LootResult<()>
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
    pub fn add_modifier(&mut self, modifier: LootModifier) {
        self.modifiers.push(modifier);
    }

    fn create_items(
        &self,
        context: &mut LootContext<'_>,
        output: &mut LootOutput<'_>,
    ) -> LootResult<()> {
        (self.create)(context, &mut |context, mut item| {
            for modifier in &self.modifiers {
                modifier(context, &mut item)?;
            }
            output(context, item)
        })
    }

    /// Selects one expanded entry. A single positive entry consumes no random value.
    pub fn select(
        candidates: &[Self],
        context: &mut LootContext<'_>,
        output: &mut LootOutput<'_>,
    ) -> LootResult<()> {
        let mut positive = candidates.iter().filter(|candidate| candidate.weight > 0);
        let Some(first) = positive.next() else {
            return Ok(());
        };
        let Some(second) = positive.next() else {
            return first.create_items(context, output);
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
                return candidate.create_items(context, output);
            }
        }
        Ok(())
    }
}
