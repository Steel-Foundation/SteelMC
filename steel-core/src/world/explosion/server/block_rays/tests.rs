use std::sync::atomic::{AtomicUsize, Ordering};

use glam::DVec3;
use sha2::{Digest as _, Sha256};
use steel_registry::fluid::FluidState;
use steel_registry::{init_vanilla_registry, vanilla_blocks};
use steel_utils::locks::SyncMutex;
use steel_utils::random::{Random, legacy_random::LegacyRandom};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, ChunkPos};

use super::cache::{
    ExplosionBlockCache, ImmutableRayCachePolicy, bounded_floor_to_i32,
    visit_immutable_ray_positions_cached,
};
use super::*;
use crate::behavior::init_behaviors;
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
use crate::world::explosion::default_block_explosion_resistance;
use crate::world::{BlockInteraction, DefaultExplosionDamageCalculator};

mod gameplay;

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
const BOUNDED_FLOOR_RANDOM_CASE_COUNT: usize = 20_000;
const BOUNDED_FLOOR_TEST_RNG_SEED: u64 = 0x6a09_e667_f3bc_c909;
const BOUNDED_FLOOR_TEST_RNG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const BOUNDED_FLOOR_TEST_RNG_INCREMENT: u64 = 1_442_695_040_888_963_407;
const EXACT_INTEGER_POWER_OF_TWO_SAMPLE: f64 = 65_536.0;
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

fn calculate_cached_immutable_rays<R: ExplosionBlockReader, const USE_BOUNDED_FLOOR: bool>(
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
        assert!(visit_immutable_ray_positions_cached::<
            R,
            ExplosionBlockCache,
            USE_BOUNDED_FLOOR,
        >(
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

#[test]
fn deterministic_empty_world_rays_match_the_java_hash_set_fixture() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("explosion_java_hash_set_fixture");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let explosion = ServerExplosion::new(
        world,
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
    let mut affected = calculate_cached_immutable_rays::<_, false>(
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
fn unusual_radii_retain_vanilla_ray_sampling_and_bounds_behavior() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("explosion_unusual_radii");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));

    for radius in [f32::NEG_INFINITY, -1.0, -0.0, 0.0, f32::NAN] {
        let explosion = ServerExplosion::new(
            world,
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
        world,
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
        world,
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
    assert_eq!(affected, Vec::new());
}

#[test]
fn immutable_rays_match_sequential_lane_in_a_complete_region() {
    const RANDOM_SEED: u64 = 0x1A11_0DED;

    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("immutable_explosion_rays");
    let world = &fixture.world;
    for x in -1..=1 {
        for z in -1..=1 {
            insert_ready_full_chunk(world, ChunkPos::new(x, z));
        }
    }
    // Rays from this center cross chunk and section boundaries through resistant blocks.
    let center = DVec3::new(0.5, 63.5, 0.5);
    for (pos, state) in [
        (
            BlockPos::new(0, 63, 0),
            vanilla_blocks::STONE.default_state(),
        ),
        (
            BlockPos::new(-1, 64, 0),
            vanilla_blocks::WATER.default_state(),
        ),
        (
            BlockPos::new(2, 63, -1),
            vanilla_blocks::OBSIDIAN.default_state(),
        ),
    ] {
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
    }
    let calculator = DefaultExplosionDamageCalculator;

    for radius in [-0.0, f32::MIN_POSITIVE, STANDARD_TNT_RADIUS] {
        let explosion = |immutable| {
            ServerExplosion::new(
                world,
                None,
                None,
                None,
                immutable,
                center,
                radius,
                false,
                BlockInteraction::Destroy,
            )
        };
        let immutable = explosion(Some(&calculator as &dyn ImmutableExplosionBlockCalculator));
        let bounds = immutable
            .immutable_ray_region_bounds(0)
            .expect("finite explosion has bounded ray coverage");
        #[expect(
            clippy::redundant_closure_for_method_calls,
            reason = "the method item cannot satisfy the region callback's higher-ranked lifetime"
        )]
        let complete = world.try_with_block_region(bounds, |region| region.has_complete_data());
        assert_eq!(
            complete,
            Some(true),
            "radius={radius:?} must use the bounded cache"
        );
        let calculate = |explosion: &ServerExplosion<'_>| {
            let mut random = LegacyRandom::from_seed(RANDOM_SEED);
            explosion.calculate_exploded_positions(|| random.next_f32())
        };

        assert_eq!(
            calculate(&immutable),
            calculate(&explosion(None)),
            "radius={radius:?}"
        );
    }
}

#[test]
fn cached_ray_decisions_keep_uncached_order_at_already_affected_positions() {
    struct RecordingCalculator(SyncMutex<Vec<(BlockPos, u32)>>);

    impl ImmutableExplosionBlockCalculator for RecordingCalculator {
        fn can_cache_explosion_resistance(&self) -> bool {
            true
        }

        fn should_explode(
            &self,
            _reader: &dyn ExplosionBlockReader,
            pos: BlockPos,
            _state: BlockStateId,
            power: f32,
        ) -> bool {
            self.0.lock().push((pos, power.to_bits()));
            true
        }
    }

    init_vanilla_registry();
    let fixture = fresh_test_world("cached_ray_callback_order");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let context = ExplosionRayContext {
        center: DVec3::new(8.5, 64.5, 8.5),
        bounds: ExplosionWorldBounds::from_world(world),
    };
    let rays = [DVec3::X, -DVec3::X].map(|direction| ExplosionRay {
        step: direction * RAY_STEP,
        initial_power: 1.0,
    });
    let uncached = RecordingCalculator(SyncMutex::new(Vec::new()));
    let mut expected_affected = JavaBlockPosSet::default();
    for ray in rays {
        visit_immutable_ray_positions(ray, context, world.as_ref(), &uncached, |pos| {
            expected_affected.insert(pos);
        });
    }
    assert!(
        uncached
            .0
            .lock()
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0)
    );

    let cached = RecordingCalculator(SyncMutex::new(Vec::new()));
    let affected =
        calculate_cached_immutable_rays::<_, true>(&rays, context, world.as_ref(), &cached);

    assert_eq!(*cached.0.lock(), *uncached.0.lock());
    assert_eq!(affected, expected_affected.into_iter().collect::<Vec<_>>());
}

