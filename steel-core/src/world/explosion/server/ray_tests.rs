use std::sync::atomic::{AtomicUsize, Ordering};

use glam::DVec3;
use sha2::{Digest as _, Sha256};
use steel_registry::fluid::FluidState;
use steel_registry::{init_vanilla_registry, vanilla_blocks};
use steel_utils::random::{Random, legacy_random::LegacyRandom};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, ChunkPos};

use super::*;
use crate::behavior::init_behaviors;
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
use crate::world::explosion::default_block_explosion_resistance;
use crate::world::{DefaultExplosionDamageCalculator, ExplosionInteraction, ExplosionOptions};

const FIXED_RANDOM_SAMPLE: f32 = 0.5;
const STANDARD_TNT_RADIUS: f32 = 4.0;
const MAX_TNT_EXPLOSION_POWER: f32 = 128.0;
const VANILLA_INITIAL_POWER_BASE: f32 = 0.7;
const VANILLA_INITIAL_POWER_RANDOM_SCALE: f32 = 0.6;
const VANILLA_OVERWORLD_MIN_Y: i32 = -64;
const VANILLA_OVERWORLD_MAX_Y: i32 = 319;
const VANILLA_HORIZONTAL_MIN: i32 = -30_000_000;
const VANILLA_HORIZONTAL_MAX_EXCLUSIVE: i32 = 30_000_000;
const EXPECTED_MAX_RADIUS_AFFECTED_COUNT: usize = 280_896;
const EXPECTED_MAX_RADIUS_POSITION_SHA256: &str =
    "157409059963f34bd804ca3dc36d83ed5161b02fbf349ff5b05e2ecdb02c7691";
const FNV1A_64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_64_PRIME: u64 = 0x100_0000_01b3;
const EXPECTED_RAY_STEPS_FNV1A_64: u64 = 0x0f55_998f_8a80_904d;
const ORIGIN_BLOCK_CENTER: DVec3 = DVec3::new(0.5, 64.5, 0.5);
// Bottom-center of a resting TNT plus Vanilla's 1/16-height explosion offset.
const RESTING_PRIMED_TNT_EXPLOSION_CENTER: DVec3 = DVec3::new(0.5, 64.061_25, 0.5);

// OpenJDK 25 iteration order after Vanilla inserts this fixture into its HashSet.
const EXPECTED_JAVA_EMPTY_WORLD_POSITIONS: [BlockPos; 27] = [
    BlockPos::new(0, 64, 0),
    BlockPos::new(-1, 64, 1),
    BlockPos::new(1, 64, -1),
    BlockPos::new(0, 64, 1),
    BlockPos::new(1, 64, 0),
    BlockPos::new(1, 64, 1),
    BlockPos::new(-1, 65, -1),
    BlockPos::new(-1, 65, 0),
    BlockPos::new(0, 65, -1),
    BlockPos::new(-1, 63, -1),
    BlockPos::new(-1, 65, 1),
    BlockPos::new(0, 65, 0),
    BlockPos::new(1, 65, -1),
    BlockPos::new(-1, 63, 0),
    BlockPos::new(0, 63, -1),
    BlockPos::new(0, 65, 1),
    BlockPos::new(1, 65, 0),
    BlockPos::new(-1, 63, 1),
    BlockPos::new(0, 63, 0),
    BlockPos::new(1, 63, -1),
    BlockPos::new(1, 65, 1),
    BlockPos::new(0, 63, 1),
    BlockPos::new(1, 63, 0),
    BlockPos::new(1, 63, 1),
    BlockPos::new(-1, 64, -1),
    BlockPos::new(-1, 64, 0),
    BlockPos::new(0, 64, -1),
];

#[derive(Default)]
struct CountingImmutableCalculator {
    resistance_calls: AtomicUsize,
    decision_calls: AtomicUsize,
    cache_resistance: bool,
    always_allows_block_explosion: bool,
    bounded_read_radius: Option<u32>,
}

impl ImmutableExplosionBlockCalculator for CountingImmutableCalculator {
    fn bounded_block_read_radius(&self) -> Option<u32> {
        self.bounded_read_radius
    }

    fn can_cache_explosion_resistance(&self) -> bool {
        self.cache_resistance
    }

