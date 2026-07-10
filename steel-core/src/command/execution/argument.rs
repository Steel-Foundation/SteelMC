use crate::command::brigadier::{
    ArgumentSuggestionContext, ArgumentType, CommandArgumentParser, CommandSyntaxError,
    CommandSyntaxErrorKind, ContainsPrimitiveArgumentValue, PrimitiveArgumentValue, StringReader,
    SuggestionsBuilder,
};
use glam::DVec3;
use steel_registry::{
    ENCHANTMENT_REGISTRY, ENTITY_TYPE_REGISTRY, REGISTRY, RegistryExt as _, TIMELINE_REGISTRY,
    WORLD_CLOCK_REGISTRY, enchantment::EnchantmentRef, entity_type::EntityTypeRef,
    item_stack::ItemStack, timeline::TimelineRef, world_clock::WorldClockRef,
};
use steel_utils::translations;
use steel_utils::{Identifier, types::GameType};
use text_components::TextComponent;

use super::{
    Coordinates, ExecutionCommandSource, ItemPredicate,
    coordinates::{parse_block_pos, parse_rotation, parse_vec3, suggest_coordinates},
    item::{parse_item_stack, suggest_item_stack},
    item_predicate::{parse_item_predicate, suggest_item_predicate},
    selector::{EntitySelector, parse_entity_selector, suggest_entity_selector},
};
use crate::chunk::heightmap::HeightmapType;
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

/// An argument parser stored by Steel's command runtime.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SteelArgumentType {
    /// One of Brigadier's built-in primitive parsers.
    Primitive(ArgumentType),
    /// A Minecraft duration measured in ticks with an optional unit suffix.
    Time { minimum: i32 },
    /// A three-dimensional block position.
    BlockPos,
    /// A three-dimensional position.
    Vec3 { center_integers: bool },
    /// A yaw and pitch rotation.
    Rotation,
    /// A unique set of coordinate axes.
    Swizzle,
    /// A live-world heightmap type.
    Heightmap,
    /// An entity's feet or eyes position.
    EntityAnchor,
    /// A deferred entity selector using vanilla's entity argument flags.
    Entity { single: bool, players_only: bool },
    /// One of vanilla's four game modes.
    GameMode,
    /// A configured Steel domain.
    Domain,
    /// A summonable entity type backed by a Steel entity factory.
    SummonableEntity,
    /// A registered enchantment.
    Enchantment,
    /// An item and supported data-component patch.
    ItemStack,
    /// A decoded vanilla item predicate.
    ItemPredicate,
    /// A registered world clock.
    WorldClock,
    /// A registered timeline, suggested only when it uses the selected clock.
    Timeline {
        clock_argument: Option<&'static str>,
    },
    /// A resource location naming a command-visible marker for the selected clock.
    TimeMarker {
        clock_argument: Option<&'static str>,
    },
}

impl SteelArgumentType {
    pub(crate) const fn time(minimum: i32) -> Self {
        Self::Time { minimum }
    }

    pub(crate) const fn block_pos() -> Self {
        Self::BlockPos
    }

    pub(crate) const fn vec3(center_integers: bool) -> Self {
        Self::Vec3 { center_integers }
    }

    pub(crate) const fn rotation() -> Self {
        Self::Rotation
    }

    pub(crate) const fn swizzle() -> Self {
        Self::Swizzle
    }

    pub(crate) const fn heightmap() -> Self {
        Self::Heightmap
    }

    pub(crate) const fn entity_anchor() -> Self {
        Self::EntityAnchor
    }

    pub(crate) const fn entity() -> Self {
        Self::Entity {
            single: true,
            players_only: false,
        }
    }

    pub(crate) const fn entities() -> Self {
        Self::Entity {
            single: false,
            players_only: false,
        }
    }

    pub(crate) const fn player() -> Self {
        Self::Entity {
            single: true,
            players_only: true,
        }
    }

    pub(crate) const fn players() -> Self {
        Self::Entity {
            single: false,
            players_only: true,
        }
    }

    pub(crate) const fn game_mode() -> Self {
        Self::GameMode
    }

    pub(crate) const fn domain() -> Self {
        Self::Domain
    }

    pub(crate) const fn summonable_entity() -> Self {
        Self::SummonableEntity
    }

    pub(crate) const fn enchantment() -> Self {
        Self::Enchantment
    }

    pub(crate) const fn item_stack() -> Self {
        Self::ItemStack
    }

    pub(crate) const fn item_predicate() -> Self {
        Self::ItemPredicate
    }

    pub(crate) const fn world_clock() -> Self {
        Self::WorldClock
    }

