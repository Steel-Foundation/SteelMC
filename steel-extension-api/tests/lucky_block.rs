//! Actual native registration, gameplay, client projection, and persistence.
#![cfg(feature = "host")]

#[path = "lucky_block/codec.rs"]
mod codec;
#[path = "lucky_block/fixture.rs"]
mod fixture;
#[path = "lucky_block/geometry.rs"]
mod geometry;
#[path = "lucky_block/mining.rs"]
mod mining;
#[path = "lucky_block/wire.rs"]
mod wire;

use std::{
    env::consts::{DLL_PREFIX, DLL_SUFFIX},
    error::Error,
    path::PathBuf,
    sync::Arc,
};

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_core::{
    bootstrap::init_globals,
    extension::ExtensionHooks,
    player::{chunk_sender::ChunkSender, game_mode::use_item_on},
};
use steel_extension_api::host;
use steel_registry::{
    REGISTRY, RegistryExt,
    blocks::{BlockRef, properties::Direction},
    client_content::{client_block_state, on_registration},
    item_stack::ItemStack,
    items::ItemRef,
    items::item::BlockHitResult,
    vanilla_blocks, vanilla_dimension_types, vanilla_items,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Identifier, WorldAabb,
    locks::SyncMutex,
    types::{InteractionHand, UpdateFlags},
};

#[test]
#[ignore = "requires cargo build --manifest-path extension-example/lucky-block/Cargo.toml"]
fn lucky_block_places_mines_projects_and_persists() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../extension-example/lucky-block/target/debug")
        .join(format!(
            "{DLL_PREFIX}steel_lucky_block_extension{DLL_SUFFIX}"
        ));
    let mut hooks = ExtensionHooks::default();
    on_registration(|registry| {
        if let Err(error) = geometry::register(registry) {
            panic!("geometry test registration failed: {error}");
        }
        // A custom block sharing a vanilla path must not inherit its loot table.
        assert!(
            registry
                .register_client_block(
                    Identifier::new_static("lucky_test", "stone"),
                    registry
                        .blocks
                        .get_default_state_id(&vanilla_blocks::OAK_PLANKS),
                    &vanilla_blocks::OAK_PLANKS,
                    None,
                    Vec::new(),
                )
                .is_ok()
        );
    })?;
    // SAFETY: this trusted example exports the checked v2 native contract;
    // the loader pins the library for the lifetime of its registration callback.
    let extension = unsafe { host::load(&path, "seed = 42", &mut hooks) }?;
    assert!(!REGISTRY.is_initialized());
    init_globals();
    let data_root = tempfile::tempdir()?;
    let _initialized = host::initialize_all(vec![extension], data_root.path(), &mut hooks)?;
    let key = Identifier::new_static("lucky", "lucky_block");
    let block = REGISTRY
        .blocks
        .by_key(&key)
        .ok_or("custom block was not registered")?;
    let item = REGISTRY
        .items
        .by_key(&key)
        .ok_or("custom item was not registered")?;
    let state = block.default_state();
    assert_eq!(item.block, Some(block));
    assert_eq!(REGISTRY.items.by_block(block), item);
    assert_eq!(
        client_block_state(state),
        vanilla_blocks::OAK_PLANKS.default_state()
    );
    let axe = ItemStack::new(&vanilla_items::DIAMOND_AXE);
    let tool = axe
        .get_tool()
        .ok_or("axe is missing its extracted tool component")?;
    assert_eq!(
        tool.get_mining_speed(state),
        tool.get_mining_speed(vanilla_blocks::OAK_PLANKS.default_state())
    );
    assert_eq!(
        tool.is_correct_for_drops(state),
        tool.is_correct_for_drops(vanilla_blocks::OAK_PLANKS.default_state())
    );
    codec::check(item)?;
    wire::check(block, item)?;

    check_gameplay(block, item)?;
    geometry::check()
}