    fn always_allows_block_explosion(&self) -> bool {
        self.always_allows_block_explosion
    }

    fn explosion_resistance(
        &self,
        _reader: &dyn ExplosionBlockReader,
        _pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        self.resistance_calls.fetch_add(1, Ordering::Relaxed);
        default_block_explosion_resistance(state, fluid)
    }

    fn should_explode(
        &self,
        _reader: &dyn ExplosionBlockReader,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        self.decision_calls.fetch_add(1, Ordering::Relaxed);
        true
    }
}

struct CountingBlockReader<'a> {
    world: &'a World,
    calls: AtomicUsize,
}

impl ExplosionBlockReader for CountingBlockReader<'_> {
    fn block_state(&self, pos: BlockPos) -> Option<BlockStateId> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Some(self.world.get_block_state(pos))
    }
}

#[cfg(test)]
fn calculate_immutable_rays_sequential<R: ExplosionBlockReader>(
    rays: &[ExplosionRay],
    context: ExplosionRayContext,
    reader: &R,
    calculator: &dyn ImmutableExplosionBlockCalculator,
) -> Vec<BlockPos> {
    let mut affected = JavaBlockPosSet::default();
    let mut cache = ExplosionBlockCache::default();
    let cache_policy = ImmutableRayCachePolicy {
        resistance: calculator.can_cache_explosion_resistance(),
        always_allows_block_explosion: calculator.always_allows_block_explosion(),
    };
    for ray in rays {
        assert!(visit_immutable_ray_positions_cached(
            *ray,
            context,
            reader,
            calculator,
            cache_policy,
            &mut cache,
            &mut affected,
        ));
    }
    affected.into_iter().collect()
}

#[cfg(test)]
#[test]
fn resistant_center_block_stops_explosion_rays() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_ray_resistance");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center_pos = BlockPos::new(0, 64, 0);
    assert!(world.set_block(
        center_pos,
        vanilla_blocks::OBSIDIAN.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        ORIGIN_BLOCK_CENTER,
        STANDARD_TNT_RADIUS,
        false,
        BlockInteraction::Destroy,
    );

    let affected = explosion.calculate_exploded_positions(|| FIXED_RANDOM_SAMPLE);

    assert!(affected.is_empty());
}

#[test]
fn deterministic_empty_world_rays_match_the_java_hash_set_fixture() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_java_hash_set_fixture");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        ORIGIN_BLOCK_CENTER,
        1.0,
        false,
        BlockInteraction::Destroy,
    );
    let mut draws = 0;

    let affected = explosion.calculate_exploded_positions(|| {
        draws += 1;
        FIXED_RANDOM_SAMPLE
    });

    assert_eq!(draws, RAY_COUNT);
    assert_eq!(affected, EXPECTED_JAVA_EMPTY_WORLD_POSITIONS);
}

#[test]
fn precomputed_ray_steps_match_java_bit_digest() {
    let mut digest = FNV1A_64_OFFSET_BASIS;
    for step in RAY_STEPS.iter() {
        for bits in [step.x.to_bits(), step.y.to_bits(), step.z.to_bits()] {
            for byte in bits.to_le_bytes() {
                digest ^= u64::from(byte);
                digest = digest.wrapping_mul(FNV1A_64_PRIME);
            }
        }
    }

    // Produced by the Minecraft 26.2 ServerExplosion expression under OpenJDK 25.
    assert_eq!(digest, EXPECTED_RAY_STEPS_FNV1A_64);
}

