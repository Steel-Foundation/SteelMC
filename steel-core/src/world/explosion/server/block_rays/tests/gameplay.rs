//! TNT ray order, block callbacks, and priming checked against `SteelExtractor` output.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;

use crate::entity::{Entity as _, SharedEntity, entities::PrimedTntEntity, next_entity_id};
use crate::world::{DefaultExplosionDamageCalculator, Explosion, ExplosionDamageCalculator, World};
use steel_registry::vanilla_entities;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{Downcast as _, WorldAabb};

use super::*;

#[derive(serde::Deserialize)]
struct TntExplosionFixture {
    minecraft_version: String,
    java_feature_version: u32,
    random_seed: u64,
    explosion_radius: f32,
    source_position: [f64; 3],
    explosion_center: [f64; 3],
    tnt_positions: Vec<[i32; 3]>,
    insertions_sha256: String,
    iteration: Vec<[i32; 3]>,
    callbacks: Vec<[i32; 3]>,
    primed_tnt: Vec<CapturedPrimedTnt>,
    surviving_tnt: Vec<[i32; 3]>,
}

#[derive(serde::Deserialize)]
struct CapturedPrimedTnt {
    position: [i32; 3],
    fuse: i32,
}

struct RecordingCalculator(SyncMutex<Vec<BlockPos>>);

impl ExplosionDamageCalculator for RecordingCalculator {
    fn should_block_explode(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        self.0.lock().push(pos);
        true
    }
}

fn format_positions(positions: &[BlockPos]) -> String {
    let mut text = String::new();
    for pos in positions {
        writeln!(text, "{} {} {}", pos.x(), pos.y(), pos.z()).expect("string write");
    }
    text
}

#[test]
fn radius_four_tnt_matches_vanilla_order_and_fuse_bounds() {
    let expected: TntExplosionFixture = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_assets/vanilla_tnt_order.json"
    )))
    .expect("SteelExtractor TNT fixture must be valid JSON");
    assert_eq!(expected.minecraft_version, steel_utils::MC_VERSION);
    assert_eq!(expected.java_feature_version, 25);
    compare_ray_mode(&expected, true);
    compare_ray_mode(&expected, false);
}

fn compare_ray_mode(expected: &TntExplosionFixture, recorded_rays: bool) {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("explosion_java_tree_gameplay");
    let world = &fixture.world;
    let tnt = populate_fixture(world, &expected.tnt_positions);
    let source: SharedEntity = Arc::new(PrimedTntEntity::primed(
        &vanilla_entities::TNT,
        next_entity_id(),
        DVec3::from_array(expected.source_position),
        world,
        None,
    ));
    let [x, y, z] = expected.source_position;
    let center = DVec3::new(
        x,
        y + f64::from(source.base().dimensions().height) * 0.0625,
        z,
    );
    assert_eq!(
        center.to_array().map(f64::to_bits),
        expected.explosion_center.map(f64::to_bits)
    );
    let calculator = RecordingCalculator(SyncMutex::new(Vec::new()));
    let explosion = ServerExplosion::new(
        world,
        Some(source),
        None,
        recorded_rays.then_some(&calculator as &dyn ExplosionDamageCalculator),
        (!recorded_rays)
            .then_some(&DefaultExplosionDamageCalculator as &dyn ImmutableExplosionBlockCalculator),
        center,
        expected.explosion_radius,
        false,
        BlockInteraction::DestroyWithDecay,
    );
    if !recorded_rays {
        let bounds = explosion
            .immutable_ray_region_bounds(0)
            .expect("fixture ray region");
        world
            .try_with_block_region(bounds, |region| assert!(region.has_complete_data()))
            .expect("fixture has a resident ray region");
    }
    let mut random = LegacyRandom::from_seed(expected.random_seed);
    let mut affected = explosion.calculate_exploded_positions(|| random.next_f32());
    let mut hashes = BTreeSet::new();
    for &pos in &affected {
        assert!(
            hashes.insert(pos.java_hash_code()),
            "fixture has equal full hashes"
        );
    }
    let iteration = affected
        .iter()
        .map(|pos| [pos.x(), pos.y(), pos.z()])
        .collect::<Vec<_>>();
    explosion.interact_with_blocks(&mut affected, &mut random);
    let callbacks = affected
        .iter()
        .map(|pos| [pos.x(), pos.y(), pos.z()])
        .collect::<Vec<_>>();
    if recorded_rays {
        let insertions = format_positions(&calculator.0.lock());
        let mut digest = String::new();
        for byte in Sha256::digest(insertions.as_bytes()) {
            write!(digest, "{byte:02x}").expect("string write");
        }
        assert_eq!(digest, expected.insertions_sha256);
    }
    assert_eq!(iteration, expected.iteration);
    assert_eq!(callbacks, expected.callbacks);
    assert_tnt_priming(world, &affected, &tnt, &expected.primed_tnt);
    let survivors = tnt
        .into_iter()
        .filter(|&pos| world.get_block_state(pos).get_block() == &vanilla_blocks::TNT)
        .map(|pos| [pos.x(), pos.y(), pos.z()])
        .collect::<Vec<_>>();
    assert_eq!(survivors, expected.surviving_tnt);
}

