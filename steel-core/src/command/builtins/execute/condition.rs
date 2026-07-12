//! `/execute if` and `/execute unless` conditions.

use std::sync::Arc;

use simdnbt::owned::NbtTag;
use steel_registry::{blocks::block_state_ext::BlockStateExt as _, vanilla_blocks};
use steel_utils::{
    BlockPos, BoundingBox, ChunkPos, SectionPos,
    nbt::{NbtPath, compare_nbt_compounds},
    translations,
};
use text_components::TextComponent;

use super::super::super::{
    brigadier::{CommandNodeBuilder, CommandRedirectTarget, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
};
use super::{objective, source_command_storage, source_scoreboard};
use crate::{block_entity::SharedBlockEntity, world::World};

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

const EXECUTE_ROOT: CommandRedirectTarget = CommandRedirectTarget::CommandRoot;
const MAX_BLOCKS_REGION: i64 = 32_768;

pub(super) fn conditionals(name: &'static str, expected: bool) -> Builder {
    // TODO: Add items after every vanilla command-slot provider and container inventory is
    // modeled, including deferred loot-table unpacking.
    // TODO: Add predicate and function after their runtime registries are ported.
    // TODO: Restore Steel stopwatch conditions with the stopwatch command system.
    literal(name)
        .then(biome_condition(expected))
        .then(block_condition(expected))
        .then(blocks_condition(expected))
        .then(data_condition(expected))
        .then(dimension_condition(expected))
        .then(entity_condition(expected))
        .then(loaded_condition(expected))
        .then(score_condition(expected))
}

fn data_condition(expected: bool) -> Builder {
    literal("data")
        .then(
            literal("block").then(
                argument("sourcePos", SteelArgumentType::block_pos())
                    .then(data_path(DataSource::Block, expected)),
            ),
        )
        .then(
            literal("entity").then(
                argument("source", SteelArgumentType::entity())
                    .then(data_path(DataSource::Entity, expected)),
            ),
        )
        .then(
            literal("storage").then(
                argument("source", SteelArgumentType::storage_key())
                    .then(data_path(DataSource::Storage, expected)),
            ),
        )
}

fn data_path(source: DataSource, expected: bool) -> Builder {
    argument("path", SteelArgumentType::nbt_path())
        .forks(EXECUTE_ROOT, move |context| {
            let matches = data_match_count(context, source)? > 0;
            Ok(conditional_sources(context.source(), expected, matches))
        })
        .executes(move |context| {
            let count = data_match_count(context, source)?;
            execute_numeric_condition(context, expected, count)
        })
}

#[derive(Clone, Copy)]
enum DataSource {
    Block,
    Entity,
    Storage,
}

fn data_match_count(
    context: &SteelCommandContext<CommandSource>,
    source: DataSource,
) -> Result<i32, CommandSyntaxError> {
    let tag = match source {
        DataSource::Block => {
            let position = loaded_block_position(context, "sourcePos")?;
            let block_entity = context
                .source()
                .world()
                .get_block_entity(position)
                .ok_or_else(invalid_block_data_source)?;
            let data = block_entity.lock().save_with_full_metadata();
            NbtTag::Compound(data)
        }
        DataSource::Entity => {
            let entity = context.entity("source")?;
            NbtTag::Compound(entity.nbt_for_data_compare())
        }
        DataSource::Storage => {
            let key = context
                .identifier("source")
                .ok_or_else(|| missing_argument("source"))?;
            NbtTag::Compound(source_command_storage(context)?.get(key))
        }
    };
    let path = context
        .nbt_path("path")
        .ok_or_else(|| missing_argument("path"))?;
    matching_data_count(path, &tag)
}

fn matching_data_count(path: &NbtPath, tag: &NbtTag) -> Result<i32, CommandSyntaxError> {
    i32::try_from(path.count_matching(tag))
        .map_err(|_| CommandSyntaxError::dynamic("NBT match count exceeds the command range"))
}

pub(super) fn invalid_block_data_source() -> CommandSyntaxError {
    CommandSyntaxError::dynamic(TextComponent::from(
        &translations::COMMANDS_DATA_BLOCK_INVALID,
    ))
}

fn dimension_condition(expected: bool) -> Builder {
    literal("dimension").then(
        argument("dimension", SteelArgumentType::world())
            .forks(EXECUTE_ROOT, move |context| {
                let matches = dimension_matches(context)?;
                Ok(conditional_sources(context.source(), expected, matches))
            })
            .executes(move |context| {
                execute_boolean_condition(context, expected, dimension_matches(context)?)
            }),
    )
}

fn dimension_matches(
    context: &SteelCommandContext<CommandSource>,
) -> Result<bool, CommandSyntaxError> {
    let world = context
        .world_argument("dimension")
        .ok_or_else(|| missing_argument("dimension"))?
        .resolve(context.source())?;
    Ok(Arc::ptr_eq(context.source().world(), &world))
}

fn blocks_condition(expected: bool) -> Builder {
    literal("blocks").then(
        argument("start", SteelArgumentType::block_pos()).then(
            argument("end", SteelArgumentType::block_pos()).then(
                argument("destination", SteelArgumentType::block_pos())
                    .then(blocks_mode("all", expected, false))
                    .then(blocks_mode("masked", expected, true)),
            ),
        ),
    )
}

fn blocks_mode(name: &'static str, expected: bool, skip_air: bool) -> Builder {
    literal(name)
        .forks(EXECUTE_ROOT, move |context| {
            let matches = matching_block_region_count(context, skip_air)?.is_some();
            Ok(conditional_sources(context.source(), expected, matches))
        })
        .executes(move |context| {
            let count = matching_block_region_count(context, skip_air)?;
            execute_blocks_condition(context, expected, count)
        })
}

fn matching_block_region_count(
    context: &SteelCommandContext<CommandSource>,
    skip_air: bool,
) -> Result<Option<i32>, CommandSyntaxError> {
    let source_start = loaded_block_position(context, "start")?;
    let source_end = loaded_block_position(context, "end")?;
    let destination_start = loaded_block_position(context, "destination")?;
    let source_region = BoundingBox::from_corners(source_start, source_end);
    let destination_end = destination_start.offset(
        source_region.max_x() - source_region.min_x(),
        source_region.max_y() - source_region.min_y(),
        source_region.max_z() - source_region.min_z(),
    );
    let destination_region = BoundingBox::from_corners(destination_start, destination_end);
    let area = block_region_volume(&source_region);
    if area > MAX_BLOCKS_REGION {
        return Err(blocks_too_big(area));
    }

    let world = context.source().world();
    ensure_region_chunks_loaded(world, &source_region)?;
    ensure_region_chunks_loaded(world, &destination_region)?;

    let offset_x = destination_region.min_x() - source_region.min_x();
    let offset_y = destination_region.min_y() - source_region.min_y();
    let offset_z = destination_region.min_z() - source_region.min_z();
    let mut count = 0;
    for z in source_region.min_z()..=source_region.max_z() {
        for y in source_region.min_y()..=source_region.max_y() {
            for x in source_region.min_x()..=source_region.max_x() {
                let source_pos = BlockPos::new(x, y, z);
                let source_state = world.get_block_state(source_pos);
                if !should_compare_block(source_state, skip_air) {
                    continue;
                }
                let destination_pos = source_pos.offset(offset_x, offset_y, offset_z);
                if source_state != world.get_block_state(destination_pos)
                    || !block_entities_match(world, source_pos, destination_pos)
                {
                    return Ok(None);
                }
                count += 1;
            }
        }
    }
    Ok(Some(count))
}

fn block_region_volume(region: &BoundingBox) -> i64 {
    let x_span = i64::from(region.max_x()) - i64::from(region.min_x()) + 1;
    let y_span = i64::from(region.max_y()) - i64::from(region.min_y()) + 1;
    let z_span = i64::from(region.max_z()) - i64::from(region.min_z()) + 1;
    x_span.saturating_mul(y_span).saturating_mul(z_span)
}

// Steel's synchronous command runner rejects unloaded region chunks instead of loading them.
fn ensure_region_chunks_loaded(
    world: &World,
    region: &BoundingBox,
) -> Result<(), CommandSyntaxError> {
    if region.max_y() < world.get_min_y() || region.min_y() > world.get_max_y() {
        return Ok(());
    }
    let min_chunk_x = SectionPos::block_to_section_coord(region.min_x());
    let max_chunk_x = SectionPos::block_to_section_coord(region.max_x());
    let min_chunk_z = SectionPos::block_to_section_coord(region.min_z());
    let max_chunk_z = SectionPos::block_to_section_coord(region.max_z());
    for chunk_z in min_chunk_z..=max_chunk_z {
        for chunk_x in min_chunk_x..=max_chunk_x {
            if !ChunkPos::is_valid(chunk_x, chunk_z) {
                continue;
            }
            let pos = BlockPos::new(chunk_x * 16, world.get_min_y(), chunk_z * 16);
            if !world.is_full_chunk_loaded_at(pos) {
                return Err(unloaded_position());
            }
        }
    }
    Ok(())
}

fn should_compare_block(state: steel_utils::BlockStateId, skip_air: bool) -> bool {
    !skip_air || state.get_block() != &vanilla_blocks::AIR
}

fn block_entities_match(world: &World, source: BlockPos, destination: BlockPos) -> bool {
    let source_entity = world.get_block_entity(source);
    let destination_entity = world.get_block_entity(destination);
    block_entity_data_matches(source_entity.as_ref(), destination_entity.as_ref())
}

fn block_entity_data_matches(
    source: Option<&SharedBlockEntity>,
    destination: Option<&SharedBlockEntity>,
) -> bool {
    let Some(source) = source else {
        return true;
    };
    let Some(destination) = destination else {
        return false;
    };
    if Arc::ptr_eq(source, destination) {
        return true;
    }
    let source = source.lock();
    let destination = destination.lock();
    if source.get_type() != destination.get_type() {
        return false;
    }
    let source_data = source.save_custom_only();
    let destination_data = destination.save_custom_only();
    source_data.len() == destination_data.len()
        && compare_nbt_compounds(&source_data, &destination_data, false)
}

fn execute_blocks_condition(
    context: &SteelCommandContext<CommandSource>,
    expected: bool,
    count: Option<i32>,
) -> Result<i32, CommandSyntaxError> {
    match (expected, count) {
        (true, Some(count)) => {
            let message = translations::COMMANDS_EXECUTE_CONDITIONAL_PASS_COUNT
                .message([TextComponent::from(count.to_string())])
                .component();
            context.source().send_success(&message, false);
            Ok(count)
        }
        (true, None) => Err(conditional_failed()),
        (false, Some(count)) => Err(conditional_failed_count(count)),
        (false, None) => {
            context.source().send_success(
                &TextComponent::from(&translations::COMMANDS_EXECUTE_CONDITIONAL_PASS),
                false,
            );
            Ok(1)
        }
    }
}

fn blocks_too_big(area: i64) -> CommandSyntaxError {
    let message = translations::COMMANDS_EXECUTE_BLOCKS_TOOBIG
        .message([
            TextComponent::from(MAX_BLOCKS_REGION.to_string()),
            TextComponent::from(area.to_string()),
        ])
        .component();
    CommandSyntaxError::dynamic(message)
}

fn block_condition(expected: bool) -> Builder {
    literal("block").then(
        argument("pos", SteelArgumentType::block_pos()).then(
            argument("block", SteelArgumentType::block_predicate())
                .forks(EXECUTE_ROOT, move |context| {
                    let matches = block_matches(context)?;
                    Ok(conditional_sources(context.source(), expected, matches))
                })
                .executes(move |context| {
                    execute_boolean_condition(context, expected, block_matches(context)?)
                }),
        ),
    )
}

fn block_matches(context: &SteelCommandContext<CommandSource>) -> Result<bool, CommandSyntaxError> {
    let position = loaded_block_position(context, "pos")?;
    let predicate = context
        .block_predicate("block")
        .ok_or_else(|| missing_argument("block"))?;
    let world = context.source().world();
    if !predicate.matches_state(world.get_block_state(position)) {
        return Ok(false);
    }
    let Some(expected_nbt) = predicate.nbt() else {
        return Ok(true);
    };
    let Some(block_entity) = world.get_block_entity(position) else {
        return Ok(false);
    };
    let actual_nbt = block_entity.lock().save_with_full_metadata();
    Ok(compare_nbt_compounds(expected_nbt, &actual_nbt, true))
}

fn biome_condition(expected: bool) -> Builder {
    literal("biome").then(
        argument("pos", SteelArgumentType::block_pos()).then(
            argument("biome", SteelArgumentType::biome_or_tag())
                .forks(EXECUTE_ROOT, move |context| {
                    let matches = biome_matches(context)?;
                    Ok(conditional_sources(context.source(), expected, matches))
                })
                .executes(move |context| {
                    execute_boolean_condition(context, expected, biome_matches(context)?)
                }),
        ),
    )
}

fn biome_matches(context: &SteelCommandContext<CommandSource>) -> Result<bool, CommandSyntaxError> {
    let position = loaded_block_position(context, "pos")?;
    let world = context.source().world();
    let biome = world.biome_at(position).ok_or_else(|| {
        CommandSyntaxError::dynamic(TextComponent::from(&translations::ARGUMENT_POS_UNLOADED))
    })?;
    let expected = context
        .biome_or_tag("biome")
        .ok_or_else(|| missing_argument("biome"))?;
    Ok(expected.matches(biome))
}

pub(super) fn loaded_block_position(
    context: &SteelCommandContext<CommandSource>,
    name: &str,
) -> Result<steel_utils::BlockPos, CommandSyntaxError> {
    let position = context
        .coordinates(name)
        .ok_or_else(|| missing_argument(name))?
        .block_pos(context.source());
    let world = context.source().world();
    if !world.is_full_chunk_loaded_at(position) {
        return Err(unloaded_position());
    }
    if !world.is_in_valid_bounds(position) {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::ARGUMENT_POS_OUTOFWORLD,
        )));
    }
    Ok(position)
}