#[test]
fn maximum_radius_air_rays_match_the_java_membership_fixture() {
    struct AirReader(BlockStateId);

    impl ExplosionBlockReader for AirReader {
        fn block_state(&self, _pos: BlockPos) -> Option<BlockStateId> {
            Some(self.0)
        }
    }

    init_vanilla_registry();
    let center = RESTING_PRIMED_TNT_EXPLOSION_CENTER;
    let initial_power = MAX_TNT_EXPLOSION_POWER
        * (VANILLA_INITIAL_POWER_BASE + FIXED_RANDOM_SAMPLE * VANILLA_INITIAL_POWER_RANDOM_SCALE);
    let rays = RAY_STEPS
        .iter()
        .copied()
        .map(|step| ExplosionRay {
            step,
            initial_power,
        })
        .collect::<Vec<_>>();
    let mut affected = calculate_immutable_rays_sequential(
        &rays,
        ExplosionRayContext {
            center,
            bounds: ExplosionWorldBounds {
                min_y: VANILLA_OVERWORLD_MIN_Y,
                max_y: VANILLA_OVERWORLD_MAX_Y,
            },
        },
        &AirReader(vanilla_blocks::AIR.default_state()),
        &DefaultExplosionDamageCalculator,
    );

    assert_eq!(affected.len(), EXPECTED_MAX_RADIUS_AFFECTED_COUNT);
    affected.sort_unstable_by_key(|pos| (pos.x(), pos.y(), pos.z()));
    let mut hasher = Sha256::new();
    for pos in affected {
        hasher.update(pos.x().to_be_bytes());
        hasher.update(pos.y().to_be_bytes());
        hasher.update(pos.z().to_be_bytes());
    }
    let digest = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(digest, EXPECTED_MAX_RADIUS_POSITION_SHA256);
}

#[test]
fn ray_sampling_consumes_the_level_random_in_vanilla_order() {
    const SEED: i64 = 0x1E71_0DE5;

    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_level_random");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    world.set_random_seed_for_test(SEED);
    let mut expected = LegacyRandom::from_seed(SEED as u64);
    for _ in 0..RAY_COUNT {
        expected.next_f32();
    }

    world.explode(ExplosionOptions::new(
        ORIGIN_BLOCK_CENTER,
        -1.0,
        ExplosionInteraction::None,
    ));

    let actual_next = world.with_random(Random::next_i64);
    assert_eq!(actual_next, expected.next_i64());
}

#[test]
fn immutable_ray_sampling_preserves_the_level_random_sequence() {
    const SEED: i64 = 0x1E71_0DE6;

    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_level_random");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    world.set_random_seed_for_test(SEED);
    let mut expected = LegacyRandom::from_seed(SEED as u64);
    for _ in 0..RAY_COUNT {
        expected.next_f32();
    }

    let calculator = DefaultExplosionDamageCalculator;
    let mut options = ExplosionOptions::new(ORIGIN_BLOCK_CENTER, 0.0, ExplosionInteraction::None);
    options.immutable_block_calculator = Some(&calculator);
    world.explode(options);

    let actual_next = world.with_random(Random::next_i64);
    assert_eq!(actual_next, expected.next_i64());
}

#[test]
fn unusual_radii_retain_vanilla_ray_sampling_and_bounds_behavior() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_unusual_radii");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

    for radius in [f32::NEG_INFINITY, -1.0, -0.0, 0.0, f32::NAN] {
        let explosion = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            None,
            ORIGIN_BLOCK_CENTER,
            radius,
            false,
            BlockInteraction::Destroy,
        );
        let mut draws = 0;
        let affected = explosion.calculate_exploded_positions(|| {
            draws += 1;
            FIXED_RANDOM_SAMPLE
        });
        assert_eq!(draws, RAY_COUNT, "radius={radius:?}");
        assert!(affected.is_empty(), "radius={radius:?}");
    }

    let tiny = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        ORIGIN_BLOCK_CENTER,
        f32::MIN_POSITIVE,
        false,
        BlockInteraction::Destroy,
    );
    let tiny_affected = tiny.calculate_exploded_positions(|| FIXED_RANDOM_SAMPLE);
    assert_eq!(tiny_affected, [BlockPos::new(0, 64, 0)]);

    let positive_infinity = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        DVec3::new(
            f64::from(VANILLA_HORIZONTAL_MAX_EXCLUSIVE) + 0.5,
            ORIGIN_BLOCK_CENTER.y,
            ORIGIN_BLOCK_CENTER.z,
        ),
        f32::INFINITY,
        false,
        BlockInteraction::Destroy,
    );
    let mut draws = 0;
    let affected = positive_infinity.calculate_exploded_positions(|| {
        draws += 1;
        FIXED_RANDOM_SAMPLE
    });
    assert_eq!(draws, RAY_COUNT);
    assert!(affected.is_empty());
}

