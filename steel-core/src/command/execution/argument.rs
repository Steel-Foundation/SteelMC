use std::{fmt, sync::Arc};

use crate::command::brigadier::{
    ArgumentSuggestionContext, ArgumentType, CommandArgumentParser, CommandSyntaxError,
    CommandSyntaxErrorKind, ContainsPrimitiveArgumentValue, PrimitiveArgumentValue, StringReader,
    SuggestionsBuilder,
};
use glam::DVec3;
use steel_protocol::packets::game::{
    ArgumentType as ProtocolArgumentType, SuggestionType as ProtocolSuggestionType,
};
use steel_registry::{
    ENCHANTMENT_REGISTRY, ENTITY_TYPE_REGISTRY, REGISTRY, RegistryExt as _, TIMELINE_REGISTRY,
    WORLD_CLOCK_REGISTRY, enchantment::EnchantmentRef, entity_type::EntityTypeRef,
    item_stack::ItemStack, timeline::TimelineRef, world_clock::WorldClockRef,
};
use steel_utils::{
    Downcast as _, DowncastType, DowncastTypeKey, ErasedType, Identifier,
    nbt::{NbtPath, parse_snbt_argument},
    translations,
    types::GameType,
};
use text_components::TextComponent;

use super::{
    BiomeOrTag, BlockPredicate, CommandArgumentSource, Coordinates, IntRange, ItemPredicate,
    ScoreHolderArgument, StructureOrTagKey, WorldArgument,
    biome::{parse_biome_or_tag, suggest_biomes},
    block::{parse_block_predicate, suggest_blocks},
    coordinates::{parse_block_pos, parse_rotation, parse_vec3, suggest_coordinates},
    item::{parse_item_stack, suggest_item_stack},
    item_predicate::{parse_item_predicate, suggest_item_predicate},
    nbt::parse_nbt_path,
    permission::{PermissionGroupParser, PermissionMetadataParser, PermissionRuleParser},
    profile::{GameProfileParser, GameProfileSuggestionMode},
    score::{parse_int_range, parse_score_holder, suggest_score_holders},
    selector::{EntitySelector, parse_entity_selector, suggest_entity_selector},
    structure::{parse_structure_or_tag_key, suggest_structures},
    text::validate_component_syntax,
    world::{parse_world_argument, suggest_worlds},
};
use crate::chunk::heightmap::HeightmapType;
use crate::command::protocol::protocol_argument_type;
use crate::entity::{ENTITIES, EntityAnchor};

/// Axes selected by vanilla's coordinate swizzle argument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoordinateAxes(u8);

impl CoordinateAxes {
    const X: u8 = 1;
    const Y: u8 = 2;
    const Z: u8 = 4;

    pub(crate) const fn x(self) -> bool {
        self.0 & Self::X != 0
    }

    pub(crate) const fn y(self) -> bool {
        self.0 & Self::Y != 0
    }

    pub(crate) const fn z(self) -> bool {
        self.0 & Self::Z != 0
    }

    pub(crate) const fn align(self, mut position: DVec3) -> DVec3 {
        if self.x() {
            position.x = position.x.floor();
        }
        if self.y() {
            position.y = position.y.floor();
        }
        if self.z() {
            position.z = position.z.floor();
        }
        position
    }
}

/// Typed parser contract erased by [`SteelArgumentType`].
pub(crate) trait SteelArgumentParser:
    DowncastType + fmt::Debug + PartialEq + Send + Sync + 'static
{
    /// Concrete value produced by this parser.
    type Value: DowncastType + fmt::Debug + Send + Sync + 'static;

    /// Parses one value from the command reader.
    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError>;

    /// Adds context-aware completion suggestions.
    fn list_suggestions(
        &self,
        _context: &dyn SteelArgumentSuggestionContext,
        _builder: &mut SuggestionsBuilder<'_>,
    ) {
    }

    /// Returns the vanilla command-tree parser representation.
    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>);
}