fn unloaded_position() -> CommandSyntaxError {
    CommandSyntaxError::dynamic(TextComponent::from(&translations::ARGUMENT_POS_UNLOADED))
}

fn entity_condition(expected: bool) -> Builder {
    literal("entity").then(
        argument("entities", SteelArgumentType::entities())
            .forks(EXECUTE_ROOT, move |context| {
                let matches = !context.optional_entities("entities")?.is_empty();
                Ok(conditional_sources(context.source(), expected, matches))
            })
            .executes(move |context| {
                let count =
                    i32::try_from(context.optional_entities("entities")?.len()).map_err(|_| {
                        CommandSyntaxError::dynamic("Entity count exceeds the command result range")
                    })?;
                execute_numeric_condition(context, expected, count)
            }),
    )
}

fn loaded_condition(expected: bool) -> Builder {
    literal("loaded").then(
        argument("pos", SteelArgumentType::block_pos())
            .forks(EXECUTE_ROOT, move |context| {
                let matches = loaded_matches(context)?;
                Ok(conditional_sources(context.source(), expected, matches))
            })
            .executes(move |context| {
                execute_boolean_condition(context, expected, loaded_matches(context)?)
            }),
    )
}

fn loaded_matches(
    context: &SteelCommandContext<CommandSource>,
) -> Result<bool, CommandSyntaxError> {
    let position = context
        .coordinates("pos")
        .ok_or_else(|| missing_argument("pos"))?
        .block_pos(context.source());
    Ok(context
        .source()
        .world()
        .is_entity_ticking_chunk_loaded(position))
}

