//! Vanilla block-region fill command.

use std::sync::Arc;

use steel_registry::{
    blocks::block_state_ext::BlockStateExt as _, vanilla_blocks,
    vanilla_game_rules::MAX_BLOCK_MODIFICATIONS,
};
use steel_utils::{BlockPos, BoundingBox, Identifier, translations, types::UpdateFlags};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        BlockInput, BlockPredicate, CommandSource, SteelArgumentType, SteelCommandContext,
        SteelCommandRuntime, argument, literal,
    },
    registration::CommandRegistration,
};
use super::execute::{ensure_region_chunks_block_ticking, loaded_block_position};
use crate::world::World;

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

#[derive(Clone, Copy)]
enum FillMode {
    Replace,
    Outline,
    Hollow,
    Destroy,
}

#[derive(Clone, Copy)]
enum FilterArgument {
    All,
    Keep,
    Predicate(&'static str),
}

enum FillSelection<'predicate> {
    All,
    Keep,
    Predicate(&'predicate BlockPredicate),
}

impl FillSelection<'_> {
    fn matches(&self, world: &World, pos: BlockPos) -> bool {
        match self {
            Self::All => true,
            Self::Keep => world.get_block_state(pos).is_air(),
            Self::Predicate(predicate) => predicate.matches(world, pos),
        }
    }
}

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("fill"), |_| command())
}

fn command() -> Builder {
    let block =
        with_modes(
            argument("block", SteelArgumentType::block_state()),
            FilterArgument::All,
        )
        .then(
            literal("replace")
                .executes(|context| {
                    execute_fill(context, FillMode::Replace, FilterArgument::All, false)
                })
                .then(with_modes(
                    argument("filter", SteelArgumentType::block_predicate()),
                    FilterArgument::Predicate("filter"),
                )),
        )
        .then(literal("keep").executes(|context| {
            execute_fill(context, FillMode::Replace, FilterArgument::Keep, false)
        }));

    literal("fill").then(
        argument("from", SteelArgumentType::block_pos())
            .then(argument("to", SteelArgumentType::block_pos()).then(block)),
    )
}

fn with_modes(builder: Builder, filter: FilterArgument) -> Builder {
    builder
        .executes(move |context| execute_fill(context, FillMode::Replace, filter, false))
        .then(
            literal("outline")
                .executes(move |context| execute_fill(context, FillMode::Outline, filter, false)),
        )
        .then(
            literal("hollow")
                .executes(move |context| execute_fill(context, FillMode::Hollow, filter, false)),
        )
        .then(
            literal("destroy")
                .executes(move |context| execute_fill(context, FillMode::Destroy, filter, false)),
        )
        .then(
            literal("strict")
                .executes(move |context| execute_fill(context, FillMode::Replace, filter, true)),
        )
}

fn execute_fill(
    context: &SteelCommandContext<CommandSource>,
    mode: FillMode,
    filter: FilterArgument,
    strict: bool,
) -> Result<i32, CommandSyntaxError> {
    let from = loaded_block_position(context, "from")?;
    let to = loaded_block_position(context, "to")?;
    let target = context.block_input("block")?;
    let selection = match filter {
        FilterArgument::All => FillSelection::All,
        FilterArgument::Keep => FillSelection::Keep,
        FilterArgument::Predicate(name) => FillSelection::Predicate(context.block_predicate(name)?),
    };
    let count = fill_blocks(
        context.source().world(),
        BoundingBox::from_corners(from, to),
        target,
        mode,
        selection,
        strict,
    )?;

    let message = translations::COMMANDS_FILL_SUCCESS
        .message([TextComponent::from(count.to_string())])
        .component();
    context.source().send_success(&message, true);
    Ok(count)
}

