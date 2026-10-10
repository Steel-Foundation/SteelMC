//! Radius-4 TNT callback order and fuses captured from Minecraft 26.2 on JDK 25.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;

use crate::entity::{Entity as _, SharedEntity, entities::PrimedTntEntity, next_entity_id};
use crate::world::{DefaultExplosionDamageCalculator, Explosion, ExplosionDamageCalculator, World};
use steel_registry::vanilla_entities;
use steel_utils::{Downcast as _, WorldAabb};

use super::*;

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
fn radius_four_tnt_matches_vanilla_iteration_callbacks_and_fuses() {
    let expected = include_str!("vanilla-tnt-order.txt");
    compare_ray_mode(expected, true);
    compare_ray_mode(expected, false);
}

fn compare_ray_mode(expected: &str, recorded_rays: bool) {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("explosion_java_tree_gameplay");
    let world = &fixture.world;
    let tnt = populate_fixture(world);
    let source: SharedEntity = Arc::new(PrimedTntEntity::primed(
        &vanilla_entities::TNT,
        next_entity_id(),
        DVec3::new(8.5, 65.0, 8.5),
        world,
        None,
    ));
    let center = DVec3::new(
        8.5,
        65.0 + f64::from(source.base().dimensions().height) * 0.0625,
        8.5,
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
        4.0,
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
    world.set_random_seed_for_test(1);
    let mut affected = explosion.calculate_exploded_positions_from_level_random();
    let mut hashes = BTreeSet::new();
    for &pos in &affected {
        assert!(
            hashes.insert(pos.java_hash_code()),
            "fixture has equal full hashes"
        );
    }
    let iteration = format_positions(&affected);
    explosion.interact_with_blocks(&mut affected);
    let callbacks = format_positions(&affected);
    let mut fuses = Vec::new();
    for entity in world.get_entities_in_aabb(&WorldAabb::new(-16.0, 52.0, -16.0, 32.0, 81.0, 32.0))
    {
        if let Some(primed) = entity.downcast_ref::<PrimedTntEntity>() {
            let pos = BlockPos::from(primed.position());
            fuses.push(format!(
                "{} {} {} {}",
                pos.x(),
                pos.y(),
                pos.z(),
                primed.fuse()
            ));
        }
    }
    fuses.sort();
    let mut actual = format!(
        "iteration\n{iteration}callbacks\n{callbacks}fuses\n{}\nsurviving-tnt\n",
        fuses.join("\n")
    );
    for &pos in &tnt {
        if world.get_block_state(pos).get_block() == &vanilla_blocks::TNT {
            actual.push_str(&format_positions(&[pos]));
        }
    }
    let (insertion_digest, expected) = expected.split_once('\n').expect("capture insertion digest");
    if recorded_rays {
        let insertions = format_positions(&calculator.0.lock());
        let mut digest = String::new();
        for byte in Sha256::digest(insertions.as_bytes()) {
            write!(digest, "{byte:02x}").expect("string write");
        }
        assert_eq!(format!("insertions-sha256 {digest}"), insertion_digest);
    }
    assert_eq!(actual, expected);
}

fn populate_fixture(world: &Arc<World>) -> [BlockPos; 3] {
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
    let tnt = [
        BlockPos::new(6, 65, 9),
        BlockPos::new(10, 65, 5),
        BlockPos::new(8, 65, 7),
    ];
    for &pos in &tnt {
        assert!(world.set_block(
            pos,
            vanilla_blocks::TNT.default_state(),
            UpdateFlags::UPDATE_NONE
        ));
    }
    tnt
}
