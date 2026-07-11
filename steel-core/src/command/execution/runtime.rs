#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "custom runtime variants are reserved for future keyed command integrations"
    )
)]

use std::sync::Arc;

use steel_registry::{
    enchantment::EnchantmentRef, entity_type::EntityTypeRef, item_stack::ItemStack,
    timeline::TimelineRef, world_clock::WorldClockRef,
};
use steel_utils::{DowncastType, Identifier, nbt::NbtPath, translations, types::GameType};
use text_components::TextComponent;

use crate::command::brigadier::{
    CommandContext, CommandNodeBuilder, CommandRedirectTarget, CommandRuntime, CommandSyntaxError,
    ContextChain,
};

use super::{
    BiomeOrTag, BlockPredicate, ChainModifiers, CommandResultSuspension, CommandSource,
    Coordinates, ExecutionCommandSource, ExecutionControl, GameProfileArgument, IntRange,
    ItemPredicate, PermissionGroupName, ScoreHolderArgument, ScoreHolderWildcard,
    SteelArgumentType, StructureOrTagKey, WorldArgument,
    argument::{
        ComponentValue, CoordinateAxes, DomainValue, EnchantmentValue, EntityTypeValue,
        GameModeValue, IdentifierValue, ItemStackValue, NbtPathValue, ObjectiveValue,
        SteelArgumentValue, TimeValue, TimelineValue, WorldClockValue,
    },
    selector::EntitySelector,
};
use crate::{
    chunk::heightmap::HeightmapType,
    entity::{EntityAnchor, SharedEntity},
    permission::{PermissionMetadataExpression, PermissionRuleExpression},
    player::Player,
    scoreboard::ScoreHolder,
};

/// Runtime model interpreted by Steel's tick-owned command scheduler.
pub(crate) struct SteelCommandRuntime;

pub(crate) type SteelCommandContext<S> = CommandContext<S, SteelCommandRuntime>;
pub(crate) type SteelContextChain<S> = ContextChain<S, SteelCommandRuntime>;

type StandardExecutor<S> =
    dyn Fn(&SteelCommandContext<S>) -> Result<i32, CommandSyntaxError> + Send + Sync;
type SuspendedExecutor<S> = dyn Fn(&SteelCommandContext<S>) -> Result<Box<dyn CommandResultSuspension>, CommandSyntaxError>
    + Send
    + Sync;
type StandardModifier<S> =
    dyn Fn(&SteelCommandContext<S>) -> Result<Vec<S>, CommandSyntaxError> + Send + Sync;

