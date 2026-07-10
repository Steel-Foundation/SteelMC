//! `/execute store` result consumers.

use std::sync::Arc;

use super::super::super::{
    brigadier::{CommandNodeBuilder, CommandRedirectTarget, CommandSyntaxError},
    execution::{
        CommandResultCallback, CommandSource, ExecutionCommandSource as _, ScoreHolderWildcard,
        SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument, literal,
    },
};
use super::{objective, source_scoreboard};
use crate::scoreboard::{ScoreHolder, Scoreboard, ScoreboardError, ScoreboardObjective};

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

const EXECUTE_ROOT: CommandRedirectTarget = CommandRedirectTarget::CommandRoot;

pub(super) fn target(name: &'static str, store_result: bool) -> Builder {
    literal(name).then(
        literal("score").then(
            argument("targets", SteelArgumentType::score_holders()).then(
                argument("objective", SteelArgumentType::objective())
                    .redirects_with(EXECUTE_ROOT, move |context| {
                        store_score(context, store_result)
                    }),
            ),
        ),
    )
}

fn store_score(
    context: &SteelCommandContext<CommandSource>,
    store_result: bool,
) -> Result<CommandSource, CommandSyntaxError> {
    let scoreboard = source_scoreboard(context)?;
    let objective = objective(context, scoreboard, "objective")?;
    let holders = context.score_holders("targets", ScoreHolderWildcard::Tracked)?;
    let source = context.source();
    let server = Arc::clone(source.server());
    let domain = source.world().domain().to_owned();
    let callback = CommandResultCallback::new(move |success, result| {
        let Some(scoreboard) = server.scoreboards.get(&domain) else {
            tracing::warn!(%domain, "execute store score domain is no longer available");
            return;
        };
        let value = stored_value(store_result, success, result);
        if let Err(error) = store_score_value(scoreboard, &holders, &objective, value) {
            tracing::warn!(%error, "failed to store execute result in scoreboard");
        }
    });
    let callback = CommandResultCallback::chain(source.callback(), callback);
    Ok(source.with_callback(callback))
}

fn stored_value(store_result: bool, success: bool, result: i32) -> i32 {
    if store_result {
        result
    } else {
        i32::from(success)
    }
}

fn store_score_value(
    scoreboard: &Scoreboard,
    holders: &[ScoreHolder],
    objective: &ScoreboardObjective,
    value: i32,
) -> Result<(), ScoreboardError> {
    for holder in holders {
        scoreboard.set_score(holder, objective, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_store_creates_and_updates_each_holder() {
        let scoreboard = Scoreboard::new();
        let Ok(objective) = scoreboard.add_objective("result") else {
            panic!("objective should be created");
        };
        let holders = [ScoreHolder::new("one"), ScoreHolder::new("two")];

        assert!(store_score_value(&scoreboard, &holders, &objective, 7).is_ok());
        assert_eq!(scoreboard.score(&holders[0], &objective), Some(7));
        assert_eq!(scoreboard.score(&holders[1], &objective), Some(7));
    }

    #[test]
    fn stored_value_distinguishes_numeric_results_from_success() {
        assert_eq!(stored_value(true, true, 17), 17);
        assert_eq!(stored_value(true, false, 0), 0);
        assert_eq!(stored_value(false, true, 17), 1);
        assert_eq!(stored_value(false, false, 17), 0);
    }
}