#[test]
fn immutable_rays_match_compatibility_lane_at_radius_boundaries() {
    const BOUNDARY_EPSILON: f64 = 0.001;

    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_radius_boundaries");
    let calculator = DefaultExplosionDamageCalculator;
    let center = DVec3::new(
        16.0 - BOUNDARY_EPSILON,
        80.0 - BOUNDARY_EPSILON,
        -16.0 + BOUNDARY_EPSILON,
    );

    let large_test_radius = 32.0;
    for radius in [
        -0.0,
        f32::MIN_POSITIVE,
        STANDARD_TNT_RADIUS,
        large_test_radius,
    ] {
        let compatibility = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            None,
            center,
            radius,
            false,
            BlockInteraction::Destroy,
        );
        let immutable = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            Some(&calculator),
            center,
            radius,
            false,
            BlockInteraction::Destroy,
        );

        assert_eq!(
            immutable.calculate_exploded_positions(|| FIXED_RANDOM_SAMPLE),
            compatibility.calculate_exploded_positions(|| FIXED_RANDOM_SAMPLE),
            "radius={radius:?}"
        );
    }
}

#[test]
fn vanilla_shuffle_uses_descending_bounded_draws() {
    let mut values = [0, 1, 2, 3];
    let mut bounds = Vec::new();
    let drawn_indexes = [1, 0, 1];
    let mut draw = 0;

    vanilla_shuffle(&mut values, |bound| {
        bounds.push(bound);
        let index = drawn_indexes[draw];
        draw += 1;
        index
    });

    let expected_descending_bounds = [4, 3, 2];
    let expected_shuffled_values = [2, 3, 0, 1];
    assert_eq!(bounds, expected_descending_bounds);
    assert_eq!(values, expected_shuffled_values);
}