trait ErasedSteelArgumentParser: ErasedType + fmt::Debug + Send + Sync {
    fn parse_erased(
        &self,
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<SteelArgumentValue, CommandSyntaxError>;

    fn list_suggestions_erased(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    );

    fn protocol_argument_erased(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>);

    fn equals_erased(&self, other: &dyn ErasedSteelArgumentParser) -> bool;
}

impl<P> ErasedSteelArgumentParser for P
where
    P: SteelArgumentParser,
{
    fn parse_erased(
        &self,
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<SteelArgumentValue, CommandSyntaxError> {
        self.parse(reader, source).map(SteelArgumentValue::new)
    }

    fn list_suggestions_erased(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        self.list_suggestions(context, builder);
    }

    fn protocol_argument_erased(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        self.protocol_argument()
    }

    fn equals_erased(&self, other: &dyn ErasedSteelArgumentParser) -> bool {
        other.downcast_ref::<P>() == Some(self)
    }
}

/// An extensible, keyed parser stored by Steel's command runtime.
#[derive(Clone)]
pub(crate) struct SteelArgumentType(Arc<dyn ErasedSteelArgumentParser>);

impl SteelArgumentType {
    /// Erases a concrete parser while retaining its deterministic type key.
    pub(crate) fn new(parser: impl SteelArgumentParser) -> Self {
        Self(Arc::new(parser))
    }

    pub(crate) fn time(minimum: i32) -> Self {
        Self::new(TimeParser { minimum })
    }

    pub(crate) fn block_pos() -> Self {
        Self::new(BlockPosParser)
    }

    pub(crate) fn vec3(center_integers: bool) -> Self {
        Self::new(Vec3Parser { center_integers })
    }

    pub(crate) fn rotation() -> Self {
        Self::new(RotationParser)
    }

    pub(crate) fn swizzle() -> Self {
        Self::new(SwizzleParser)
    }

    pub(crate) fn heightmap() -> Self {
        Self::new(HeightmapParser)
    }

    pub(crate) fn entity_anchor() -> Self {
        Self::new(EntityAnchorParser)
    }

    pub(crate) fn entity() -> Self {
        Self::new(EntityParser {
            single: true,
            players_only: false,
        })
    }

    pub(crate) fn entities() -> Self {
        Self::new(EntityParser {
            single: false,
            players_only: false,
        })
    }

    pub(crate) fn player() -> Self {
        Self::new(EntityParser {
            single: true,
            players_only: true,
        })
    }

    pub(crate) fn players() -> Self {
        Self::new(EntityParser {
            single: false,
            players_only: true,
        })
    }

    pub(crate) fn score_holder() -> Self {
        Self::new(ScoreHolderParser { multiple: false })
    }

    pub(crate) fn non_operator_profile() -> Self {
        Self::new(GameProfileParser::new(
            GameProfileSuggestionMode::NonOperators,
        ))
    }

    pub(crate) fn game_profile() -> Self {
        Self::new(GameProfileParser::new(GameProfileSuggestionMode::All))
    }

    pub(crate) fn operator_profile() -> Self {
        Self::new(GameProfileParser::new(GameProfileSuggestionMode::Operators))
    }

    pub(crate) fn permission_rule() -> Self {
        Self::new(PermissionRuleParser::all())
    }

    pub(crate) fn user_permission_rule() -> Self {
        Self::new(PermissionRuleParser::user_owned())
    }

    pub(crate) fn group_permission_rule() -> Self {
        Self::new(PermissionRuleParser::group_owned())
    }

    pub(crate) fn permission_metadata() -> Self {
        Self::new(PermissionMetadataParser::all())
    }

    pub(crate) fn user_permission_metadata() -> Self {
        Self::new(PermissionMetadataParser::user_owned())
    }

    pub(crate) fn group_permission_metadata() -> Self {
        Self::new(PermissionMetadataParser::group_owned())
    }

    pub(crate) fn permission_group(require_existing: bool) -> Self {
        Self::new(PermissionGroupParser { require_existing })
    }

    pub(crate) fn score_holders() -> Self {
        Self::new(ScoreHolderParser { multiple: true })
    }

    pub(crate) fn objective() -> Self {
        Self::new(ObjectiveParser)
    }

    pub(crate) fn int_range() -> Self {
        Self::new(IntRangeParser)
    }

    pub(crate) fn biome_or_tag() -> Self {
        Self::new(BiomeOrTagParser)
    }

    pub(crate) fn structure_or_tag_key() -> Self {
        Self::new(StructureOrTagKeyParser)
    }

    pub(crate) fn block_predicate() -> Self {
        Self::new(BlockPredicateParser)
    }

    pub(crate) fn game_mode() -> Self {
        Self::new(GameModeParser)
    }

    pub(crate) fn domain() -> Self {
        Self::new(DomainParser)
    }

    pub(crate) fn world() -> Self {
        Self::new(WorldParser)
    }

    pub(crate) fn summonable_entity() -> Self {
        Self::new(SummonableEntityParser)
    }

    pub(crate) fn enchantment() -> Self {
        Self::new(EnchantmentParser)
    }

    pub(crate) fn item_stack() -> Self {
        Self::new(ItemStackParser)
    }

    pub(crate) fn item_predicate() -> Self {
        Self::new(ItemPredicateParser)
    }

    pub(crate) fn component() -> Self {
        Self::new(ComponentParser)
    }

    pub(crate) fn nbt_path() -> Self {
        Self::new(NbtPathParser)
    }

    pub(crate) fn storage_key() -> Self {
        Self::new(StorageKeyParser)
    }

    pub(crate) fn world_clock() -> Self {
        Self::new(WorldClockParser)
    }

    pub(crate) fn timeline(clock_argument: Option<&'static str>) -> Self {
        Self::new(TimelineParser { clock_argument })
    }

    pub(crate) fn time_marker(clock_argument: Option<&'static str>) -> Self {
        Self::new(TimeMarkerParser { clock_argument })
    }

    pub(crate) fn protocol_argument(
        &self,
    ) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        self.0.protocol_argument_erased()
    }

    #[cfg(test)]
    pub(crate) fn parser_type_key(&self) -> DowncastTypeKey {
        self.0.downcast_type_key()
    }
}

impl fmt::Debug for SteelArgumentType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SteelArgumentType")
            .field("type_key", &self.0.downcast_type_key())
            .field("parser", &self.0)
            .finish()
    }
}

