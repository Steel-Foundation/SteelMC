//! Vanilla geometry projection, placement, persistence, and custom mining.

use std::{error::Error, sync::Arc};

use glam::DVec3;
use steel_core::{
    entity::{Entity as _, entities::objects::items::ItemEntity},
    player::game_mode::{
        block_breaking::{BlockBreakAction, BlockBreakingManager},
        use_item_on,
    },
};
use steel_protocol::packets::game::CBlockUpdate;
use steel_registry::{
    REGISTRY, Registry, RegistryExt,
    blocks::{BlockRef, block_state_ext::BlockStateExt, properties::Direction},
    client_content::{BlockItemRegistration, client_block_state},
    item_stack::ItemStack,
    items::{Item, item::BlockHitResult},
    vanilla_blocks, vanilla_dimension_types, vanilla_items,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Downcast as _, Identifier, WorldAabb,
    serial::WriteTo,
    types::{GameType, InteractionHand, UpdateFlags},
};
use text_components::TextComponent;

use super::fixture::Fixture;

pub(super) fn register(registry: &mut Registry) -> Result<(), String> {
    for (name, geometry, properties) in [
        ("barrier", &vanilla_blocks::BARRIER, vec![]),
        ("empty", &vanilla_blocks::STRUCTURE_VOID, vec![]),
        ("slab", &vanilla_blocks::OAK_SLAB, vec![("type", "top")]),
        (
            "stairs",
            &vanilla_blocks::OAK_STAIRS,
            vec![("facing", "east"), ("half", "top")],
        ),
    ] {
        let carrier = registry
            .blocks
            .state_id_from_block_defaulted_properties(geometry, properties)
            .ok_or("invalid native test geometry")?;
        let key = Identifier::new("geometry_test", name);
        registry.register_client_block_item(BlockItemRegistration {
            key: key.clone(),
            carrier,
            material: &vanilla_blocks::OAK_PLANKS,
            displays: Vec::new(),
            item: Item::new(key, TextComponent::from(name.to_owned()), None),
            item_fallback: &vanilla_items::PAPER,
            drop_self: true,
        })?;
    }
    Ok(())
}

pub(super) fn check() -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::with_dimension(&vanilla_dimension_types::OVERWORLD, false)?;
    fixture.join_player()?;
    let pos = BlockPos::new(8, 65, 8);
    fixture.world.set_block(
        pos.below(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    );
    for name in ["barrier", "empty", "slab", "stairs"] {
        let block = REGISTRY
            .blocks
            .by_key(&Identifier::new("geometry_test", name))
            .ok_or("missing geometry block")?;
        check_placement_and_save(&fixture, pos, block)?;
        check_projection(block)?;
        check_cancellation(&fixture, pos, block.default_state())?;
        // No STOP packet: invisible barriers never produce one on an unmodified client.
        let before = drops_of(&fixture, block);
        let mut breaking = BlockBreakingManager::new();
        action(&mut breaking, &fixture, pos, BlockBreakAction::Start);
        for index in 0..80 {
            if index % 5 == 0 && !fixture.world.get_block_state(pos).is_air() {
                action(&mut breaking, &fixture, pos, BlockBreakAction::Start);
            }
            tick(&mut breaking, &fixture, 1);
        }
        assert!(fixture.world.get_block_state(pos).is_air(), "{name}");
        assert_eq!(drops_of(&fixture, block), before + 1, "{name}");
    }
    check_vanilla(&fixture, pos);
    Ok(())
}

fn check_projection(block: BlockRef) -> Result<(), Box<dyn Error>> {
    let state = block.default_state();
    let model = REGISTRY
        .client_content
        .block(block)
        .ok_or("missing carrier")?;
    assert_eq!(client_block_state(state), model.carrier);
    assert_eq!(
        state.get_static_collision_shape(),
        model.carrier.get_static_collision_shape()
    );
    assert_eq!(
        state.get_static_outline_shape(),
        model.carrier.get_static_outline_shape()
    );
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    CBlockUpdate {
        pos: BlockPos::ZERO,
        block_state: state,
    }
    .write(&mut actual)?;
    CBlockUpdate {
        pos: BlockPos::ZERO,
        block_state: model.carrier,
    }
    .write(&mut expected)?;
    assert_eq!(actual, expected);
    let axe = ItemStack::new(&vanilla_items::DIAMOND_AXE);
    let tool = axe.get_tool().ok_or("missing extracted axe tool")?;
    assert_eq!(
        tool.get_mining_speed(state),
        tool.get_mining_speed(vanilla_blocks::OAK_PLANKS.default_state())
    );
    Ok(())
}