#[test]
fn fire_creation_draws_before_testing_air_and_support() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("explosion_fire_order");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let supported = BlockPos::new(1, 64, 1);
    let unsupported = supported.east();
    let occupied = unsupported.east();
    assert!(world.set_block(
        supported.below(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert!(world.set_block(
        occupied,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        DVec3::ZERO,
        1.0,
        true,
        BlockInteraction::Destroy,
    );
    let mut draws = 0;

    explosion.create_fire_with(&[unsupported, occupied, supported], || {
        draws += 1;
        0
    });

    assert_eq!(draws, 3);
    assert!(world.get_block_state(unsupported).is_air());
    assert_eq!(
        world.get_block_state(occupied).get_block(),
        &vanilla_blocks::STONE
    );
    assert!(!world.get_block_state(supported).is_air());
}

#[test]
fn explosion_rays_use_vanilla_world_bounds_in_both_lanes() {
    init_vanilla_registry();
    let world = fresh_test_world("explosion_world_bounds");
    let bounds = ExplosionWorldBounds::from_world(&world);
    let min_y = world.get_min_y();
    let max_y = world.get_max_y();
    let cases = [
        (
            BlockPos::new(VANILLA_HORIZONTAL_MIN, min_y, VANILLA_HORIZONTAL_MIN),
            true,
        ),
        (
            BlockPos::new(
                VANILLA_HORIZONTAL_MAX_EXCLUSIVE - 1,
                max_y,
                VANILLA_HORIZONTAL_MAX_EXCLUSIVE - 1,
            ),
            true,
        ),
        (BlockPos::new(VANILLA_HORIZONTAL_MIN - 1, min_y, 0), false),
        (
            BlockPos::new(VANILLA_HORIZONTAL_MAX_EXCLUSIVE, min_y, 0),
            false,
        ),
        (BlockPos::new(0, min_y, VANILLA_HORIZONTAL_MIN - 1), false),
        (
            BlockPos::new(0, min_y, VANILLA_HORIZONTAL_MAX_EXCLUSIVE),
            false,
        ),
        (BlockPos::new(0, min_y - 1, 0), false),
        (BlockPos::new(0, max_y + 1, 0), false),
    ];

    for (pos, expected) in cases {
        assert_eq!(bounds.contains(pos), expected, "pos={pos:?}");
    }
}

#[test]
fn immutable_block_rays_match_sequential_order_and_repeat() {
    const RANDOM_SEED: i64 = 0x1A11_0DED;

    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_rays");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(8.5, 64.5, 8.5);
    assert!(world.set_block(
        BlockPos::from(center),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let immutable_calculator = DefaultExplosionDamageCalculator;
    let immutable = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        Some(&immutable_calculator),
        center,
        STANDARD_TNT_RADIUS,
        false,
        BlockInteraction::Destroy,
    );
    let sequential = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        center,
        STANDARD_TNT_RADIUS,
        false,
        BlockInteraction::Destroy,
    );
    let calculate = |explosion: &ServerExplosion<'_>| {
        let mut random = LegacyRandom::from_seed(RANDOM_SEED as u64);
        explosion.calculate_exploded_positions(|| random.next_f32())
    };

    let sequential_positions = calculate(&sequential);
    let first_immutable_positions = calculate(&immutable);
    let second_immutable_positions = calculate(&immutable);

    assert_eq!(first_immutable_positions, sequential_positions);
    assert_eq!(second_immutable_positions, first_immutable_positions);
}

#[test]
fn immutable_cache_preserves_order_and_extensible_hook_calls() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_cache");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(8.5, 64.5, 8.5);
    let explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        center,
        STANDARD_TNT_RADIUS,
        false,
        BlockInteraction::Destroy,
    );
    let rays = explosion.draw_immutable_rays(|| FIXED_RANDOM_SAMPLE);
    let context = ExplosionRayContext {
        center,
        bounds: ExplosionWorldBounds::from_world(&world),
    };

    let cached_reader = CountingBlockReader {
        world: world.as_ref(),
        calls: AtomicUsize::new(0),
    };
    let cached_calculator = CountingImmutableCalculator {
        cache_resistance: true,
        ..CountingImmutableCalculator::default()
    };
    let cached =
        calculate_immutable_rays_sequential(&rays, context, &cached_reader, &cached_calculator);

    let uncached_reader = CountingBlockReader {
        world: world.as_ref(),
        calls: AtomicUsize::new(0),
    };
    let uncached_calculator = CountingImmutableCalculator {
        cache_resistance: true,
        ..CountingImmutableCalculator::default()
    };
    let mut uncached_set = JavaBlockPosSet::default();
    for &ray in &rays {
        visit_immutable_ray_positions(
            ray,
            context,
            &uncached_reader,
            &uncached_calculator,
            |pos| {
                uncached_set.insert(pos);
            },
        );
    }
    let uncached = uncached_set.into_iter().collect::<Vec<_>>();

    assert_eq!(cached, uncached);
    assert_eq!(
        cached_calculator.decision_calls.load(Ordering::Relaxed),
        uncached_calculator.decision_calls.load(Ordering::Relaxed)
    );
    assert!(
        cached_calculator.resistance_calls.load(Ordering::Relaxed)
            < uncached_calculator.resistance_calls.load(Ordering::Relaxed)
    );
    assert!(
        cached_reader.calls.load(Ordering::Relaxed) < uncached_reader.calls.load(Ordering::Relaxed)
    );

    let always_allows_calculator = CountingImmutableCalculator {
        cache_resistance: true,
        always_allows_block_explosion: true,
        ..CountingImmutableCalculator::default()
    };
    let always_allows = calculate_immutable_rays_sequential(
        &rays,
        context,
        &cached_reader,
        &always_allows_calculator,
    );
    assert_eq!(always_allows, uncached);
    assert_eq!(
        always_allows_calculator
            .decision_calls
            .load(Ordering::Relaxed),
        0
    );
}

#[test]
fn incomplete_bounded_region_falls_back_before_calculator_hooks() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("immutable_explosion_incomplete_region");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let center = DVec3::new(15.5, 64.5, 8.5);
    let actual_calculator = CountingImmutableCalculator {
        bounded_read_radius: Some(0),
        ..CountingImmutableCalculator::default()
    };
    let actual_explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        Some(&actual_calculator),
        center,
        STANDARD_TNT_RADIUS,
        false,
        BlockInteraction::Destroy,
    );
    let powers = actual_explosion.draw_immutable_ray_powers(|| FIXED_RANDOM_SAMPLE);

    let actual = actual_explosion.calculate_immutable_ray_powers(&powers, &actual_calculator);

    let expected_calculator = CountingImmutableCalculator::default();
    let expected_explosion = ServerExplosion::new(
        &world,
        None,
        None,
        None,
        None,
        center,
        STANDARD_TNT_RADIUS,
        false,
        BlockInteraction::Destroy,
    );
    let expected = expected_explosion.calculate_immutable_ray_powers_uncached_with_reader(
        &powers,
        &expected_calculator,
        world.as_ref(),
    );

    assert_eq!(actual, expected);
    assert_eq!(
        actual_calculator.resistance_calls.load(Ordering::Relaxed),
        expected_calculator.resistance_calls.load(Ordering::Relaxed)
    );
    assert_eq!(
        actual_calculator.decision_calls.load(Ordering::Relaxed),
        expected_calculator.decision_calls.load(Ordering::Relaxed)
    );
}