impl PartialEq for SteelArgumentType {
    fn eq(&self, other: &Self) -> bool {
        self.0.equals_erased(other.0.as_ref())
    }
}

impl From<ArgumentType> for SteelArgumentType {
    fn from(argument: ArgumentType) -> Self {
        Self::new(PrimitiveParser(argument))
    }
}

trait ErasedSteelArgumentValue: ErasedType + fmt::Debug + Send + Sync {}

impl<T> ErasedSteelArgumentValue for T where T: DowncastType + fmt::Debug + Send + Sync {}

/// A keyed parsed value retained by Steel's command runtime.
#[derive(Clone)]
pub(crate) struct SteelArgumentValue(Arc<dyn ErasedSteelArgumentValue>);

impl SteelArgumentValue {
    pub(crate) fn new(value: impl DowncastType + fmt::Debug + Send + Sync) -> Self {
        Self(Arc::new(value))
    }

    pub(crate) fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }

    #[cfg(test)]
    pub(crate) fn type_key(&self) -> DowncastTypeKey {
        self.0.downcast_type_key()
    }
}

impl fmt::Debug for SteelArgumentValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SteelArgumentValue")
            .field("type_key", &self.0.downcast_type_key())
            .field("value", &self.0)
            .finish()
    }
}

impl ContainsPrimitiveArgumentValue for SteelArgumentValue {
    fn primitive_value(&self) -> Option<&PrimitiveArgumentValue> {
        self.downcast_ref::<PrimitiveArgumentValue>()
    }
}

/// Suggestion context exposed to erased Steel argument parsers.
pub(crate) trait SteelArgumentSuggestionContext {
    fn source(&self) -> &dyn CommandArgumentSource;

    fn argument(&self, name: &str) -> Option<&SteelArgumentValue>;
}

impl<S> SteelArgumentSuggestionContext for ArgumentSuggestionContext<'_, S, SteelArgumentValue>
where
    S: CommandArgumentSource,
{
    fn source(&self) -> &dyn CommandArgumentSource {
        ArgumentSuggestionContext::source(self)
    }

    fn argument(&self, name: &str) -> Option<&SteelArgumentValue> {
        ArgumentSuggestionContext::argument(self, name)
    }
}

impl<S> CommandArgumentParser<S> for SteelArgumentType
where
    S: CommandArgumentSource,
{
    type Value = SteelArgumentValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &S,
    ) -> Result<Self::Value, CommandSyntaxError> {
        self.0.parse_erased(reader, source)
    }

    fn list_suggestions(
        &self,
        context: &ArgumentSuggestionContext<'_, S, Self::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        self.0.list_suggestions_erased(context, builder);
    }
}

macro_rules! impl_downcast_type {
    ($type:ty, $key:literal) => {
        // SAFETY: This Steel-owned key uniquely identifies the concrete type in the process.
        unsafe impl DowncastType for $type {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($key);
        }
    };
}

impl_downcast_type!(PrimitiveArgumentValue, "steel:command/value/primitive");
impl_downcast_type!(Coordinates, "steel:command/value/coordinates");
impl_downcast_type!(EntityAnchor, "steel:command/value/entity_anchor");
impl_downcast_type!(CoordinateAxes, "steel:command/value/swizzle");
impl_downcast_type!(HeightmapType, "steel:command/value/heightmap");
impl_downcast_type!(EntitySelector, "steel:command/value/entity_selector");
impl_downcast_type!(ScoreHolderArgument, "steel:command/value/score_holder");
impl_downcast_type!(IntRange, "steel:command/value/int_range");
impl_downcast_type!(BiomeOrTag, "steel:command/value/biome_or_tag");
impl_downcast_type!(
    StructureOrTagKey,
    "steel:command/value/structure_or_tag_key"
);
impl_downcast_type!(BlockPredicate, "steel:command/value/block_predicate");
impl_downcast_type!(WorldArgument, "steel:command/value/world");
impl_downcast_type!(ItemPredicate, "steel:command/value/item_predicate");

macro_rules! argument_value_wrapper {
    ($name:ident($value:ty), $key:literal) => {
        #[derive(Debug)]
        pub(super) struct $name(pub(super) $value);

        impl_downcast_type!($name, $key);
    };
}

argument_value_wrapper!(TimeValue(i32), "steel:command/value/time");
argument_value_wrapper!(ObjectiveValue(Box<str>), "steel:command/value/objective");
argument_value_wrapper!(GameModeValue(GameType), "steel:command/value/game_mode");
argument_value_wrapper!(DomainValue(Box<str>), "steel:command/value/domain");
argument_value_wrapper!(
    EntityTypeValue(EntityTypeRef),
    "steel:command/value/entity_type"
);
argument_value_wrapper!(
    EnchantmentValue(EnchantmentRef),
    "steel:command/value/enchantment"
);
argument_value_wrapper!(ItemStackValue(ItemStack), "steel:command/value/item_stack");
argument_value_wrapper!(
    ComponentValue(TextComponent),
    "steel:command/value/component"
);
argument_value_wrapper!(NbtPathValue(NbtPath), "steel:command/value/nbt_path");
argument_value_wrapper!(
    IdentifierValue(Identifier),
    "steel:command/value/identifier"
);
argument_value_wrapper!(
    WorldClockValue(WorldClockRef),
    "steel:command/value/world_clock"
);
argument_value_wrapper!(TimelineValue(TimelineRef), "steel:command/value/timeline");