#[test]
fn bounded_floor_matches_rust_floor_cast_across_i32_domain() {
    let mut values = vec![
        0.0,
        -0.0,
        f64::from_bits(1),
        -f64::from_bits(1),
        0.5,
        -0.5,
        f64::from(i32::MIN),
        f64::from(i32::MAX),
        f64::from(VANILLA_HORIZONTAL_MIN),
        f64::from(VANILLA_HORIZONTAL_MAX_EXCLUSIVE),
    ];
    for base in [
        f64::from(i32::MIN),
        f64::from(VANILLA_HORIZONTAL_MIN),
        -EXACT_INTEGER_POWER_OF_TWO_SAMPLE,
        -1.0,
        0.0,
        1.0,
        EXACT_INTEGER_POWER_OF_TWO_SAMPLE,
        f64::from(VANILLA_HORIZONTAL_MAX_EXCLUSIVE),
        f64::from(i32::MAX),
    ] {
        values.extend([base.next_down(), base, base.next_up()]);
    }

    let lower = f64::from(i32::MIN);
    let upper = f64::from(i32::MAX) + 1.0;
    for value in values {
        if value >= lower && value < upper {
            assert_eq!(bounded_floor_to_i32(value), value.floor() as i32);
        }
    }

    let mut state = BOUNDED_FLOOR_TEST_RNG_SEED;
    for _ in 0..BOUNDED_FLOOR_RANDOM_CASE_COUNT {
        state = state
            .wrapping_mul(BOUNDED_FLOOR_TEST_RNG_MULTIPLIER)
            .wrapping_add(BOUNDED_FLOOR_TEST_RNG_INCREMENT);
        let integer = (state >> u32::BITS) as u32 as i32;
        state = state
            .wrapping_mul(BOUNDED_FLOOR_TEST_RNG_MULTIPLIER)
            .wrapping_add(BOUNDED_FLOOR_TEST_RNG_INCREMENT);
        let fraction = f64::from(state as u32) / (f64::from(u32::MAX) + 1.0);
        let value = f64::from(integer) + fraction;
        if value < upper {
            assert_eq!(bounded_floor_to_i32(value), value.floor() as i32);
        }
    }
}

#[test]
fn incomplete_bounded_region_falls_back_before_calculator_hooks() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("immutable_explosion_incomplete_region");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let center = DVec3::new(15.5, 64.5, 8.5);
    let actual_calculator = CountingImmutableCalculator {
        bounded_read_radius: Some(0),
        ..CountingImmutableCalculator::default()
    };
    let actual_explosion = ServerExplosion::new(
        world,
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
        world,
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
    let fixture = fresh_test_world("bounded_explosion_reader_coverage");
    let world = &fixture.world;
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
            world,
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
                explosion.calculate_immutable_ray_powers_with_reader(
                    &powers,
                    &calculator,
                    &reader,
                    bounds,
                )
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
fn java_block_pos_set_resizes_on_a_ninth_collision_before_capacity_sixty_four() {
    let mut positions = JavaBlockPosSet::default();
    for x in 1..=13 {
        assert!(positions.insert(BlockPos::new(x, 0, 0)));
    }
    assert_eq!(positions.bucket_count(), 32);

    for x in (0..=224).step_by(32) {
        assert!(positions.insert(BlockPos::new(x, 0, 0)));
    }
    assert_eq!(positions.bucket_count(), 32);

    assert!(positions.insert(BlockPos::new(256, 0, 0)));
    assert_eq!(positions.bucket_count(), 64);
}