    pub(crate) const fn timeline(clock_argument: Option<&'static str>) -> Self {
        Self::Timeline { clock_argument }
    }

    pub(crate) const fn time_marker(clock_argument: Option<&'static str>) -> Self {
        Self::TimeMarker { clock_argument }
    }
}

impl From<ArgumentType> for SteelArgumentType {
    fn from(argument: ArgumentType) -> Self {
        Self::Primitive(argument)
    }
}

/// A parsed argument retained by Steel's command runtime.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SteelArgumentValue {
    /// A value produced by a Brigadier primitive parser.
    Primitive(PrimitiveArgumentValue),
    /// A Minecraft duration resolved to ticks.
    Time(i32),
    /// A coordinate expression retained until command execution.
    Coordinates(Coordinates),
    /// A parsed entity position anchor.
    EntityAnchor(EntityAnchor),
    /// Axes selected by a coordinate swizzle.
    Swizzle(CoordinateAxes),
    /// A parsed live-world heightmap type.
    Heightmap(HeightmapType),
    /// A source-independent entity selector retained until command execution.
    EntitySelector(Box<EntitySelector>),
    /// A parsed vanilla game mode.
    GameMode(GameType),
    /// A configured Steel domain name.
    Domain(Box<str>),
    /// A resolved summonable entity type.
    EntityType(EntityTypeRef),
    /// A resolved registered enchantment.
    Enchantment(EnchantmentRef),
    /// A parsed item stack with a count of one.
    ItemStack(ItemStack),
    /// A parsed item predicate ready for infallible matching.
    ItemPredicate(ItemPredicate),
    /// A parsed resource location.
    Identifier(Identifier),
    /// A resolved registered world clock.
    WorldClock(WorldClockRef),
    /// A resolved registered timeline.
    Timeline(TimelineRef),
}

impl ContainsPrimitiveArgumentValue for SteelArgumentValue {
    fn primitive_value(&self) -> Option<&PrimitiveArgumentValue> {
        match self {
            Self::Primitive(value) => Some(value),
            Self::Time(_)
            | Self::Coordinates(_)
            | Self::EntityAnchor(_)
            | Self::Swizzle(_)
            | Self::Heightmap(_)
            | Self::EntitySelector(_)
            | Self::GameMode(_)
            | Self::Domain(_)
            | Self::EntityType(_)
            | Self::Enchantment(_)
            | Self::ItemStack(_)
            | Self::ItemPredicate(_)
            | Self::Identifier(_)
            | Self::WorldClock(_)
            | Self::Timeline(_) => None,
        }
    }
}