fn assert_tnt_priming(
    world: &Arc<World>,
    affected: &[BlockPos],
    tnt: &[BlockPos],
    expected_tnt: &[CapturedPrimedTnt],
) {
    // Live TNT uses runtime RNG; compare fuse bounds and priming order.
    let mut primed_positions = Vec::new();
    for entity in world.get_entities_in_aabb(&WorldAabb::new(-16.0, 52.0, -16.0, 32.0, 81.0, 32.0))
    {
        if let Some(primed) = entity.downcast_ref::<PrimedTntEntity>() {
            assert!((10..30).contains(&primed.fuse()), "fuse={}", primed.fuse());
            primed_positions.push((entity.id(), BlockPos::from(primed.position())));
        }
    }
    primed_positions.sort_unstable_by_key(|&(id, _)| id);
    let primed_positions = primed_positions
        .into_iter()
        .map(|(_, pos)| pos)
        .collect::<Vec<_>>();
    assert_eq!(
        primed_positions,
        affected
            .iter()
            .copied()
            .filter(|pos| tnt.contains(pos))
            .collect::<Vec<_>>(),
        "TNT must be primed in callback order"
    );
    let expected_positions = expected_tnt
        .iter()
        .map(|captured| {
            assert!(
                (10..30).contains(&captured.fuse),
                "captured vanilla fuse={}",
                captured.fuse
            );
            let [x, y, z] = captured.position;
            BlockPos::new(x, y, z)
        })
        .collect::<Vec<_>>();
    assert_eq!(primed_positions, expected_positions);
}

fn populate_fixture(world: &Arc<World>, tnt_positions: &[[i32; 3]]) -> Vec<BlockPos> {
    for x in -1..=1 {
        for z in -1..=1 {
            insert_ready_full_chunk(world, ChunkPos::new(x, z));
        }
    }
    for x in -4..=20 {
        for z in -4..=20 {
            for y in 52..=80 {
                if !(65..=67).contains(&y) || (x + z - 16_i32).abs() > 1 {
                    assert!(world.set_block(
                        BlockPos::new(x, y, z),
                        vanilla_blocks::OBSIDIAN.default_state(),
                        UpdateFlags::UPDATE_NONE,
                    ));
                }
            }
        }
    }
    let tnt = tnt_positions
        .iter()
        .map(|&[x, y, z]| BlockPos::new(x, y, z))
        .collect::<Vec<_>>();
    for &pos in &tnt {
        assert!(world.set_block(
            pos,
            vanilla_blocks::TNT.default_state(),
            UpdateFlags::UPDATE_NONE
        ));
    }
    tnt
}