/// A terminal executor stored in a Steel command graph.
pub(crate) enum SteelExecutor<S>
where
    S: ExecutionCommandSource,
{
    Standard(Box<StandardExecutor<S>>),
    Suspended(Box<SuspendedExecutor<S>>),
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

    /// Attaches an ordinary executor whose command result is produced across ticks.
    #[must_use]
    pub(crate) fn executes_suspended<T>(
        self,
        executor: impl Fn(&SteelCommandContext<S>) -> Result<T, CommandSyntaxError>
        + Send
        + Sync
        + 'static,
    ) -> Self
    where
        T: CommandResultSuspension,
    {
        let executor = move |context: &SteelCommandContext<S>| {
            executor(context)
                .map(|suspension| Box::new(suspension) as Box<dyn CommandResultSuspension>)
        };
        self.executes_with_executor(Arc::new(SteelExecutor::Suspended(Box::new(executor))))
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
        target: impl Into<CommandRedirectTarget>,
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
        target: impl Into<CommandRedirectTarget>,
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
        target: impl Into<CommandRedirectTarget>,
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
    fn typed_argument<T: DowncastType>(&self, name: &str) -> Option<&T> {
        self.argument(name)?.downcast_ref::<T>()
    }

    /// Returns a parsed Minecraft time argument in ticks.
    pub(crate) fn time(&self, name: &str) -> Option<i32> {
        self.typed_argument::<TimeValue>(name).map(|value| value.0)
    }

    /// Returns a parsed coordinate expression without resolving it early.
    pub(crate) fn coordinates(&self, name: &str) -> Option<Coordinates> {
        self.typed_argument::<Coordinates>(name).copied()
    }

    /// Returns a parsed entity position anchor.
    pub(crate) fn entity_anchor(&self, name: &str) -> Option<EntityAnchor> {
        self.typed_argument::<EntityAnchor>(name).copied()
    }

    pub(crate) fn swizzle(&self, name: &str) -> Option<CoordinateAxes> {
        self.typed_argument::<CoordinateAxes>(name).copied()
    }

    pub(crate) fn heightmap(&self, name: &str) -> Option<HeightmapType> {
        self.typed_argument::<HeightmapType>(name).copied()
    }

    pub(crate) fn score_holder_argument(&self, name: &str) -> Option<&ScoreHolderArgument> {
        self.typed_argument(name)
    }

    pub(crate) fn objective_name(&self, name: &str) -> Option<&str> {
        self.typed_argument::<ObjectiveValue>(name)
            .map(|value| value.0.as_ref())
    }

    pub(crate) fn int_range(&self, name: &str) -> Option<IntRange> {
        self.typed_argument::<IntRange>(name).copied()
    }

    pub(crate) fn biome_or_tag(&self, name: &str) -> Option<&BiomeOrTag> {
        self.typed_argument(name)
    }

    pub(crate) fn structure_or_tag_key(&self, name: &str) -> Option<&StructureOrTagKey> {
        self.typed_argument(name)
    }

    pub(crate) fn block_predicate(&self, name: &str) -> Option<&BlockPredicate> {
        self.typed_argument(name)
    }

    /// Returns a configured Steel domain name.
    pub(crate) fn domain(&self, name: &str) -> Option<&str> {
        self.typed_argument::<DomainValue>(name)
            .map(|value| value.0.as_ref())
    }

    pub(crate) fn world_argument(&self, name: &str) -> Option<&WorldArgument> {
        self.typed_argument(name)
    }

    /// Returns a parsed vanilla game mode.
    pub(crate) fn game_mode(&self, name: &str) -> Option<GameType> {
        self.typed_argument::<GameModeValue>(name)
            .map(|value| value.0)
    }

    pub(crate) fn entity_type(&self, name: &str) -> Option<EntityTypeRef> {
        self.typed_argument::<EntityTypeValue>(name)
            .map(|value| value.0)
    }

    pub(crate) fn enchantment(&self, name: &str) -> Option<EnchantmentRef> {
        self.typed_argument::<EnchantmentValue>(name)
            .map(|value| value.0)
    }

    pub(crate) fn item_stack(&self, name: &str) -> Option<&ItemStack> {
        self.typed_argument::<ItemStackValue>(name)
            .map(|value| &value.0)
    }

    pub(crate) fn item_predicate(&self, name: &str) -> Option<&ItemPredicate> {
        self.typed_argument(name)
    }

    pub(crate) fn text_component(&self, name: &str) -> Option<&TextComponent> {
        self.typed_argument::<ComponentValue>(name)
            .map(|value| &value.0)
    }

    pub(crate) fn nbt_path(&self, name: &str) -> Option<&NbtPath> {
        self.typed_argument::<NbtPathValue>(name)
            .map(|value| &value.0)
    }

    pub(crate) fn identifier(&self, name: &str) -> Option<&Identifier> {
        self.typed_argument::<IdentifierValue>(name)
            .map(|value| &value.0)
    }

    pub(crate) fn world_clock(&self, name: &str) -> Option<WorldClockRef> {
        self.typed_argument::<WorldClockValue>(name)
            .map(|value| value.0)
    }

    pub(crate) fn timeline(&self, name: &str) -> Option<TimelineRef> {
        self.typed_argument::<TimelineValue>(name)
            .map(|value| value.0)
    }

    pub(crate) fn entity_selector(&self, name: &str) -> Option<&EntitySelector> {
        self.typed_argument(name)
    }

    pub(crate) fn game_profile_argument(&self, name: &str) -> Option<&GameProfileArgument> {
        self.typed_argument(name)
    }

    pub(crate) fn permission_rule_expression(
        &self,
        name: &str,
    ) -> Option<&PermissionRuleExpression> {
        self.typed_argument(name)
    }

    pub(crate) fn permission_metadata_expression(
        &self,
        name: &str,
    ) -> Option<&PermissionMetadataExpression> {
        self.typed_argument(name)
    }

    pub(crate) fn permission_group(&self, name: &str) -> Option<&PermissionGroupName> {
        self.typed_argument(name)
    }
}

impl SteelCommandContext<CommandSource> {
    pub(crate) fn score_holders(
        &self,
        name: &str,
        wildcard: ScoreHolderWildcard,
    ) -> Result<Vec<ScoreHolder>, CommandSyntaxError> {
        let holders = self
            .score_holder_argument(name)
            .ok_or_else(|| missing_score_holder_argument(name))?
            .resolve(self.source(), wildcard)?;
        if holders.is_empty() {
            Err(CommandSyntaxError::dynamic(TextComponent::from(
                &translations::ARGUMENT_SCORE_HOLDER_EMPTY,
            )))
        } else {
            Ok(holders)
        }
    }

    pub(crate) fn score_holder(&self, name: &str) -> Result<ScoreHolder, CommandSyntaxError> {
        let mut holders = self.score_holders(name, ScoreHolderWildcard::Empty)?;
        Ok(holders.remove(0))
    }

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

fn missing_score_holder_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed score holder for {name} is missing from the command context"
    ))
}
