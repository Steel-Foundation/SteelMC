use super::fixture::Fixture;
use glam::DVec3;
use std::{error::Error, sync::Arc};
use steel_core::player::game_mode::{
    block_breaking::{BlockBreakAction, BlockBreakingManager},
    use_item_on,
};
use steel_core::{behavior::InteractionResult, chunk_saver::LoadedChunk};
use steel_registry::{
    REGISTRY, RegistryExt,
    blocks::{BlockRef, properties::Direction},
    item_stack::ItemStack,
    items::item::BlockHitResult,
    vanilla_blocks,
};
use steel_utils::{
    BlockPos, ChunkPos, Identifier,
    axis::Axis,
    types::{InteractionHand, UpdateFlags},
};

pub(super) fn floor(fixture: &Fixture) {
    for x in 4..=11 {
        for z in 4..=11 {
            fixture.world.set_block(
                BlockPos::new(x, 64, z),
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }
}

pub(super) fn block(name: &str) -> Result<BlockRef, Box<dyn Error>> {
    REGISTRY
        .blocks
        .by_key(&Identifier::new("furniture", name.to_owned()))
        .ok_or_else(|| "missing furniture block".into())
}

pub(super) fn interact(fixture: &Fixture, pos: BlockPos) -> InteractionResult {
    use_item_on(
        &fixture.player,
        &fixture.world,
        InteractionHand::MainHand,
        &BlockHitResult {
            location: DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y())
                    + fixture
                        .world
                        .get_block_state(pos)
                        .get_static_outline_shape()
                        .max(Axis::Y),
                f64::from(pos.z()) + 0.5,
            ),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        },
    )
}

pub(super) fn place(
    fixture: &Fixture,
    name: &str,
    pos: BlockPos,
) -> Result<BlockRef, Box<dyn Error>> {
    let block = block(name)?;
    fixture
        .player
        .inventory
        .lock()
        .set_selected_item(ItemStack::with_count(REGISTRY.items.by_block(block), 2));
    assert!(interact(fixture, pos.below()).consumes_action());
    assert_eq!(
        fixture.player.inventory.lock().get_selected_item().count(),
        if fixture.player.has_infinite_materials() {
            2
        } else {
            1
        }
    );
    fixture
        .player
        .inventory
        .lock()
        .set_selected_item(ItemStack::empty());
    assert_eq!(fixture.world.get_block_state(pos).get_block(), block);
    Ok(block)
}

pub(super) fn mine(fixture: &Fixture, pos: BlockPos) {
    let mut breaking = BlockBreakingManager::new();
    breaking.handle_block_break_action(
        &fixture.player,
        &fixture.world,
        pos,
        BlockBreakAction::Start,
        Direction::West,
    );
    for _ in 0..80 {
        breaking.tick(&fixture.player, &fixture.world);
    }
    breaking.handle_block_break_action(
        &fixture.player,
        &fixture.world,
        pos,
        BlockBreakAction::Stop,
        Direction::West,
    );
}

pub(super) fn saved(fixture: &Fixture) -> Result<LoadedChunk, Box<dyn Error>> {
    fixture.runtime.block_on(fixture.world.save_all_chunks())?;
    fixture
        .runtime
        .block_on(fixture.world.chunk_map.storage.load_chunk(
            ChunkPos::new(0, 0),
            fixture.world.get_min_y(),
            fixture.world.get_height(),
            Arc::downgrade(&fixture.world),
            &fixture.pool,
        ))?
        .ok_or_else(|| "missing saved chunk".into())
}

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
