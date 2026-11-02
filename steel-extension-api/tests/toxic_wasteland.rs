//! The toxic-wasteland extension against a real world: registration, spreading, client
//! projection of the liquid and of its tint, hazards, the bucket, and persistence.
#![cfg(feature = "host")]

#[path = "lucky_block/fixture.rs"]
mod fixture;

use std::{
    env::consts::{DLL_PREFIX, DLL_SUFFIX},
    error::Error,
    path::PathBuf,
    sync::Arc,
};

use glam::DVec3;
use steel_core::{
    behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS},
    bootstrap::init_globals,
    command::CommandRegistry,
    entity::{
        InsideBlockEffectCollector, LivingEntity, SharedEntity, entities::PigEntity, next_entity_id,
    },
    extension::{ExtensionHooks, RegisteredExtensions},
    world::World,
};
use steel_extension_api::host;
use steel_registry::{
    REGISTRY, RegistryEntry as _, RegistryExt as _,
    blocks::block_state_ext::BlockStateExt as _,
    blocks::properties::BlockStateProperties,
    client_content::{ClientFluid, client_block_state, client_fluid},
    vanilla_blocks, vanilla_dimension_types, vanilla_entities, vanilla_fluids, vanilla_mob_effects,
};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Identifier, types::UpdateFlags};

type TestResult = Result<(), Box<dyn Error>>;

const SLUDGE: Identifier = Identifier::new_static("toxic_wasteland", "sludge");
const BIOME: Identifier = Identifier::new_static("toxic_wasteland", "wasteland");
const FLOOR_Y: i32 = 64;

fn library() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../extension-example/toxic-wasteland/target/debug")
        .join(format!("{DLL_PREFIX}toxic_wasteland{DLL_SUFFIX}"))
}

#[test]
#[ignore = "requires cargo build --manifest-path extension-example/toxic-wasteland/Cargo.toml"]
fn toxic_wasteland_spreads_tints_hurts_and_persists() -> TestResult {
    let mut hooks = ExtensionHooks::default();
    // SAFETY: this trusted example uses the checked native v3 contract; the host pins
    // its library and all callback allocations for their required lifetime.
    let extension = unsafe { host::load(&library(), "", &mut hooks) }?;
    init_globals();
    // Building these runs the generator and command registration the extension subscribed to.
    let _extensions = RegisteredExtensions::new_with_builtins()?;
    let _commands = CommandRegistry::new().freeze()?;
    let data_root = tempfile::tempdir()?;
    let _initialized = host::initialize_all(vec![extension], data_root.path(), &mut hooks)?;

    let sludge = check_registration()?;
    let fixture = fixture::Fixture::with_dimension(&vanilla_dimension_types::OVERWORLD, false)?;
    floor(&fixture.world);
    check_spread_and_tint(&fixture.world, sludge)?;
    check_hazards(&fixture.world, sludge)?;
    check_random_tick_rates(&fixture.world, sludge);
    check_lava(&fixture.world, sludge);
    check_bucket(&fixture.world, sludge)?;
    check_persistence(&fixture, sludge)
}

fn floor(world: &Arc<World>) {
    let stone = vanilla_blocks::STONE.default_state();
    for x in 0..16 {
        for z in 0..16 {
            world.set_block(BlockPos::new(x, FLOOR_Y, z), stone, UpdateFlags::UPDATE_ALL);
        }
    }
}

fn clear(world: &Arc<World>) {
    let air = vanilla_blocks::AIR.default_state();
    for x in 0..16 {
        for z in 0..16 {
            for y in FLOOR_Y + 1..FLOOR_Y + 4 {
                world.set_block(BlockPos::new(x, y, z), air, UpdateFlags::UPDATE_ALL);
            }
        }
    }
}

fn check_registration() -> Result<&'static ClientFluid, Box<dyn Error>> {
    let source = REGISTRY
        .fluids
        .by_key(&SLUDGE)
        .ok_or("sludge fluid missing")?;
    let sludge = REGISTRY
        .client_content
        .fluid(source)
        .ok_or("sludge is not a registered client fluid")?;
    assert_eq!(
        sludge.flowing.key,
        Identifier::new_static("toxic_wasteland", "sludge_flowing")
    );
    assert!(sludge.tint.is_some(), "sludge must tint water");
    assert!(REGISTRY.blocks.by_key(&SLUDGE).is_some());
    assert!(
        REGISTRY
            .items
            .by_key(&Identifier::new_static("toxic_wasteland", "sludge_bucket"))
            .is_some()
    );
    assert!(REGISTRY.biomes.by_key(&BIOME).is_some());

    // Clients see water, state by state, and move as in water.
    let block = sludge.block;
    for level in 0..16_u8 {
        let state = block
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, level);
        let expected = vanilla_blocks::WATER
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, level);
        assert_eq!(client_block_state(state), expected, "level {level}");
    }
    assert_eq!(client_fluid(sludge.source), &vanilla_fluids::WATER);
    assert_eq!(client_fluid(sludge.flowing), &vanilla_fluids::FLOWING_WATER);
    assert!(!REGISTRY.client_content.is_vanilla_fluid(sludge.source));

    // One green variant per server biome follows the server biomes.
    let table = REGISTRY.client_content.client_biomes();
    assert_eq!(table.client_count(), table.server_count() * 2);
    let wasteland = REGISTRY.biomes.by_key(&BIOME).ok_or("biome missing")?;
    let variant = &table.variants()[table.variant_id(wasteland.id(), 0) - table.server_count()];
    assert!(std::ptr::eq(variant.source, wasteland));
    assert_eq!(Some(variant.tint), sludge.tint);
    Ok(sludge)
}

