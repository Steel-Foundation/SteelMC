#![expect(missing_docs, reason = "benchmarks")]

use std::hint::black_box;
use std::sync::Once;

use criterion::{Criterion, criterion_group, criterion_main};
use steel_registry::template_pool::{TemplateData, TemplatePoolData};
use steel_registry::test_support::init_test_registry;
use steel_registry::vanilla_template_pools::{vanilla_template_pools, vanilla_templates};
use steel_registry::{
    REGISTRY, RegistryExt,
    structure::{JigsawConfig, StartHeight, StructureData},
};
use steel_utils::Identifier;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::{PositionalRandom, Random};
use steel_worldgen::structure::jigsaw::{assemble, resolve_aliases};

static INIT: Once = Once::new();

fn ensure_registry() {
    INIT.call_once(init_test_registry);
}

struct JigsawBenchCase {
    name: &'static str,
    structure_key: &'static str,
    chunk_x: i32,
    chunk_z: i32,
}

const SEED: i64 = 13579;
const OVERWORLD_MIN_Y: i32 = -64;
const OVERWORLD_MAX_Y: i32 = 320;
const NETHER_MIN_Y: i32 = 0;
const NETHER_MAX_Y: i32 = 256;

const CASES: [JigsawBenchCase; 3] = [
    JigsawBenchCase {
        name: "trial_chambers",
        structure_key: "trial_chambers",
        chunk_x: -98,
        chunk_z: 7,
    },
    JigsawBenchCase {
        name: "bastion_remnant",
        structure_key: "bastion_remnant",
        chunk_x: -96,
        chunk_z: -59,
    },
    JigsawBenchCase {
        name: "village_savanna",
        structure_key: "village_savanna",
        chunk_x: -98,
        chunk_z: -10,
    },
];

fn structure_data(key: &'static str) -> &'static StructureData {
    REGISTRY
        .structures
        .by_key(&Identifier::vanilla_static(key))
        .unwrap_or_else(|| panic!("missing structure registry data for minecraft:{key}"))
}

fn jigsaw_assets() -> (
    rustc_hash::FxHashMap<Identifier, TemplatePoolData>,
    rustc_hash::FxHashMap<Identifier, TemplateData>,
) {
    let pools = vanilla_template_pools()
        .into_iter()
        .map(|pool| (pool.key.clone(), pool))
        .collect();
    let templates = vanilla_templates().into_iter().collect();
    (pools, templates)
}

fn sample_start_height(config: &JigsawConfig, rng: &mut impl Random) -> i32 {
    match config.start_height {
        StartHeight::Constant(y) => y,
        StartHeight::Uniform { min, max } => rng.next_i32_between(min, max),
    }
}

fn run_assembly(
    case: &JigsawBenchCase,
    pools: &rustc_hash::FxHashMap<Identifier, TemplatePoolData>,
    templates: &rustc_hash::FxHashMap<Identifier, TemplateData>,
    min_y: i32,
    max_y: i32,
) -> usize {
    let structure = structure_data(case.structure_key);
    let config = structure
        .config
        .as_jigsaw()
        .unwrap_or_else(|| panic!("{} is not a jigsaw structure", case.structure_key));

    let mut rng = LegacyRandom::from_seed(0);
    rng.set_large_feature_seed(SEED, case.chunk_x, case.chunk_z);

    let mut alias_position_rng = LegacyRandom::from_seed(0);
    alias_position_rng.set_large_feature_seed(SEED, case.chunk_x, case.chunk_z);
    let start_y = sample_start_height(config, &mut alias_position_rng);
    let mut alias_source = LegacyRandom::from_seed(SEED as u64);
    let mut alias_rng =
        alias_source
            .next_positional()
            .at(case.chunk_x << 4, start_y, case.chunk_z << 4);
    let alias_map = resolve_aliases(&config.pool_aliases, &mut alias_rng);

    let mut get_height = |_: i32, _: i32| 64i32;
    let result = assemble(
        config,
        &mut rng,
        case.chunk_x,
        case.chunk_z,
        pools,
        templates,
        &alias_map,
        &mut get_height,
        min_y,
        max_y,
    );

    result.map_or(0, |assembly| assembly.pieces.len())
}

fn bench_jigsaw_assembly(c: &mut Criterion) {
    ensure_registry();
    let (pools, templates) = jigsaw_assets();

    let mut group = c.benchmark_group("jigsaw_assemble");
    for case in CASES {
        let (min_y, max_y) = if case.structure_key == "bastion_remnant" {
            (NETHER_MIN_Y, NETHER_MAX_Y)
        } else {
            (OVERWORLD_MIN_Y, OVERWORLD_MAX_Y)
        };

        group.bench_function(case.name, |b| {
            b.iter(|| {
                black_box(run_assembly(
                    black_box(&case),
                    &pools,
                    &templates,
                    min_y,
                    max_y,
                ))
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_jigsaw_assembly);
criterion_main!(benches);