macro_rules! unit_argument_parser {
    (
        $parser:ident,
        $key:literal,
        $value:ty,
        parse |$reader:ident, $source:ident| $parse:block,
        suggest |$context:ident, $builder:ident| $suggest:block,
        protocol $protocol:expr
    ) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        struct $parser;

        impl_downcast_type!($parser, $key);

        impl SteelArgumentParser for $parser {
            type Value = $value;

            fn parse(
                &self,
                $reader: &mut StringReader<'_>,
                $source: &dyn CommandArgumentSource,
            ) -> Result<Self::Value, CommandSyntaxError> $parse

            fn list_suggestions(
                &self,
                $context: &dyn SteelArgumentSuggestionContext,
                $builder: &mut SuggestionsBuilder<'_>,
            ) $suggest

            fn protocol_argument(
                &self,
            ) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
                $protocol
            }
        }
    };
}

#[derive(Clone, Debug, PartialEq)]
struct PrimitiveParser(ArgumentType);

impl_downcast_type!(PrimitiveParser, "steel:command/parser/primitive");

impl SteelArgumentParser for PrimitiveParser {
    type Value = PrimitiveArgumentValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        self.0.parse_value(reader)
    }

    fn list_suggestions(
        &self,
        _context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        self.0.suggest(builder);
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (protocol_argument_type(&self.0), None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimeParser {
    minimum: i32,
}

impl_downcast_type!(TimeParser, "steel:command/parser/time");

impl SteelArgumentParser for TimeParser {
    type Value = TimeValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        parse_time(reader, self.minimum).map(TimeValue)
    }

    fn list_suggestions(
        &self,
        _context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        suggest_time_units(builder);
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (ProtocolArgumentType::Time { min: self.minimum }, None)
    }
}

unit_argument_parser!(
    BlockPosParser,
    "steel:command/parser/block_pos",
    Coordinates,
    parse | reader,
    _source | { parse_block_pos(reader) },
    suggest | _context,
    builder | {
        suggest_coordinates(builder, parse_block_pos);
    },
    protocol(ProtocolArgumentType::BlockPos, None)
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Vec3Parser {
    center_integers: bool,
}

impl_downcast_type!(Vec3Parser, "steel:command/parser/vec3");

impl SteelArgumentParser for Vec3Parser {
    type Value = Coordinates;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        parse_vec3(reader, self.center_integers)
    }

    fn list_suggestions(
        &self,
        _context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        suggest_coordinates(builder, |reader| parse_vec3(reader, self.center_integers));
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (ProtocolArgumentType::Vec3, None)
    }
}

unit_argument_parser!(
    RotationParser,
    "steel:command/parser/rotation",
    Coordinates,
    parse | reader,
    _source | { parse_rotation(reader) },
    suggest | _context,
    _builder | {},
    protocol(ProtocolArgumentType::Rotation, None)
);
unit_argument_parser!(
    SwizzleParser,
    "steel:command/parser/swizzle",
    CoordinateAxes,
    parse | reader,
    _source | { parse_swizzle(reader) },
    suggest | _context,
    _builder | {},
    protocol(ProtocolArgumentType::Swizzle, None)
);
unit_argument_parser!(
    HeightmapParser,
    "steel:command/parser/heightmap",
    HeightmapType,
    parse | reader,
    _source | { parse_heightmap(reader) },
    suggest | _context,
    builder | {
        suggest_heightmaps(builder);
    },
    protocol(ProtocolArgumentType::Heightmap, None)
);
unit_argument_parser!(
    EntityAnchorParser,
    "steel:command/parser/entity_anchor",
    EntityAnchor,
    parse | reader,
    _source | { parse_entity_anchor(reader) },
    suggest | _context,
    builder | {
        suggest_entity_anchors(builder);
    },
    protocol(ProtocolArgumentType::EntityAnchor, None)
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EntityParser {
    single: bool,
    players_only: bool,
}

impl_downcast_type!(EntityParser, "steel:command/parser/entity");

impl SteelArgumentParser for EntityParser {
    type Value = EntitySelector;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        parse_entity_selector(reader, source, self.single, self.players_only)
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        suggest_entity_selector(builder, context.source(), self.single, self.players_only);
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (
            ProtocolArgumentType::Entity {
                flags: u8::from(self.single) | (u8::from(self.players_only) << 1),
            },
            Some(ProtocolSuggestionType::AskServer),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScoreHolderParser {
    multiple: bool,
}

impl_downcast_type!(ScoreHolderParser, "steel:command/parser/score_holder");

impl SteelArgumentParser for ScoreHolderParser {
    type Value = ScoreHolderArgument;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        parse_score_holder(reader, source, self.multiple)
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        suggest_score_holders(builder, context.source());
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (
            ProtocolArgumentType::ScoreHolder {
                flags: u8::from(self.multiple),
            },
            Some(ProtocolSuggestionType::AskServer),
        )
    }
}

unit_argument_parser!(
    ObjectiveParser,
    "steel:command/parser/objective",
    ObjectiveValue,
    parse | reader,
    _source | { Ok(ObjectiveValue(reader.read_unquoted_string().into())) },
    suggest | context,
    builder | {
        let prefix = builder.remaining();
        for objective in context
            .source()
            .scoreboard_objective_names()
            .into_iter()
            .filter(|objective| objective.starts_with(prefix))
        {
            builder.suggest(objective);
        }
    },
    protocol(
        ProtocolArgumentType::Objective,
        Some(ProtocolSuggestionType::AskServer),
    )
);
unit_argument_parser!(
    IntRangeParser,
    "steel:command/parser/int_range",
    IntRange,
    parse | reader,
    _source | { parse_int_range(reader) },
    suggest | _context,
    _builder | {},
    protocol(ProtocolArgumentType::IntRange, None)
);
unit_argument_parser!(
    BiomeOrTagParser,
    "steel:command/parser/biome_or_tag",
    BiomeOrTag,
    parse | reader,
    _source | { parse_biome_or_tag(reader) },
    suggest | _context,
    builder | {
        suggest_biomes(builder);
    },
    protocol(
        ProtocolArgumentType::ResourceOrTag {
            identifier: "minecraft:worldgen/biome",
        },
        Some(ProtocolSuggestionType::AskServer),
    )
);
unit_argument_parser!(
    StructureOrTagKeyParser,
    "steel:command/parser/structure_or_tag_key",
    StructureOrTagKey,
    parse | reader,
    _source | { parse_structure_or_tag_key(reader) },
    suggest | _context,
    builder | {
        suggest_structures(builder);
    },
    protocol(
        ProtocolArgumentType::ResourceOrTagKey {
            identifier: "minecraft:worldgen/structure",
        },
        Some(ProtocolSuggestionType::AskServer),
    )
);
unit_argument_parser!(
    BlockPredicateParser,
    "steel:command/parser/block_predicate",
    BlockPredicate,
    parse | reader,
    _source | { parse_block_predicate(reader) },
    suggest | _context,
    builder | {
        suggest_blocks(builder);
    },
    protocol(ProtocolArgumentType::BlockPredicate, None)
);
unit_argument_parser!(
    GameModeParser,
    "steel:command/parser/game_mode",
    GameModeValue,
    parse | reader,
    _source | { parse_game_mode(reader).map(GameModeValue) },
    suggest | _context,
    builder | {
        suggest_game_modes(builder);
    },
    protocol(ProtocolArgumentType::Gamemode, None)
);
unit_argument_parser!(
    DomainParser,
    "steel:command/parser/domain",
    DomainValue,
    parse | reader,
    source | { parse_domain(reader, source).map(DomainValue) },
    suggest | context,
    builder | {
        let prefix = builder.remaining();
        for domain in context
            .source()
            .domain_names()
            .into_iter()
            .filter(|domain| domain.starts_with(prefix))
        {
            builder.suggest(domain);
        }
    },
    protocol(
        ProtocolArgumentType::ResourceLocation,
        Some(ProtocolSuggestionType::AskServer),
    )
);
unit_argument_parser!(
    WorldParser,
    "steel:command/parser/world",
    WorldArgument,
    parse | reader,
    _source | { parse_world_argument(reader) },
    suggest | context,
    builder | {
        suggest_worlds(builder, context.source());
    },
    protocol(
        ProtocolArgumentType::Dimension,
        Some(ProtocolSuggestionType::AskServer),
    )
);
unit_argument_parser!(
    SummonableEntityParser,
    "steel:command/parser/summonable_entity",
    EntityTypeValue,
    parse | reader,
    _source | { parse_summonable_entity(reader).map(EntityTypeValue) },
    suggest | _context,
    builder | {
        suggest_resources(
            REGISTRY
                .entity_types
                .iter()
                .filter(|(_, entity_type)| can_summon(entity_type))
                .map(|(_, entity_type)| &entity_type.key),
            builder,
        );
    },
    protocol(
        ProtocolArgumentType::Resource {
            identifier: "minecraft:entity_type",
        },
        Some(ProtocolSuggestionType::SummonableEntities),
    )
);
unit_argument_parser!(
    EnchantmentParser,
    "steel:command/parser/enchantment",
    EnchantmentValue,
    parse | reader,
    _source | {
        let key = parse_identifier(reader)?;
        REGISTRY.enchantments.by_key(&key).map_or_else(
            || Err(unknown_resource(reader, &key, &ENCHANTMENT_REGISTRY)),
            |enchantment| Ok(EnchantmentValue(enchantment)),
        )
    },
    suggest | _context,
    builder | {
        suggest_resources(
            REGISTRY
                .enchantments
                .iter()
                .map(|(_, enchantment)| &enchantment.key),
            builder,
        );
    },
    protocol(
        ProtocolArgumentType::Resource {
            identifier: "minecraft:enchantment",
        },
        None,
    )
);
unit_argument_parser!(
    ItemStackParser,
    "steel:command/parser/item_stack",
    ItemStackValue,
    parse | reader,
    _source | { parse_item_stack(reader).map(ItemStackValue) },
    suggest | _context,
    builder | {
        suggest_item_stack(builder);
    },
    protocol(
        ProtocolArgumentType::ItemStack,
        Some(ProtocolSuggestionType::AskServer),
    )
);
unit_argument_parser!(
    ItemPredicateParser,
    "steel:command/parser/item_predicate",
    ItemPredicate,
    parse | reader,
    _source | { parse_item_predicate(reader) },
    suggest | _context,
    builder | {
        suggest_item_predicate(builder);
    },
    protocol(
        ProtocolArgumentType::ItemPredicate,
        Some(ProtocolSuggestionType::AskServer),
    )
);
unit_argument_parser!(
    ComponentParser,
    "steel:command/parser/component",
    ComponentValue,
    parse | reader,
    _source | { parse_component(reader).map(ComponentValue) },
    suggest | _context,
    _builder | {},
    protocol(ProtocolArgumentType::Component, None)
);
unit_argument_parser!(
    NbtPathParser,
    "steel:command/parser/nbt_path",
    NbtPathValue,
    parse | reader,
    _source | { parse_nbt_path(reader).map(NbtPathValue) },
    suggest | _context,
    _builder | {},
    protocol(ProtocolArgumentType::NbtPath, None)
);
unit_argument_parser!(
    StorageKeyParser,
    "steel:command/parser/storage_key",
    IdentifierValue,
    parse | reader,
    _source | { parse_identifier(reader).map(IdentifierValue) },
    suggest | context,
    builder | {
        suggest_storage_keys(context.source(), builder);
    },
    protocol(
        ProtocolArgumentType::ResourceLocation,
        Some(ProtocolSuggestionType::AskServer),
    )
);
unit_argument_parser!(
    WorldClockParser,
    "steel:command/parser/world_clock",
    WorldClockValue,
    parse | reader,
    _source | {
        let key = parse_identifier(reader)?;
        REGISTRY.world_clocks.by_key(&key).map_or_else(
            || Err(unknown_resource(reader, &key, &WORLD_CLOCK_REGISTRY)),
            |clock| Ok(WorldClockValue(clock)),
        )
    },
    suggest | _context,
    builder | {
        suggest_resources(
            REGISTRY.world_clocks.iter().map(|(_, clock)| &clock.key),
            builder,
        );
    },
    protocol(
        ProtocolArgumentType::Resource {
            identifier: "minecraft:world_clock",
        },
        None,
    )
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimelineParser {
    clock_argument: Option<&'static str>,
}

impl_downcast_type!(TimelineParser, "steel:command/parser/timeline");

impl SteelArgumentParser for TimelineParser {
    type Value = TimelineValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        let key = parse_identifier(reader)?;
        REGISTRY.timelines.by_key(&key).map_or_else(
            || Err(unknown_resource(reader, &key, &TIMELINE_REGISTRY)),
            |timeline| Ok(TimelineValue(timeline)),
        )
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        let Some(clock) = selected_clock(context, self.clock_argument) else {
            return;
        };
        suggest_resources(
            REGISTRY
                .timelines
                .iter()
                .filter(|(_, timeline)| timeline.clock == clock)
                .map(|(_, timeline)| &timeline.key),
            builder,
        );
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (
            ProtocolArgumentType::Resource {
                identifier: "minecraft:timeline",
            },
            Some(ProtocolSuggestionType::AskServer),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimeMarkerParser {
    clock_argument: Option<&'static str>,
}

impl_downcast_type!(TimeMarkerParser, "steel:command/parser/time_marker");

impl SteelArgumentParser for TimeMarkerParser {
    type Value = IdentifierValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        parse_identifier(reader).map(IdentifierValue)
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        let Some(clock) = selected_clock(context, self.clock_argument) else {
            return;
        };
        suggest_resources(
            REGISTRY
                .timelines
                .iter()
                .filter(|(_, timeline)| timeline.clock == clock)
                .flat_map(|(_, timeline)| timeline.time_markers)
                .filter(|marker| marker.show_in_commands == Some(true))
                .map(|marker| &marker.key),
            builder,
        );
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (
            ProtocolArgumentType::ResourceLocation,
            Some(ProtocolSuggestionType::AskServer),
        )
    }
}

fn selected_clock(
    context: &dyn SteelArgumentSuggestionContext,
    clock_argument: Option<&str>,
) -> Option<WorldClockRef> {
    let Some(clock_argument) = clock_argument else {
        return context.source().default_world_clock();
    };
    context
        .argument(clock_argument)?
        .downcast_ref::<WorldClockValue>()
        .map(|clock| clock.0)
}

fn parse_component(reader: &mut StringReader<'_>) -> Result<TextComponent, CommandSyntaxError> {
    let start = reader.checkpoint();
    let (tag, consumed) = parse_snbt_argument(reader.remaining()).map_err(|error| {
        reader.advance_bytes(error.cursor());
        component_snbt_error(reader, error.component())
    })?;
    if !reader.advance_bytes(consumed) {
        return Err(component_snbt_error(
            reader,
            "Invalid text component cursor",
        ));
    }

    let component = TextComponent::try_from_nbt(&tag).map_err(|error| {
        reader.restore(start);
        invalid_component(reader, error.to_string())
    })?;
    validate_component_syntax(&component).map_err(|error| {
        reader.restore(start);
        invalid_component(reader, error)
    })?;
    Ok(component)
}

fn component_snbt_error(
    reader: &StringReader<'_>,
    message: impl Into<TextComponent>,
) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message.into())))
}

fn invalid_component(reader: &StringReader<'_>, message: String) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        translations::ARGUMENT_COMPONENT_INVALID
            .message([message])
            .component(),
    )))
}