/// Runs every sludge cell's fluid tick until nothing changes, as the scheduler would.
fn settle(world: &Arc<World>, sludge: &ClientFluid) {
    for _ in 0..32 {
        for x in 0..16 {
            for z in 0..16 {
                let pos = BlockPos::new(x, FLOOR_Y + 1, z);
                let state = world.get_block_state(pos);
                let fluid = state.get_fluid_state();
                if std::ptr::eq(fluid.fluid_id, sludge.source)
                    || std::ptr::eq(fluid.fluid_id, sludge.flowing)
                {
                    FLUID_BEHAVIORS
                        .get_behavior(fluid.fluid_id)
                        .tick(world, pos, state, fluid);
                }
            }
        }
    }
}

const fn section_index(world: &World, y: i32) -> usize {
    ((y - world.get_min_y()) / 16) as usize
}

fn client_biomes(world: &World) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(world
        .chunk_map
        .with_full_chunk(ChunkPos::new(0, 0), |chunk| {
            chunk.sections().client_biome_buffer()
        })
        .ok_or("chunk 0,0 is not loaded")?)
}

fn take_biome_change(world: &World) -> Result<bool, Box<dyn Error>> {
    let index = section_index(world, FLOOR_Y + 1);
    Ok(world
        .chunk_map
        .with_full_chunk(ChunkPos::new(0, 0), |chunk| {
            chunk.sections().take_client_biome_changes([index])
        })
        .ok_or("chunk 0,0 is not loaded")?)
}

fn check_spread_and_tint(world: &Arc<World>, sludge: &ClientFluid) -> TestResult {
    let before = client_biomes(world)?;
    let _ = take_biome_change(world)?;
    let source = BlockPos::new(8, FLOOR_Y + 1, 8);
    world.set_block(
        source,
        sludge.block.default_state(),
        UpdateFlags::UPDATE_ALL,
    );
    settle(world, sludge);

    // drop_off 2: amounts 8 → 6 → 4 → 2, so sludge reaches three blocks out.
    let at = |x: i32| world.get_block_state(BlockPos::new(x, FLOOR_Y + 1, 8));
    for x in 8..=11 {
        assert_eq!(at(x).get_block(), sludge.block, "x {x}");
    }
    assert!(at(12).is_air(), "sludge spread past its reach");
    for x in 0..16 {
        for z in 0..16 {
            let state = world.get_block_state(BlockPos::new(x, FLOOR_Y + 1, z));
            assert_ne!(
                state.get_block(),
                &vanilla_blocks::WATER,
                "sludge turned into water"
            );
        }
    }

    let tinted = client_biomes(world)?;
    assert_ne!(tinted, before, "sludge cells must carry the tint variant");
    assert!(
        take_biome_change(world)?,
        "a newly tinted cell must queue a client refresh"
    );
    assert!(
        !take_biome_change(world)?,
        "the refresh flag clears once taken"
    );

    clear(world);
    assert_eq!(
        client_biomes(world)?,
        before,
        "drained cells return to their real biome"
    );
    assert!(take_biome_change(world)?);
    Ok(())
}

fn check_hazards(world: &Arc<World>, sludge: &ClientFluid) -> TestResult {
    let pos = BlockPos::new(4, FLOOR_Y + 1, 4);
    world.set_block(pos, sludge.block.default_state(), UpdateFlags::UPDATE_ALL);
    let pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        DVec3::new(4.5, f64::from(FLOOR_Y + 1), 4.5),
        Arc::downgrade(world),
    ));
    world.try_add_entity(Arc::clone(&pig) as SharedEntity)?;
    let health = pig.get_health();
    let behavior = FLUID_BEHAVIORS.get_behavior(sludge.source);
    behavior.entity_inside(
        world,
        pos,
        pig.as_ref(),
        &mut InsideBlockEffectCollector::new(),
    );
    assert!(pig.get_health() < health, "contact damage was not dealt");
    assert!(
        pig.has_mob_effect(vanilla_mob_effects::POISON),
        "poison was not applied"
    );

    // Within the damage interval, more contact deals no more damage.
    pig.set_invulnerable_time(0);
    let health = pig.get_health();
    behavior.entity_inside(
        world,
        pos,
        pig.as_ref(),
        &mut InsideBlockEffectCollector::new(),
    );
    assert!((pig.get_health() - health).abs() < f32::EPSILON);
    clear(world);
    Ok(())
}

fn rate(hits: u32, trials: u32) -> f64 {
    f64::from(hits) / f64::from(trials)
}