fn score_condition(expected: bool) -> Builder {
    literal("score").then(
        argument("target", SteelArgumentType::score_holder()).then(
            argument("targetObjective", SteelArgumentType::objective())
                .then(score_comparison("=", ScoreComparison::Equal, expected))
                .then(score_comparison("<", ScoreComparison::Less, expected))
                .then(score_comparison(
                    "<=",
                    ScoreComparison::LessOrEqual,
                    expected,
                ))
                .then(score_comparison(">", ScoreComparison::Greater, expected))
                .then(score_comparison(
                    ">=",
                    ScoreComparison::GreaterOrEqual,
                    expected,
                ))
                .then(
                    literal("matches").then(
                        argument("range", SteelArgumentType::int_range())
                            .forks(EXECUTE_ROOT, move |context| {
                                let matches = score_range_matches(context)?;
                                Ok(conditional_sources(context.source(), expected, matches))
                            })
                            .executes(move |context| {
                                execute_boolean_condition(
                                    context,
                                    expected,
                                    score_range_matches(context)?,
                                )
                            }),
                    ),
                ),
        ),
    )
}

fn score_comparison(name: &'static str, comparison: ScoreComparison, expected: bool) -> Builder {
    literal(name).then(
        argument("source", SteelArgumentType::score_holder()).then(
            argument("sourceObjective", SteelArgumentType::objective())
                .forks(EXECUTE_ROOT, move |context| {
                    let matches = scores_match(context, comparison)?;
                    Ok(conditional_sources(context.source(), expected, matches))
                })
                .executes(move |context| {
                    execute_boolean_condition(context, expected, scores_match(context, comparison)?)
                }),
        ),
    )
}