fn fill_blocks(
    world: &Arc<World>,
    region: BoundingBox,
    target: &BlockInput,
    mode: FillMode,
    selection: FillSelection<'_>,
    strict: bool,
) -> Result<i32, CommandSyntaxError> {
    let area = block_region_volume(region);
    let limit = world.get_game_rule(&MAX_BLOCK_MODIFICATIONS);
    if area > i64::from(limit) {
        return Err(area_too_large(limit, area));
    }
    // Block-ticking readiness guarantees the radius-one Full-chunk halo needed by
    // direct shape and neighbor access. Callbacks that propagate farther retain
    // Steel's normal loaded-world boundary behavior instead of acquiring chunks.
    ensure_region_chunks_block_ticking(world, &region)?;

    let air = BlockInput::from_state(vanilla_blocks::AIR.default_state());
    let flags = placement_flags(strict);
    let mut updated_positions = Vec::new();
    let mut count = 0;

    for z in region.min_z()..=region.max_z() {
        for y in region.min_y()..=region.max_y() {
            for x in region.min_x()..=region.max_x() {
                let pos = BlockPos::new(x, y, z);
                if !selection.matches(world, pos) {
                    continue;
                }

                let old_state = world.get_block_state(pos);
                let affected = matches!(mode, FillMode::Destroy) && world.destroy_block(pos, true);
                let input = input_at(mode, region, pos, target, &air);
                let Some(input) = input else {
                    if affected {
                        count += 1;
                    }
                    continue;
                };

                let placed = input.place(world, pos, flags).map_err(|error| {
                    CommandSyntaxError::dynamic(format!(
                        "Failed to apply block entity NBT at {pos:?}: {error}"
                    ))
                })?;
                if !placed {
                    if affected {
                        count += 1;
                    }
                    continue;
                }

                if !strict {
                    updated_positions.push((pos, old_state));
                }
                count += 1;
            }
        }
    }

    for (pos, old_state) in updated_positions {
        world.update_neighbors_on_block_set(pos, old_state);
    }

    if count == 0 {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_FILL_FAILED,
        )));
    }
    Ok(count)
}

const fn input_at<'input>(
    mode: FillMode,
    region: BoundingBox,
    pos: BlockPos,
    target: &'input BlockInput,
    air: &'input BlockInput,
) -> Option<&'input BlockInput> {
    let boundary = pos.x() == region.min_x()
        || pos.x() == region.max_x()
        || pos.y() == region.min_y()
        || pos.y() == region.max_y()
        || pos.z() == region.min_z()
        || pos.z() == region.max_z();
    match mode {
        FillMode::Outline if !boundary => None,
        FillMode::Hollow if !boundary => Some(air),
        FillMode::Replace | FillMode::Outline | FillMode::Hollow | FillMode::Destroy => {
            Some(target)
        }
    }
}

fn placement_flags(strict: bool) -> UpdateFlags {
    let mut flags = UpdateFlags::UPDATE_CLIENTS | UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS;
    if strict {
        flags |= UpdateFlags::UPDATE_KNOWN_SHAPE
            | UpdateFlags::UPDATE_SUPPRESS_DROPS
            | UpdateFlags::UPDATE_SKIP_ON_PLACE;
    }
    flags
}

fn block_region_volume(region: BoundingBox) -> i64 {
    let x_span = i64::from(region.max_x()) - i64::from(region.min_x()) + 1;
    let y_span = i64::from(region.max_y()) - i64::from(region.min_y()) + 1;
    let z_span = i64::from(region.max_z()) - i64::from(region.min_z()) + 1;
    x_span.saturating_mul(y_span).saturating_mul(z_span)
}