fn check_random_tick_rates(world: &Arc<World>, sludge: &ClientFluid) {
    let pos = BlockPos::new(8, FLOOR_Y + 1, 8);
    let water_pos = BlockPos::new(9, FLOOR_Y + 1, 8);
    let ground_pos = BlockPos::new(8, FLOOR_Y, 8);
    let water = vanilla_blocks::WATER.default_state();
    let grass = vanilla_blocks::GRASS_BLOCK.default_state();
    let behavior = FLUID_BEHAVIORS.get_behavior(sludge.source);
    let (mut corrupted, mut withered) = (0, 0);
    let trials = 10_000;
    for _ in 0..trials {
        world.set_block(
            pos,
            sludge.block.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        );
        world.set_block(water_pos, water, UpdateFlags::UPDATE_CLIENTS);
        world.set_block(ground_pos, grass, UpdateFlags::UPDATE_CLIENTS);
        behavior.random_tick(world, pos);
        if world.get_block_state(water_pos).get_block() == sludge.block {
            corrupted += 1;
        }
        if world.get_block_state(ground_pos).get_block() == &vanilla_blocks::COARSE_DIRT {
            withered += 1;
        }
    }
    let (corruption, withering) = (rate(corrupted, trials), rate(withered, trials));
    assert!(
        (0.225..=0.275).contains(&corruption),
        "corruption rate {corruption}"
    );
    assert!(
        (0.135..=0.165).contains(&withering),
        "withering rate {withering}"
    );
    world.set_block(
        ground_pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    );
    clear(world);
}

fn check_lava(world: &Arc<World>, sludge: &ClientFluid) {
    let pos = BlockPos::new(3, FLOOR_Y + 1, 12);
    let behavior = FLUID_BEHAVIORS.get_behavior(sludge.source);
    for (level, cooled) in [
        (0, &vanilla_blocks::COARSE_DIRT),
        (2, &vanilla_blocks::COBBLESTONE),
    ] {
        let lava = vanilla_blocks::LAVA
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, level);
        world.set_block(
            pos,
            sludge.block.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        );
        world.set_block(pos.offset(1, 0, 0), lava, UpdateFlags::UPDATE_CLIENTS);
        assert!(
            behavior.react_with_neighbors(world, pos),
            "sludge itself keeps spreading"
        );
        assert_eq!(
            world.get_block_state(pos.offset(1, 0, 0)).get_block(),
            cooled,
            "lava level {level}"
        );
    }
    clear(world);
}

fn check_bucket(world: &Arc<World>, sludge: &ClientFluid) -> TestResult {
    let pos = BlockPos::new(12, FLOOR_Y + 1, 12);
    let liquid = BLOCK_BEHAVIORS.get_behavior(sludge.block);
    let flowing = sludge
        .block
        .default_state()
        .set_value(&BlockStateProperties::LEVEL, 3_u8);
    world.set_block(pos, flowing, UpdateFlags::UPDATE_CLIENTS);
    assert!(
        liquid.pickup_block(world, pos, flowing, None).is_none(),
        "flowing sludge was picked up"
    );
    let source = sludge.block.default_state();
    world.set_block(pos, source, UpdateFlags::UPDATE_CLIENTS);
    let picked = liquid
        .pickup_block(world, pos, source, None)
        .ok_or("the sludge source was not picked up")?;
    assert_eq!(
        picked.filled_bucket.item().key,
        Identifier::new_static("toxic_wasteland", "sludge_bucket")
    );
    assert!(world.get_block_state(pos).is_air());
    Ok(())
}

fn check_persistence(fixture: &fixture::Fixture, sludge: &ClientFluid) -> TestResult {
    let world = &fixture.world;
    let pos = BlockPos::new(6, FLOOR_Y + 1, 6);
    let flowing: BlockStateId = sludge
        .block
        .default_state()
        .set_value(&BlockStateProperties::LEVEL, 2_u8);
    world.set_block(
        pos,
        sludge.block.default_state(),
        UpdateFlags::UPDATE_CLIENTS,
    );
    world.set_block(pos.offset(1, 0, 0), flowing, UpdateFlags::UPDATE_CLIENTS);
    let wasteland = REGISTRY.biomes.by_key(&BIOME).ok_or("biome missing")?;
    let cells = world.fill_biome(
        BlockPos::new(0, FLOOR_Y, 0),
        BlockPos::new(7, FLOOR_Y + 7, 7),
        wasteland,
    )?;
    assert_eq!(cells, 2 * 2 * 2, "an 8-block cube covers 2×2×2 biome cells");
    assert_eq!(
        world.biome_at(BlockPos::new(2, FLOOR_Y + 2, 2)),
        Some(wasteland)
    );

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
        .ok_or("missing saved chunk")?;
    assert_eq!(
        loaded.chunk.get_block_state(pos),
        sludge.block.default_state()
    );
    assert_eq!(loaded.chunk.get_block_state(pos.offset(1, 0, 0)), flowing);
    let biome = loaded.chunk.sections().sections[section_index(world, FLOOR_Y + 2)]
        .read()
        .biomes
        .get(0, 0, 0);
    assert_eq!(usize::from(biome), wasteland.id());
    Ok(())
}
