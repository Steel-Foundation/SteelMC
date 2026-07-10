use std::sync::Arc;

use steel_registry::{
    entity_type::EntityTypeRef, timeline::TimelineRef, world_clock::WorldClockRef,
};
use steel_utils::translations;
use steel_utils::{Identifier, types::GameType};
use text_components::TextComponent;

use crate::command::brigadier::{
    CommandContext, CommandNodeBuilder, CommandRuntime, CommandSyntaxError, ContextChain, NodeId,
};

use super::{
    ChainModifiers, CommandSource, Coordinates, ExecutionCommandSource, ExecutionControl,
    SteelArgumentType, argument::SteelArgumentValue, selector::EntitySelector,
};
use crate::{
    entity::{EntityAnchor, SharedEntity},
    player::Player,
};

/// Runtime model interpreted by Steel's tick-owned command scheduler.
pub(crate) struct SteelCommandRuntime;

pub(crate) type SteelCommandContext<S> = CommandContext<S, SteelCommandRuntime>;
pub(crate) type SteelContextChain<S> = ContextChain<S, SteelCommandRuntime>;

type StandardExecutor<S> =
    dyn Fn(&SteelCommandContext<S>) -> Result<i32, CommandSyntaxError> + Send + Sync;
type StandardModifier<S> =
    dyn Fn(&SteelCommandContext<S>) -> Result<Vec<S>, CommandSyntaxError> + Send + Sync;

/// A terminal executor stored in a Steel command graph.
pub(crate) enum SteelExecutor<S>
where
    S: ExecutionCommandSource,
{
    Standard(Box<StandardExecutor<S>>),
    Custom(Arc<dyn CustomCommandExecutor<S>>),
}

/// A redirect modifier stored in a Steel command graph.
pub(crate) enum SteelModifier<S>
where
    S: ExecutionCommandSource,
{
    Standard(Box<StandardModifier<S>>),
    Custom(Arc<dyn CustomModifierExecutor<S>>),
}

/// Special terminal behavior that controls command frames or queues more work.
pub(crate) trait CustomCommandExecutor<S>: Send + Sync
where
    S: ExecutionCommandSource,
{
    fn run(
        &self,
        source: Arc<S>,
        chain: &SteelContextChain<S>,
        modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, S>,
    );
}

/// Special redirect behavior that controls command frames or queues more work.
pub(crate) trait CustomModifierExecutor<S>: Send + Sync
where
    S: ExecutionCommandSource,
{
    fn apply(
        &self,
        original_source: Arc<S>,
        sources: Vec<Arc<S>>,
        chain: &SteelContextChain<S>,
        modifiers: ChainModifiers,
        control: &mut ExecutionControl<'_, S>,
    );
}

impl<S> CommandRuntime<S> for SteelCommandRuntime
where
    S: ExecutionCommandSource,
{
    type Argument = SteelArgumentType;
    type ArgumentValue = SteelArgumentValue;
    type Executor = SteelExecutor<S>;
    type Modifier = SteelModifier<S>;
}

/// Creates a literal backed by Steel's runtime model.
pub(crate) fn literal<S>(name: impl Into<Box<str>>) -> CommandNodeBuilder<S, SteelCommandRuntime>
where
    S: ExecutionCommandSource,
{
    CommandNodeBuilder::literal(name)
}

/// Creates an argument backed by Steel's runtime model.
pub(crate) fn argument<S>(
    name: impl Into<Box<str>>,
    argument_type: impl Into<SteelArgumentType>,
) -> CommandNodeBuilder<S, SteelCommandRuntime>
where
    S: ExecutionCommandSource,
{
    CommandNodeBuilder::argument(name, argument_type.into())
}

impl<S> CommandNodeBuilder<S, SteelCommandRuntime>
where
    S: ExecutionCommandSource,
{
    /// Attaches an ordinary synchronous executor.
    #[must_use]
    pub(crate) fn executes(
        self,
        executor: impl Fn(&SteelCommandContext<S>) -> Result<i32, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.executes_with_executor(Arc::new(SteelExecutor::Standard(Box::new(executor))))
    }

    /// Attaches an internal executor with frame and queue control.
    #[must_use]
    pub(crate) fn executes_custom(self, executor: impl CustomCommandExecutor<S> + 'static) -> Self {
        self.executes_with_executor(Arc::new(SteelExecutor::Custom(Arc::new(executor))))
    }

    /// Redirects parsing and transforms the source once before continuing.
    #[must_use]
    pub(crate) fn redirects_with(
        self,
        target: NodeId,
        modifier: impl Fn(&SteelCommandContext<S>) -> Result<S, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let modifier = SteelModifier::Standard(Box::new(move |context| {
            modifier(context).map(|source| vec![source])
        }));
        self.redirects_with_modifier(target, Arc::new(modifier), false)
    }

    /// Redirects parsing and expands one source into zero or more sources.
    #[must_use]
    pub(crate) fn forks(
        self,
        target: NodeId,
        modifier: impl Fn(&SteelCommandContext<S>) -> Result<Vec<S>, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.redirects_with_modifier(
            target,
            Arc::new(SteelModifier::Standard(Box::new(modifier))),
            true,
        )
    }

    /// Redirects with an internal modifier that controls frames or queued work.
    #[must_use]
    pub(crate) fn redirects_custom(
        self,
        target: NodeId,
        modifier: impl CustomModifierExecutor<S> + 'static,
        forks: bool,
    ) -> Self {
        self.redirects_with_modifier(
            target,
            Arc::new(SteelModifier::Custom(Arc::new(modifier))),
            forks,
        )
    }
}