fn scores_match(
    context: &SteelCommandContext<CommandSource>,
    comparison: ScoreComparison,
) -> Result<bool, CommandSyntaxError> {
    let scoreboard = source_scoreboard(context)?;
    let target = context.score_holder("target")?;
    let target_objective = objective(context, scoreboard, "targetObjective")?;
    let source = context.score_holder("source")?;
    let source_objective = objective(context, scoreboard, "sourceObjective")?;
    let Some(target_score) = scoreboard.score(&target, &target_objective) else {
        return Ok(false);
    };
    let Some(source_score) = scoreboard.score(&source, &source_objective) else {
        return Ok(false);
    };
    Ok(comparison.matches(target_score, source_score))
}

fn score_range_matches(
    context: &SteelCommandContext<CommandSource>,
) -> Result<bool, CommandSyntaxError> {
    let scoreboard = source_scoreboard(context)?;
    let target = context.score_holder("target")?;
    let target_objective = objective(context, scoreboard, "targetObjective")?;
    let range = context
        .int_range("range")
        .ok_or_else(|| missing_argument("range"))?;
    Ok(scoreboard
        .score(&target, &target_objective)
        .is_some_and(|score| range.matches(score)))
}

#[derive(Clone, Copy)]
enum ScoreComparison {
    Equal,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

impl ScoreComparison {
    const fn matches(self, target: i32, source: i32) -> bool {
        match self {
            Self::Equal => target == source,
            Self::Less => target < source,
            Self::LessOrEqual => target <= source,
            Self::Greater => target > source,
            Self::GreaterOrEqual => target >= source,
        }
    }
}

fn conditional_sources(
    source: &CommandSource,
    expected: bool,
    matches: bool,
) -> Vec<CommandSource> {
    if matches == expected {
        vec![source.clone()]
    } else {
        Vec::new()
    }
}

fn execute_boolean_condition(
    context: &SteelCommandContext<CommandSource>,
    expected: bool,
    matches: bool,
) -> Result<i32, CommandSyntaxError> {
    if matches != expected {
        return Err(conditional_failed());
    }
    context.source().send_success(
        &TextComponent::from(&translations::COMMANDS_EXECUTE_CONDITIONAL_PASS),
        false,
    );
    Ok(1)
}

fn execute_numeric_condition(
    context: &SteelCommandContext<CommandSource>,
    expected: bool,
    count: i32,
) -> Result<i32, CommandSyntaxError> {
    if expected {
        if count == 0 {
            return Err(conditional_failed());
        }
        let message = translations::COMMANDS_EXECUTE_CONDITIONAL_PASS_COUNT
            .message([TextComponent::from(count.to_string())])
            .component();
        context.source().send_success(&message, false);
        return Ok(count);
    }

    if count != 0 {
        return Err(conditional_failed_count(count));
    }
    context.source().send_success(
        &TextComponent::from(&translations::COMMANDS_EXECUTE_CONDITIONAL_PASS),
        false,
    );
    Ok(1)
}

fn conditional_failed() -> CommandSyntaxError {
    CommandSyntaxError::dynamic(TextComponent::from(
        &translations::COMMANDS_EXECUTE_CONDITIONAL_FAIL,
    ))
}

fn conditional_failed_count(count: i32) -> CommandSyntaxError {
    let message = translations::COMMANDS_EXECUTE_CONDITIONAL_FAIL_COUNT
        .message([TextComponent::from(count.to_string())])
        .component();
    CommandSyntaxError::dynamic(message)
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_registry::{
        test_support::init_test_registry, vanilla_block_entity_types, vanilla_blocks,
    };
    use steel_utils::{locks::SyncMutex, nbt::parse_nbt_path};

    use super::*;
    use crate::block_entity::entities::RawBlockEntity;

    fn raw_block_entity(value: i32, pos: BlockPos, reverse_order: bool) -> SharedBlockEntity {
        let mut data = NbtCompound::new();
        if reverse_order {
            data.insert("other", 11_i32);
            data.insert("value", value);
        } else {
            data.insert("value", value);
            data.insert("other", 11_i32);
        }
        data.insert("x", pos.x());
        Arc::new(SyncMutex::new(RawBlockEntity::with_data(
            &vanilla_block_entity_types::BARREL,
            Weak::new(),
            pos,
            vanilla_blocks::BARREL.default_state(),
            data,
        )))
    }

    #[test]
    fn block_region_volume_uses_inclusive_normalized_corners() {
        let region = BoundingBox::from_corners(BlockPos::new(2, 5, -1), BlockPos::new(-1, 3, 2));

        assert_eq!(block_region_volume(&region), 48);
    }

    #[test]
    fn data_match_count_returns_selected_tag_count() {
        let path = parse_nbt_path("items[].value").expect("path should parse");
        let mut first = NbtCompound::new();
        first.insert("value", 1);
        let mut second = NbtCompound::new();
        second.insert("value", 2);
        let mut root = NbtCompound::new();
        root.insert("items", NbtList::Compound(vec![first, second]));
        let tag = NbtTag::Compound(root);

        assert_eq!(
            matching_data_count(&path, &tag).expect("count should fit"),
            2
        );
    }

    #[test]
    fn masked_regions_skip_only_vanilla_air() {
        init_test_registry();

        assert!(!should_compare_block(
            vanilla_blocks::AIR.default_state(),
            true
        ));
        assert!(should_compare_block(
            vanilla_blocks::CAVE_AIR.default_state(),
            true
        ));
        assert!(should_compare_block(
            vanilla_blocks::VOID_AIR.default_state(),
            true
        ));
        assert!(should_compare_block(
            vanilla_blocks::AIR.default_state(),
            false
        ));
    }

    #[test]
    fn region_block_entities_compare_type_and_custom_data_only() {
        init_test_registry();
        let source = raw_block_entity(7, BlockPos::new(1, 64, 1), false);
        let matching = raw_block_entity(7, BlockPos::new(4, 70, 4), true);
        let different = raw_block_entity(8, BlockPos::new(4, 70, 4), false);

        assert!(block_entity_data_matches(Some(&source), Some(&source)));
        assert!(block_entity_data_matches(Some(&source), Some(&matching)));
        assert!(!block_entity_data_matches(Some(&source), Some(&different)));
        assert!(!block_entity_data_matches(Some(&source), None));
        assert!(block_entity_data_matches(None, Some(&matching)));
    }
}