fn parse_swizzle(reader: &mut StringReader<'_>) -> Result<CoordinateAxes, CommandSyntaxError> {
    let mut axes = CoordinateAxes::default();
    while reader.can_read() && reader.peek() != Some(' ') {
        let bit = match reader.read() {
            Some('x') => CoordinateAxes::X,
            Some('y') => CoordinateAxes::Y,
            Some('z') => CoordinateAxes::Z,
            Some(_) | None => return Err(invalid_swizzle(reader)),
        };
        if axes.0 & bit != 0 {
            return Err(invalid_swizzle(reader));
        }
        axes.0 |= bit;
    }
    Ok(axes)
}

fn invalid_swizzle(reader: &StringReader<'_>) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        TextComponent::from(&translations::ARGUMENTS_SWIZZLE_INVALID),
    )))
}

fn parse_heightmap(reader: &mut StringReader<'_>) -> Result<HeightmapType, CommandSyntaxError> {
    let raw = reader.read_unquoted_string();
    match raw.to_ascii_lowercase().as_str() {
        "world_surface" => Ok(HeightmapType::WorldSurface),
        "motion_blocking" => Ok(HeightmapType::MotionBlocking),
        "motion_blocking_no_leaves" => Ok(HeightmapType::MotionBlockingNoLeaves),
        "ocean_floor" => Ok(HeightmapType::OceanFloor),
        _ => {
            let message = translations::ARGUMENT_ENUM_INVALID
                .message([raw.to_owned()])
                .component();
            Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message))))
        }
    }
}