#[test]
fn bounded_immutable_reader_covers_maximum_power_rays() {
    const BOUNDARY_EPSILON: f64 = 0.001;

    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("bounded_explosion_reader_coverage");
    let calculator = DefaultExplosionDamageCalculator;
    let maximum_initial_power =
        STANDARD_TNT_RADIUS * (VANILLA_INITIAL_POWER_BASE + VANILLA_INITIAL_POWER_RANDOM_SCALE);
    let powers = [maximum_initial_power; RAY_COUNT];

    for center in [
        DVec3::new(0.0, 64.0, 0.0),
        DVec3::new(
            16.0 - BOUNDARY_EPSILON,
            80.0 - BOUNDARY_EPSILON,
            16.0 - BOUNDARY_EPSILON,
        ),
        DVec3::new(
            -16.0 + BOUNDARY_EPSILON,
            48.0 + BOUNDARY_EPSILON,
            -16.0 + BOUNDARY_EPSILON,
        ),
    ] {
        let explosion = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            Some(&calculator),
            center,
            STANDARD_TNT_RADIUS,
            false,
            BlockInteraction::Destroy,
        );
        let bounds = explosion
            .immutable_ray_region_bounds(0)
            .expect("finite radius-four explosion has bounded ray coverage");
        let affected = world
            .try_with_block_region(bounds, |region| {
                let reader = RegionExplosionBlockReader::new(region);
                explosion.calculate_immutable_ray_powers_with_reader(&powers, &calculator, &reader)
            })
            .expect("radius-four ray workset stays within the bounded-reader slot limit")
            .expect("bounded reader covers every maximum-power ray access");

        assert!(
            !affected.is_empty(),
            "maximum-power rays affect blocks at {center:?}"
        );
    }
}

#[test]
fn java_block_pos_set_matches_jdk_collision_resize_order() {
    let mut positions = JavaBlockPosSet::default();
    for x in (0..=128).step_by(16) {
        assert!(positions.insert(BlockPos::new(x, 0, 0)));
    }

    let expected_bucket_count = 32;
    let expected_iteration_order =
        [0, 32, 64, 96, 128, 16, 48, 80, 112].map(|x| BlockPos::new(x, 0, 0));
    assert_eq!(positions.buckets.len(), expected_bucket_count);
    assert_eq!(
        positions.into_iter().collect::<Vec<_>>(),
        expected_iteration_order
    );
}

#[test]
fn java_block_pos_set_resizes_on_a_ninth_collision_before_capacity_sixty_four() {
    let mut positions = JavaBlockPosSet::default();
    for x in 1..=13 {
        assert!(positions.insert(BlockPos::new(x, 0, 0)));
    }
    assert_eq!(positions.buckets.len(), 32);

    for x in (0..=224).step_by(32) {
        assert!(positions.insert(BlockPos::new(x, 0, 0)));
    }
    assert_eq!(positions.buckets.len(), 32);

    assert!(positions.insert(BlockPos::new(256, 0, 0)));
    assert_eq!(positions.buckets.len(), 64);
}

#[test]
fn java_block_pos_set_load_resize_preserves_low_high_split_order() {
    let mut positions = JavaBlockPosSet::default();
    let insertion_order = [16, 0, 17, 1, 18, 2, 19, 3, 20, 4, 21, 5, 22];
    for x in insertion_order {
        assert!(positions.insert(BlockPos::new(x, 0, 0)));
    }

    assert_eq!(positions.buckets.len(), 32);
    let expected_iteration_order =
        [0, 1, 2, 3, 4, 5, 16, 17, 18, 19, 20, 21, 22].map(|x| BlockPos::new(x, 0, 0));
    assert_eq!(
        positions.into_iter().collect::<Vec<_>>(),
        expected_iteration_order
    );
}