fn check_placement_and_save(
    fixture: &Fixture,
    pos: BlockPos,
    block: BlockRef,
) -> Result<(), Box<dyn Error>> {
    let item = REGISTRY.items.by_block(block);
    fixture
        .player
        .inventory
        .lock()
        .set_selected_item(ItemStack::with_count(item, 2));
    let result = use_item_on(
        &fixture.player,
        &fixture.world,
        InteractionHand::MainHand,
        &BlockHitResult {
            location: DVec3::new(8.5, 65.0, 8.5),
            direction: Direction::Up,
            block_pos: pos.below(),
            miss: false,
            inside: false,
            world_border_hit: false,
        },
    );
    assert!(result.consumes_action());
    assert_eq!(fixture.world.get_block_state(pos), block.default_state());
    assert_eq!(
        fixture.player.inventory.lock().get_selected_item().count(),
        1
    );
    fixture
        .player
        .inventory
        .lock()
        .set_selected_item(ItemStack::empty());
    fixture.runtime.block_on(fixture.world.save_all_chunks())?;
    let saved = fixture
        .runtime
        .block_on(fixture.world.chunk_map.storage.load_chunk(
            ChunkPos::from_block_pos(pos),
            fixture.world.get_min_y(),
            fixture.world.get_height(),
            Arc::downgrade(&fixture.world),
            &fixture.pool,
        ))?
        .ok_or("missing saved geometry chunk")?;
    assert_eq!(saved.chunk.get_block_state(pos), block.default_state());
    Ok(())
}

fn check_cancellation(
    fixture: &Fixture,
    pos: BlockPos,
    state: BlockStateId,
) -> Result<(), Box<dyn Error>> {
    let mut breaking = BlockBreakingManager::new();
    action(&mut breaking, fixture, pos, BlockBreakAction::Start);
    tick(&mut breaking, fixture, 5);
    action(&mut breaking, fixture, pos, BlockBreakAction::Abort);
    tick(&mut breaking, fixture, 80);
    assert_eq!(fixture.world.get_block_state(pos), state);

    action(&mut breaking, fixture, pos, BlockBreakAction::Start);
    fixture
        .player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::DIAMOND_AXE));
    tick(&mut breaking, fixture, 80);
    assert_eq!(
        fixture.world.get_block_state(pos),
        state,
        "changing tools cancels the old mining timer"
    );
    fixture
        .player
        .inventory
        .lock()
        .set_selected_item(ItemStack::empty());

    action(&mut breaking, fixture, pos, BlockBreakAction::Start);
    fixture.world.set_block(
        pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    );
    tick(&mut breaking, fixture, 80);
    assert_eq!(
        fixture.world.get_block_state(pos),
        vanilla_blocks::STONE.default_state()
    );
    fixture.world.set_block(pos, state, UpdateFlags::UPDATE_ALL);

    action(&mut breaking, fixture, pos, BlockBreakAction::Start);
    fixture
        .player
        .try_set_position(DVec3::new(15.5, 65.0, 15.5))?;
    tick(&mut breaking, fixture, 80);
    assert_eq!(
        fixture.world.get_block_state(pos),
        state,
        "leaving reach cancels mining"
    );
    fixture
        .player
        .try_set_position(DVec3::new(6.5, 65.0, 8.5))?;

    action(&mut breaking, fixture, pos, BlockBreakAction::Start);
    assert!(fixture.player.set_game_mode(GameType::Creative));
    tick(&mut breaking, fixture, 80);
    assert_eq!(
        fixture.world.get_block_state(pos),
        state,
        "changing game mode cancels the survival timer"
    );
    assert!(fixture.player.set_game_mode(GameType::Survival));

    action(&mut breaking, fixture, pos, BlockBreakAction::Start);
    action(&mut breaking, fixture, pos, BlockBreakAction::Stop);
    assert_eq!(
        fixture.world.get_block_state(pos),
        state,
        "early STOP must not bypass the material's timer"
    );
    action(&mut breaking, fixture, pos, BlockBreakAction::Abort);
    Ok(())
}

fn check_vanilla(fixture: &Fixture, pos: BlockPos) {
    let mut breaking = BlockBreakingManager::new();
    for block in [&vanilla_blocks::BARRIER, &vanilla_blocks::OAK_PLANKS] {
        fixture
            .world
            .set_block(pos, block.default_state(), UpdateFlags::UPDATE_ALL);
        action(&mut breaking, fixture, pos, BlockBreakAction::Start);
        tick(&mut breaking, fixture, 80);
        assert_eq!(fixture.world.get_block_state(pos), block.default_state());
        action(&mut breaking, fixture, pos, BlockBreakAction::Abort);
    }
}

fn action(
    breaking: &mut BlockBreakingManager,
    fixture: &Fixture,
    pos: BlockPos,
    action: BlockBreakAction,
) {
    breaking.handle_block_break_action(
        &fixture.player,
        &fixture.world,
        pos,
        action,
        Direction::West,
    );
}

fn tick(breaking: &mut BlockBreakingManager, fixture: &Fixture, count: usize) {
    for _ in 0..count {
        breaking.tick(&fixture.player, &fixture.world);
    }
}

fn drops_of(fixture: &Fixture, block: BlockRef) -> i32 {
    fixture
        .world
        .get_entities_in_aabb(&WorldAabb::of_size(
            DVec3::new(8.5, 65.5, 8.5),
            4.0,
            4.0,
            4.0,
        ))
        .iter()
        .filter_map(|entity| entity.downcast_ref::<ItemEntity>())
        .map(ItemEntity::get_item)
        .filter(|stack| stack.is(REGISTRY.items.by_block(block)))
        .map(|stack| stack.count())
        .sum()
}