fn suggest_heightmaps(builder: &mut SuggestionsBuilder<'_>) {
    const HEIGHTMAPS: &[&str] = &[
        "world_surface",
        "motion_blocking",
        "motion_blocking_no_leaves",
        "ocean_floor",
    ];
    for heightmap in HEIGHTMAPS {
        if heightmap.starts_with(builder.remaining_lowercase()) {
            builder.suggest(*heightmap);
        }
    }
}

fn parse_entity_anchor(reader: &mut StringReader<'_>) -> Result<EntityAnchor, CommandSyntaxError> {
    let start = reader.checkpoint();
    let name = reader.read_unquoted_string();
    match name {
        "feet" => Ok(EntityAnchor::Feet),
        "eyes" => Ok(EntityAnchor::Eyes),
        _ => {
            reader.restore(start);
            let message = translations::ARGUMENT_ANCHOR_INVALID
                .message([name.to_owned()])
                .component();
            Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message))))
        }
    }
}

fn suggest_entity_anchors(builder: &mut SuggestionsBuilder<'_>) {
    let prefix = builder.remaining_lowercase().to_owned();
    for anchor in ["feet", "eyes"] {
        if anchor.starts_with(&prefix) {
            builder.suggest(anchor);
        }
    }
}

fn parse_game_mode(reader: &mut StringReader<'_>) -> Result<GameType, CommandSyntaxError> {
    let name = reader.read_unquoted_string();
    let game_mode = match name {
        "survival" => GameType::Survival,
        "creative" => GameType::Creative,
        "adventure" => GameType::Adventure,
        "spectator" => GameType::Spectator,
        _ => {
            let message = translations::ARGUMENT_GAMEMODE_INVALID
                .message([name.to_owned()])
                .component();
            return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message))));
        }
    };
    Ok(game_mode)
}

