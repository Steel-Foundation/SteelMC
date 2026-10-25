#![expect(missing_docs, reason = "benchmarks")]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use steel_core::fluid::benchmark_support;
use steel_core::fluid::{can_pass_through_wall, can_pass_through_wall_uncached, get_spread};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, SlabType};
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, ChunkPos};

struct OcclusionCase {
    name: &'static str,
    from: BlockStateId,
    to: BlockStateId,
    direction: Direction,
}

fn occlusion_cases() -> Vec<OcclusionCase> {
    let water = vanilla_blocks::WATER.default_state();
    let stone = vanilla_blocks::STONE.default_state();
    let air = vanilla_blocks::AIR.default_state();
    let slab = vanilla_blocks::OAK_SLAB
        .default_state()
        .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Bottom);
    let stairs = vanilla_blocks::OAK_STAIRS.default_state();
    let fence = vanilla_blocks::OAK_FENCE.default_state();
    let bamboo = vanilla_blocks::BAMBOO.default_state();

    vec![
        OcclusionCase {
            name: "water-air",
            from: water,
            to: air,
            direction: Direction::East,
        },
        OcclusionCase {
            name: "water-stone",
            from: water,
            to: stone,
            direction: Direction::East,
        },
        OcclusionCase {
            name: "water-slab",
            from: water,
            to: slab,
            direction: Direction::East,
        },
        OcclusionCase {
            name: "water-stairs",
            from: water,
            to: stairs,
            direction: Direction::East,
        },
        OcclusionCase {
            name: "water-fence",
            from: water,
            to: fence,
            direction: Direction::East,
        },
        OcclusionCase {
            name: "water-bamboo-dynamic",
            from: water,
            to: bamboo,
            direction: Direction::East,
        },
    ]
}

fn bench_occlusion(c: &mut Criterion) {
    let fixture = benchmark_support::create_world("fluid_occlusion");
    let world = &fixture.world;
    let from = BlockPos::new(8, 64, 8);
    let to = BlockPos::new(9, 64, 8);

    let mut group = c.benchmark_group("fluid/occlusion");
    group.throughput(Throughput::Elements(1));

    for case in occlusion_cases() {
        group.bench_function(BenchmarkId::new("cached", case.name), |b| {
            b.iter(|| {
                black_box(can_pass_through_wall(
                    world,
                    from,
                    case.from,
                    to,
                    case.to,
                    case.direction,
                ))
            });
        });
        group.bench_function(BenchmarkId::new("uncached", case.name), |b| {
            b.iter(|| {
                black_box(can_pass_through_wall_uncached(
                    world,
                    from,
                    case.from,
                    to,
                    case.to,
                    case.direction,
                ))
            });
        });
    }

    group.finish();
}

fn spread_world(
    name: &'static str,
    with_floor: bool,
) -> (benchmark_support::FluidBenchWorld, BlockPos, BlockStateId) {
    let fixture = benchmark_support::create_world(name);
    let stone = vanilla_blocks::STONE.default_state();
    let water = vanilla_blocks::WATER.default_state();
    let origin = BlockPos::new(8, 64, 8);

    benchmark_support::insert_full_chunk(&fixture.world, ChunkPos::new(0, 0), |sections, min_y| {
        benchmark_support::set_block(sections, min_y, origin, water);
        if !with_floor {
            return;
        }

        for x in 0..16 {
            for z in 0..16 {
                benchmark_support::set_block(sections, min_y, BlockPos::new(x, 63, z), stone);
            }
        }
    });

    (fixture, origin, water)
}

fn bench_spread(c: &mut Criterion) {
    let mut group = c.benchmark_group("fluid/get_spread");
    group.throughput(Throughput::Elements(1));

    let (flat_fixture, flat_origin, flat_state) = spread_world("fluid_spread_flat", true);
    group.bench_function("flat-stone", |b| {
        b.iter(|| {
            black_box(get_spread(
                &flat_fixture.world,
                flat_origin,
                flat_state,
                &vanilla_fluids::WATER,
                1,
                4,
            ))
        });
    });

    let (open_fixture, open_origin, open_state) = spread_world("fluid_spread_open", false);
    group.bench_function("open-air", |b| {
        b.iter(|| {
            black_box(get_spread(
                &open_fixture.world,
                open_origin,
                open_state,
                &vanilla_fluids::WATER,
                1,
                4,
            ))
        });
    });

    group.finish();
}

criterion_group!(fluid, bench_occlusion, bench_spread);
criterion_main!(fluid);