impl<S> SteelCommandContext<S>
where
    S: ExecutionCommandSource,
{
    /// Returns a parsed Minecraft time argument in ticks.
    pub(crate) fn time(&self, name: &str) -> Option<i32> {
        match self.argument(name) {
            Some(SteelArgumentValue::Time(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    /// Returns a parsed coordinate expression without resolving it early.
    pub(crate) fn coordinates(&self, name: &str) -> Option<Coordinates> {
        match self.argument(name) {
            Some(SteelArgumentValue::Coordinates(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    /// Returns a parsed entity position anchor.
    pub(crate) fn entity_anchor(&self, name: &str) -> Option<EntityAnchor> {
        match self.argument(name) {
            Some(SteelArgumentValue::EntityAnchor(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    /// Returns a configured Steel domain name.
    pub(crate) fn domain(&self, name: &str) -> Option<&str> {
        match self.argument(name) {
            Some(SteelArgumentValue::Domain(value)) => Some(value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    /// Returns a parsed vanilla game mode.
    pub(crate) fn game_mode(&self, name: &str) -> Option<GameType> {
        match self.argument(name) {
            Some(SteelArgumentValue::GameMode(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    pub(crate) fn entity_type(&self, name: &str) -> Option<EntityTypeRef> {
        match self.argument(name) {
            Some(SteelArgumentValue::EntityType(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    pub(crate) fn identifier(&self, name: &str) -> Option<&Identifier> {
        match self.argument(name) {
            Some(SteelArgumentValue::Identifier(value)) => Some(value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    pub(crate) fn world_clock(&self, name: &str) -> Option<WorldClockRef> {
        match self.argument(name) {
            Some(SteelArgumentValue::WorldClock(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }

    pub(crate) fn timeline(&self, name: &str) -> Option<TimelineRef> {
        match self.argument(name) {
            Some(SteelArgumentValue::Timeline(value)) => Some(*value),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::EntitySelector(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::WorldClock(_),
            )
            | None => None,
        }
    }

    pub(crate) fn entity_selector(&self, name: &str) -> Option<&EntitySelector> {
        match self.argument(name) {
            Some(SteelArgumentValue::EntitySelector(value)) => Some(value.as_ref()),
            Some(
                SteelArgumentValue::Primitive(_)
                | SteelArgumentValue::Time(_)
                | SteelArgumentValue::Coordinates(_)
                | SteelArgumentValue::EntityAnchor(_)
                | SteelArgumentValue::Domain(_)
                | SteelArgumentValue::EntityType(_)
                | SteelArgumentValue::Identifier(_)
                | SteelArgumentValue::GameMode(_)
                | SteelArgumentValue::WorldClock(_)
                | SteelArgumentValue::Timeline(_),
            )
            | None => None,
        }
    }
}

impl SteelCommandContext<CommandSource> {
    pub(crate) fn optional_entities(
        &self,
        name: &str,
    ) -> Result<Vec<SharedEntity>, CommandSyntaxError> {
        self.entity_selector(name)
            .ok_or_else(|| missing_selector_argument(name))?
            .find_entities(self.source())
    }

    pub(crate) fn entities(&self, name: &str) -> Result<Vec<SharedEntity>, CommandSyntaxError> {
        let entities = self.optional_entities(name)?;
        if entities.is_empty() {
            Err(CommandSyntaxError::dynamic(TextComponent::from(
                &translations::ARGUMENT_ENTITY_NOTFOUND_ENTITY,
            )))
        } else {
            Ok(entities)
        }
    }

    pub(crate) fn entity(&self, name: &str) -> Result<SharedEntity, CommandSyntaxError> {
        let mut entities = self.entities(name)?;
        if entities.len() != 1 {
            return Err(CommandSyntaxError::dynamic(TextComponent::from(
                &translations::ARGUMENT_ENTITY_TOOMANY,
            )));
        }
        Ok(entities.remove(0))
    }

    pub(crate) fn optional_players(
        &self,
        name: &str,
    ) -> Result<Vec<Arc<Player>>, CommandSyntaxError> {
        self.entity_selector(name)
            .ok_or_else(|| missing_selector_argument(name))?
            .find_players(self.source())
    }

    pub(crate) fn players(&self, name: &str) -> Result<Vec<Arc<Player>>, CommandSyntaxError> {
        let players = self.optional_players(name)?;
        if players.is_empty() {
            Err(CommandSyntaxError::dynamic(TextComponent::from(
                &translations::ARGUMENT_ENTITY_NOTFOUND_PLAYER,
            )))
        } else {
            Ok(players)
        }
    }

    pub(crate) fn player(&self, name: &str) -> Result<Arc<Player>, CommandSyntaxError> {
        let mut players = self.players(name)?;
        if players.len() != 1 {
            return Err(CommandSyntaxError::dynamic(TextComponent::from(
                &translations::ARGUMENT_PLAYER_TOOMANY,
            )));
        }
        Ok(players.remove(0))
    }
}

fn missing_selector_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed selector for {name} is missing from the command context"
    ))
}