fn suggest_game_modes(builder: &mut SuggestionsBuilder<'_>) {
    let prefix = builder.remaining_lowercase().to_owned();
    for game_mode in [
        GameType::Survival,
        GameType::Creative,
        GameType::Adventure,
        GameType::Spectator,
    ] {
        let name = game_mode.name();
        if name.starts_with(&prefix) {
            builder.suggest(name);
        }
    }
}

fn parse_summonable_entity(
    reader: &mut StringReader<'_>,
) -> Result<EntityTypeRef, CommandSyntaxError> {
    let key = parse_identifier(reader)?;
    let Some(entity_type) = REGISTRY.entity_types.by_key(&key) else {
        return Err(unknown_resource(reader, &key, &ENTITY_TYPE_REGISTRY));
    };
    if can_summon(entity_type) {
        return Ok(entity_type);
    }
    let message = translations::ENTITY_NOT_SUMMONABLE
        .message([key.to_string()])
        .component();
    Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message))))
}

fn can_summon(entity_type: EntityTypeRef) -> bool {
    entity_type.summonable
        && ENTITIES
            .get()
            .is_some_and(|registry| registry.has_factory(entity_type))
}

fn parse_domain<S>(
    reader: &mut StringReader<'_>,
    source: &S,
) -> Result<Box<str>, CommandSyntaxError>
where
    S: CommandArgumentSource + ?Sized,
{
    let domain = reader.read_unquoted_string();
    if source.domain_exists(domain) {
        return Ok(domain.into());
    }
    Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        TextComponent::from(format!("Unknown domain {domain}")),
    ))))
}