fn area_too_large(limit: i32, area: i64) -> CommandSyntaxError {
    let message = translations::COMMANDS_FILL_TOOBIG
        .message([
            TextComponent::from(limit.to_string()),
            TextComponent::from(area.to_string()),
        ])
        .component();
    CommandSyntaxError::dynamic(message)
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_game_rules};
    use steel_utils::{ChunkPos, Downcast as _, WorldAabb};

    use super::super::create_dispatcher;
    use super::*;
    use crate::{
        behavior::init_behaviors,
        block_entity::init_block_entities,
        command::{
            brigadier::{CommandDispatcher, NodeId},
            execution::{SteelArgumentType, SteelCommandRuntime},
        },
        entity::entities::ItemEntity,
        test_support::{fresh_test_world, insert_ready_full_chunk, insert_unready_full_chunk},
    };

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(children) = dispatcher.children(parent) else {
            panic!("parent node should exist");
        };
        let Some(child) = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == name)
        }) else {
            panic!("child {name} should exist");
        };
        child
    }

    fn assert_executable(dispatcher: &Dispatcher, node: NodeId) {
        let Some(node) = dispatcher.node(node) else {
            panic!("command node should exist");
        };
        assert!(node.is_executable());
    }

    fn setup_world(key: &'static str, chunk: ChunkPos) -> Arc<World> {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world(key);
        insert_ready_full_chunk(&world, chunk);
        world
    }

    #[test]
    fn fill_graph_exposes_all_vanilla_modes_and_typed_arguments() {
        init_vanilla_registry();

        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let fill = child(&dispatcher, dispatcher.root(), "fill");
        let from = child(&dispatcher, fill, "from");
        let to = child(&dispatcher, from, "to");
        let block = child(&dispatcher, to, "block");
        assert_eq!(
            dispatcher.node(block).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::block_state())
        );
        assert_executable(&dispatcher, block);

        for mode in ["outline", "hollow", "destroy", "strict", "keep"] {
            let node = child(&dispatcher, block, mode);
            assert_executable(&dispatcher, node);
        }
        let replace = child(&dispatcher, block, "replace");
        assert_executable(&dispatcher, replace);
        let filter = child(&dispatcher, replace, "filter");
        assert_eq!(
            dispatcher
                .node(filter)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::block_predicate())
        );
        for mode in ["outline", "hollow", "destroy", "strict"] {
            let node = child(&dispatcher, filter, mode);
            assert_executable(&dispatcher, node);
        }
    }

    #[test]
    fn hollow_replaces_the_shell_and_clears_the_core() {
        let origin = BlockPos::new(4, 64, 4);
        let world = setup_world("fill_hollow", ChunkPos::from_block_pos(origin));
        let region = BoundingBox::from_corners(origin, origin.offset(2, 2, 2));
        for z in region.min_z()..=region.max_z() {
            for y in region.min_y()..=region.max_y() {
                for x in region.min_x()..=region.max_x() {
                    assert!(world.set_block(
                        BlockPos::new(x, y, z),
                        vanilla_blocks::STONE.default_state(),
                        UpdateFlags::UPDATE_NONE,
                    ));
                }
            }
        }

        let target = BlockInput::from_state(vanilla_blocks::GLASS.default_state());
        let result = fill_blocks(
            &world,
            region,
            &target,
            FillMode::Hollow,
            FillSelection::All,
            false,
        );

        assert_eq!(result, Ok(27));
        assert!(world.get_block_state(origin.offset(1, 1, 1)).is_air());
        assert_eq!(
            world.get_block_state(origin).get_block(),
            &vanilla_blocks::GLASS
        );
    }

    #[test]
    fn replace_filter_only_changes_matching_blocks() {
        let origin = BlockPos::new(5, 64, 5);
        let world = setup_world("fill_filter", ChunkPos::from_block_pos(origin));
        let states = [
            vanilla_blocks::STONE.default_state(),
            vanilla_blocks::DIRT.default_state(),
            vanilla_blocks::STONE.default_state(),
        ];
        for (offset, state) in states.into_iter().enumerate() {
            assert!(world.set_block(
                origin.offset(offset as i32, 0, 0),
                state,
                UpdateFlags::UPDATE_NONE,
            ));
        }
        let predicate = BlockPredicate::Block {
            block: &vanilla_blocks::STONE,
            properties: Vec::new(),
            nbt: None,
        };
        let target = BlockInput::from_state(vanilla_blocks::GLASS.default_state());

        let result = fill_blocks(
            &world,
            BoundingBox::from_corners(origin, origin.offset(2, 0, 0)),
            &target,
            FillMode::Replace,
            FillSelection::Predicate(&predicate),
            false,
        );

        assert_eq!(result, Ok(2));
        assert_eq!(
            world.get_block_state(origin).get_block(),
            &vanilla_blocks::GLASS
        );
        assert_eq!(
            world.get_block_state(origin.east()).get_block(),
            &vanilla_blocks::DIRT
        );
        assert_eq!(
            world.get_block_state(origin.east().east()).get_block(),
            &vanilla_blocks::GLASS
        );
    }

    #[test]
    fn destroy_mode_counts_the_destroyed_block_and_drops_its_loot() {
        let pos = BlockPos::new(8, 64, 8);
        let world = setup_world("fill_destroy", ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::DIRT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let air = BlockInput::from_state(vanilla_blocks::AIR.default_state());

        assert_eq!(
            fill_blocks(
                &world,
                BoundingBox::from_corners(pos, pos),
                &air,
                FillMode::Destroy,
                FillSelection::All,
                false,
            ),
            Ok(1)
        );

        assert!(world.get_block_state(pos).is_air());
        assert!(
            world
                .get_entities_in_aabb(&WorldAabb::new(7.0, 63.0, 7.0, 10.0, 67.0, 10.0))
                .iter()
                .any(|entity| entity.downcast_ref::<ItemEntity>().is_some())
        );
    }

    #[test]
    fn fill_limit_and_unloaded_region_fail_before_mutation() {
        let first = BlockPos::new(15, 64, 0);
        let world = setup_world("fill_preflight", ChunkPos::from_block_pos(first));
        assert!(world.set_game_rule(&vanilla_game_rules::MAX_BLOCK_MODIFICATIONS, 1));
        let target = BlockInput::from_state(vanilla_blocks::STONE.default_state());
        let two_blocks = BoundingBox::from_corners(first, first.east());
        assert!(
            fill_blocks(
                &world,
                two_blocks,
                &target,
                FillMode::Replace,
                FillSelection::All,
                false,
            )
            .is_err()
        );
        assert!(world.get_block_state(first).is_air());

        assert!(world.set_game_rule(&vanilla_game_rules::MAX_BLOCK_MODIFICATIONS, 32_768));
        assert!(
            fill_blocks(
                &world,
                two_blocks,
                &target,
                FillMode::Replace,
                FillSelection::All,
                false,
            )
            .is_err()
        );
        assert!(world.get_block_state(first).is_air());
    }

    #[test]
    fn fill_requires_block_ticking_readiness_for_its_neighbor_halo() {
        let pos = BlockPos::new(15, 64, 8);
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();

        let unavailable = fresh_test_world("fill_unready_halo");
        insert_unready_full_chunk(&unavailable, ChunkPos::new(0, 0));
        insert_ready_full_chunk(&unavailable, ChunkPos::new(1, 0));
        let fence = BlockInput::from_state(vanilla_blocks::OAK_FENCE.default_state());
        assert!(
            fill_blocks(
                &unavailable,
                BoundingBox::from_corners(pos, pos),
                &fence,
                FillMode::Replace,
                FillSelection::All,
                false,
            )
            .is_err()
        );
        assert!(unavailable.get_block_state(pos).is_air());

        let ready = fresh_test_world("fill_ready_halo");
        insert_ready_full_chunk(&ready, ChunkPos::new(0, 0));
        insert_ready_full_chunk(&ready, ChunkPos::new(1, 0));
        assert!(ready.set_block(
            pos.east(),
            vanilla_blocks::OAK_FENCE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert_eq!(
            fill_blocks(
                &ready,
                BoundingBox::from_corners(pos, pos),
                &fence,
                FillMode::Replace,
                FillSelection::All,
                false,
            ),
            Ok(1)
        );
        assert!(
            steel_registry::REGISTRY
                .blocks
                .get_properties(ready.get_block_state(pos))
                .contains(&("east", "true"))
        );
    }
}