impl<S> CommandArgumentParser<S> for SteelArgumentType
where
    S: ExecutionCommandSource,
{
    type Value = SteelArgumentValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &S,
    ) -> Result<Self::Value, CommandSyntaxError> {
        match self {
            Self::Primitive(argument) => argument
                .parse_value(reader)
                .map(SteelArgumentValue::Primitive),
            Self::Time { minimum } => parse_time(reader, *minimum).map(SteelArgumentValue::Time),
            Self::BlockPos => parse_block_pos(reader).map(SteelArgumentValue::Coordinates),
            Self::Vec3 { center_integers } => {
                parse_vec3(reader, *center_integers).map(SteelArgumentValue::Coordinates)
            }
            Self::Rotation => parse_rotation(reader).map(SteelArgumentValue::Coordinates),
            Self::Swizzle => parse_swizzle(reader).map(SteelArgumentValue::Swizzle),
            Self::Heightmap => parse_heightmap(reader).map(SteelArgumentValue::Heightmap),
            Self::EntityAnchor => parse_entity_anchor(reader).map(SteelArgumentValue::EntityAnchor),
            Self::Entity {
                single,
                players_only,
            } => parse_entity_selector(reader, source, *single, *players_only)
                .map(Box::new)
                .map(SteelArgumentValue::EntitySelector),
            Self::GameMode => parse_game_mode(reader).map(SteelArgumentValue::GameMode),
            Self::Domain => parse_domain(reader, source).map(SteelArgumentValue::Domain),
            Self::SummonableEntity => {
                parse_summonable_entity(reader).map(SteelArgumentValue::EntityType)
            }
            Self::Enchantment => {
                let key = parse_identifier(reader)?;
                REGISTRY.enchantments.by_key(&key).map_or_else(
                    || Err(unknown_resource(reader, &key, &ENCHANTMENT_REGISTRY)),
                    |enchantment| Ok(SteelArgumentValue::Enchantment(enchantment)),
                )
            }
            Self::ItemStack => parse_item_stack(reader).map(SteelArgumentValue::ItemStack),
            Self::ItemPredicate => {
                parse_item_predicate(reader).map(SteelArgumentValue::ItemPredicate)
            }
            Self::WorldClock => {
                let key = parse_identifier(reader)?;
                REGISTRY.world_clocks.by_key(&key).map_or_else(
                    || Err(unknown_resource(reader, &key, &WORLD_CLOCK_REGISTRY)),
                    |clock| Ok(SteelArgumentValue::WorldClock(clock)),
                )
            }
            Self::Timeline { .. } => {
                let key = parse_identifier(reader)?;
                REGISTRY.timelines.by_key(&key).map_or_else(
                    || Err(unknown_resource(reader, &key, &TIMELINE_REGISTRY)),
                    |timeline| Ok(SteelArgumentValue::Timeline(timeline)),
                )
            }
            Self::TimeMarker { .. } => parse_identifier(reader).map(SteelArgumentValue::Identifier),
        }
    }

    fn list_suggestions(
        &self,
        context: &ArgumentSuggestionContext<'_, S, Self::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        match self {
            Self::Primitive(argument) => argument.suggest(builder),
            Self::Time { .. } => suggest_time_units(builder),
            Self::BlockPos => suggest_coordinates(builder, parse_block_pos),
            Self::Vec3 { center_integers } => {
                suggest_coordinates(builder, |reader| parse_vec3(reader, *center_integers));
            }
            Self::Rotation | Self::Swizzle => {}
            Self::Heightmap => suggest_heightmaps(builder),
            Self::EntityAnchor => suggest_entity_anchors(builder),
            Self::Entity {
                single,
                players_only,
            } => suggest_entity_selector(builder, context.source(), *single, *players_only),
            Self::GameMode => suggest_game_modes(builder),
            Self::Domain => {
                let prefix = builder.remaining();
                for domain in context
                    .source()
                    .domain_names()
                    .into_iter()
                    .filter(|domain| domain.starts_with(prefix))
                {
                    builder.suggest(domain);
                }
            }
            Self::SummonableEntity => {
                suggest_resources(
                    REGISTRY
                        .entity_types
                        .iter()
                        .filter(|(_, entity_type)| can_summon(entity_type))
                        .map(|(_, entity_type)| &entity_type.key),
                    builder,
                );
            }
            Self::Enchantment => {
                suggest_resources(
                    REGISTRY
                        .enchantments
                        .iter()
                        .map(|(_, enchantment)| &enchantment.key),
                    builder,
                );
            }
            Self::ItemStack => suggest_item_stack(builder),
            Self::ItemPredicate => suggest_item_predicate(builder),
            Self::WorldClock => {
                suggest_resources(
                    REGISTRY.world_clocks.iter().map(|(_, clock)| &clock.key),
                    builder,
                );
            }
            Self::Timeline { clock_argument } => {
                let Some(clock) = selected_clock(context, *clock_argument) else {
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
            Self::TimeMarker { clock_argument } => {
                let Some(clock) = selected_clock(context, *clock_argument) else {
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
        }
    }
}

fn selected_clock<S>(
    context: &ArgumentSuggestionContext<'_, S, SteelArgumentValue>,
    clock_argument: Option<&str>,
) -> Option<WorldClockRef>
where
    S: ExecutionCommandSource,
{
    let Some(clock_argument) = clock_argument else {
        return context.source().default_world_clock();
    };
    match context.argument(clock_argument) {
        Some(SteelArgumentValue::WorldClock(clock)) => Some(*clock),
        Some(
            SteelArgumentValue::Primitive(_)
            | SteelArgumentValue::Time(_)
            | SteelArgumentValue::Coordinates(_)
            | SteelArgumentValue::EntityAnchor(_)
            | SteelArgumentValue::Swizzle(_)
            | SteelArgumentValue::Heightmap(_)
            | SteelArgumentValue::EntitySelector(_)
            | SteelArgumentValue::GameMode(_)
            | SteelArgumentValue::Domain(_)
            | SteelArgumentValue::EntityType(_)
            | SteelArgumentValue::Enchantment(_)
            | SteelArgumentValue::ItemStack(_)
            | SteelArgumentValue::ItemPredicate(_)
            | SteelArgumentValue::Identifier(_)
            | SteelArgumentValue::Timeline(_),
        )
        | None => None,
    }
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
    S: ExecutionCommandSource,
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

fn unknown_resource(
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

pub(super) fn matches_substring(pattern: &str, input: &str) -> bool {
    if input.starts_with(pattern) {
        return true;
    }
    input.char_indices().any(|(index, character)| {
        matches!(character, '.' | '_' | '/')
            && input[index + character.len_utf8()..].starts_with(pattern)
    })
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