pub(super) fn parse_identifier(
    reader: &mut StringReader<'_>,
) -> Result<Identifier, CommandSyntaxError> {
    let start = reader.checkpoint();
    let start_byte = reader.read_so_far().len();
    while reader.peek().is_some_and(is_allowed_in_identifier) {
        reader.skip();
    }
    let raw = &reader.read_so_far()[start_byte..];
    let (namespace, path) =
        raw.split_once(':')
            .map_or((Identifier::VANILLA_NAMESPACE, raw), |(namespace, path)| {
                if namespace.is_empty() {
                    (Identifier::VANILLA_NAMESPACE, path)
                } else {
                    (namespace, path)
                }
            });
    if namespace != ".."
        && Identifier::validate_namespace(namespace)
        && Identifier::validate_path(path)
    {
        return Ok(Identifier::new(namespace.to_owned(), path.to_owned()));
    }

    reader.restore(start);
    Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        TextComponent::from(&translations::ARGUMENT_ID_INVALID),
    ))))
}

const fn is_allowed_in_identifier(character: char) -> bool {
    character.is_ascii_digit()
        || character.is_ascii_lowercase()
        || matches!(character, '_' | ':' | '/' | '.' | '-')
}

pub(super) fn unknown_resource(
    reader: &StringReader<'_>,
    key: &Identifier,
    registry: &Identifier,
) -> CommandSyntaxError {
    let message = translations::ARGUMENT_RESOURCE_NOT_FOUND
        .message([key.to_string(), registry.to_string()])
        .component();
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message)))
}

fn suggest_resources<'a>(
    resources: impl Iterator<Item = &'a Identifier>,
    builder: &mut SuggestionsBuilder<'_>,
) {
    let contents = builder.remaining_lowercase();
    let has_namespace = contents.contains(':');
    let suggestions = resources.filter_map(|resource| {
        let full_name = resource.to_string();
        let matches = if has_namespace {
            matches_substring(contents, &full_name)
        } else {
            matches_substring(contents, resource.namespace.as_ref())
                || matches_substring(contents, resource.path.as_ref())
        };
        matches.then_some(full_name)
    });
    let suggestions = suggestions.collect::<Vec<_>>();
    for suggestion in suggestions {
        builder.suggest(suggestion);
    }
}

fn suggest_storage_keys<S>(source: &S, builder: &mut SuggestionsBuilder<'_>)
where
    S: CommandArgumentSource + ?Sized,
{
    let keys = source
        .command_storage_keys()
        .into_iter()
        .filter_map(|key| key.parse::<Identifier>().ok())
        .collect::<Vec<_>>();
    suggest_resources(keys.iter(), builder);
}

pub(super) fn matches_substring(pattern: &str, input: &str) -> bool {
    if input.starts_with(pattern) {
        return true;
    }
    input.char_indices().any(|(index, character)| {
        matches!(character, '.' | '_' | '/')
            && input[index + character.len_utf8()..].starts_with(pattern)
    })
}

pub(super) fn identifier_matches(pattern: &str, identifier: &Identifier) -> bool {
    if pattern.contains(':') {
        matches_substring(pattern, &identifier.to_string())
    } else {
        matches_substring(pattern, identifier.namespace.as_ref())
            || matches_substring(pattern, identifier.path.as_ref())
    }
}

fn parse_time(reader: &mut StringReader<'_>, minimum: i32) -> Result<i32, CommandSyntaxError> {
    let value = reader.read_float()?;
    let unit = reader.read_unquoted_string();
    let factor = match unit {
        "d" => 24_000.0,
        "s" => 20.0,
        "t" | "" => 1.0,
        _ => {
            return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
                TextComponent::from(&translations::ARGUMENT_TIME_INVALID_UNIT),
            ))));
        }
    };
    let ticks = java_round(value * factor);
    if ticks < minimum {
        let message = translations::ARGUMENT_TIME_TICK_COUNT_TOO_LOW
            .message([minimum.to_string(), ticks.to_string()])
            .component();
        return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message))));
    }
    Ok(ticks)
}

fn suggest_time_units(builder: &mut SuggestionsBuilder<'_>) {
    let mut reader = StringReader::new(builder.remaining());
    if reader.read_float().is_err() {
        return;
    }
    let number = reader.read_so_far();
    let unit = reader.read_unquoted_string();
    for candidate in ["d", "s", "t"] {
        if candidate.starts_with(unit) {
            builder.suggest(format!("{number}{candidate}"));
        }
    }
}

fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}