fn check_gameplay(block: BlockRef, item: ItemRef) -> Result<(), Box<dyn Error>> {
    let state = block.default_state();
    let fixture = fixture::Fixture::with_dimension(&vanilla_dimension_types::OVERWORLD, false)?;
    let world = &fixture.world;
    let player = &fixture.player;
    let empty_block = REGISTRY
        .blocks
        .by_key(&Identifier::new_static("lucky_test", "stone"))
        .ok_or("missing empty test block")?;
    let empty_pos = BlockPos::new(3, 65, 3);
    assert!(world.set_block(
        empty_pos,
        empty_block.default_state(),
        UpdateFlags::UPDATE_ALL
    ));
    assert!(world.destroy_block(empty_pos, true));
    assert!(
        world
            .get_entities_in_aabb(&WorldAabb::of_size(
                DVec3::new(3.5, 65.5, 3.5),
                3.0,
                3.0,
                3.0
            ))
            .is_empty()
    );
    let support = BlockPos::new(8, 64, 8);
    let placed = support.above();
    assert!(world.set_block(
        support,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL
    ));
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::with_count(item, 2));
    let result = use_item_on(
        player,
        world,
        InteractionHand::MainHand,
        &BlockHitResult {
            location: DVec3::new(8.5, 65.0, 8.5),
            direction: Direction::Up,
            block_pos: support,
            miss: false,
            inside: false,
            world_border_hit: false,
        },
    );
    assert!(result.consumes_action());
    assert_eq!(world.get_block_state(placed), state);
    assert!(world.set_block(
        BlockPos::new(9, 65, 8),
        vanilla_blocks::OAK_PLANKS.default_state(),
        UpdateFlags::UPDATE_ALL
    ));
    assert_eq!(player.inventory.lock().get_selected_item().count(), 1);

    // Saving uses the real custom key, while client chunks use the carrier.
    fixture.runtime.block_on(world.save_all_chunks())?;
    let loaded = fixture
        .runtime
        .block_on(world.chunk_map.storage.load_chunk(
            ChunkPos::new(0, 0),
            world.get_min_y(),
            world.get_height(),
            Arc::downgrade(world),
            &fixture.pool,
        ))?
        .ok_or("saved chunk was not found")?;
    assert_eq!(loaded.chunk.get_block_state(placed), state);
    assert!(
        loaded.pending_entities.is_empty(),
        "display attachments must not become saved world entities"
    );

    check_displays(&fixture, placed, state)?;

    mining::check(&fixture, item, placed, state)?;
    Ok(())
}

fn check_displays(
    fixture: &fixture::Fixture,
    placed: BlockPos,
    state: BlockStateId,
) -> Result<(), Box<dyn Error>> {
    let world = &fixture.world;
    let connection = &fixture.player.connection;
    let pos = ChunkPos::new(0, 0);
    let epoch = SyncMutex::new(0);
    let mut sender = ChunkSender::default();
    let mut cache = FxHashMap::default();
    sender.mark_chunk_pending_to_send(pos);
    let batch = sender
        .prepare_batch(world, pos, &epoch)
        .ok_or("chunk is not ready")?;
    let encoded = ChunkSender::encode_batch(&batch, &mut cache, None, &fixture.pool);
    fixture.packets.lock().clear();
    assert_eq!(
        sender.commit_batch(&batch, encoded, connection, &epoch),
        [pos]
    );
    let first = fixture.display_ids()?;
    assert_eq!(first.len(), 1);
    fixture.packets.lock().clear();
    sender.update_client_block(connection, placed, state);
    assert!(
        fixture.packets.lock().is_empty(),
        "an unchanged model must not duplicate its displays"
    );

    sender.update_client_block(connection, placed, vanilla_blocks::AIR.default_state());
    let mut removed = fixture.removed_ids()?;
    removed.sort_unstable();
    let mut expected = first;
    expected.sort_unstable();
    assert_eq!(removed, expected);
    fixture.packets.lock().clear();
    sender.update_client_block(connection, placed, state);
    let second = fixture.display_ids()?;
    assert_eq!(second.len(), 1);
    fixture.packets.lock().clear();
    sender.drop_chunk(connection, pos);
    let mut removed = fixture.removed_ids()?;
    removed.sort_unstable();
    let mut expected = second;
    expected.sort_unstable();
    assert_eq!(removed, expected);
    fixture.packets.lock().clear();
    sender.update_client_block(connection, placed, state);
    sender.drop_chunk(connection, pos);
    assert!(
        fixture.packets.lock().is_empty(),
        "unloaded chunks must not receive or retain displays"
    );
    Ok(())
}
