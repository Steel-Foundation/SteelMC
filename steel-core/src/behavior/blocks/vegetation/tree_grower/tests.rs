use std::convert::Infallible;

use rand::{SeedableRng, TryRng, rngs::StdRng};
use steel_registry::blocks::BlockRef;
use steel_registry::{init_vanilla_registry, vanilla_blocks};
use steel_utils::ChunkPos;

use super::*;
use crate::behavior::init_behaviors;
use crate::test_support::{MaxRng, TestLevel, ZeroRng, fresh_test_world, insert_ready_full_chunk};

/// Always rolls zero and counts how many values vanilla's selection consumes.
#[derive(Default)]
struct CountingRng {
    calls: u32,
}

impl TryRng for CountingRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        self.calls += 1;
        Ok(0)
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        self.calls += 1;
        Ok(0)
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        self.calls += 1;
        dst.fill(0);
        Ok(())
    }
}

fn assert_feature(actual: Option<&ConfiguredFeature>, expected: &ConfiguredFeature) {
    assert_eq!(actual.map(|feature| &feature.key), Some(&expected.key));
}

/// A world with loaded chunks and a dirt floor large enough for any vanilla tree.
fn prepared_tree_world(key: &'static str) -> (Arc<World>, BlockPos) {
    let world = fresh_test_world(key);
    let pos = BlockPos::new(8, 64, 8);
    let center = ChunkPos::from_block_pos(pos);
    for chunk_x in center.0.x - 2..=center.0.x + 2 {
        for chunk_z in center.0.y - 2..=center.0.y + 2 {
            insert_ready_full_chunk(&world, ChunkPos::new(chunk_x, chunk_z));
        }
    }
    for x in pos.x() - 20..=pos.x() + 20 {
        for z in pos.z() - 20..=pos.z() + 20 {
            assert!(world.set_block(
                BlockPos::new(x, pos.y() - 1, z),
                vanilla_blocks::DIRT.default_state(),
                UpdateFlags::UPDATE_NONE,
            ));
        }
    }
    (world, pos)
}

fn square_from(origin: BlockPos) -> [BlockPos; 4] {
    [origin, origin.east(), origin.south(), origin.east().south()]
}

fn grow_with_any_seed(
    grower: TreeGrower,
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
) -> bool {
    (0..128).any(|seed| {
        let mut rng = StdRng::seed_from_u64(seed);
        grower.grow_tree(world, pos, state, &mut rng)
    })
}

fn has_log_near(world: &Arc<World>, pos: BlockPos, log: BlockRef) -> bool {
    (pos.x() - 12..=pos.x() + 12).any(|x| {
        (pos.z() - 12..=pos.z() + 12).any(|z| {
            (pos.y()..=pos.y() + 40)
                .any(|y| world.get_block_state(BlockPos::new(x, y, z)).get_block() == log)
        })
    })
}

#[test]
fn single_tree_selection_matches_vanilla() {
    init_vanilla_registry();

    assert_feature(
        TreeGrower::Oak.get_configured_feature(&mut ZeroRng, false),
        &vanilla_configured_features::FANCY_OAK,
    );
    assert_feature(
        TreeGrower::Oak.get_configured_feature(&mut MaxRng, false),
        &vanilla_configured_features::OAK,
    );
    assert_feature(
        TreeGrower::Oak.get_configured_feature(&mut ZeroRng, true),
        &vanilla_configured_features::FANCY_OAK_BEES_005,
    );
    assert_feature(
        TreeGrower::Oak.get_configured_feature(&mut MaxRng, true),
        &vanilla_configured_features::OAK_BEES_005,
    );
    assert_feature(
        TreeGrower::Birch.get_configured_feature(&mut ZeroRng, true),
        &vanilla_configured_features::BIRCH_BEES_005,
    );
    assert_feature(
        TreeGrower::Cherry.get_configured_feature(&mut ZeroRng, true),
        &vanilla_configured_features::CHERRY_BEES_005,
    );
    assert_feature(
        TreeGrower::Mangrove.get_configured_feature(&mut ZeroRng, false),
        &vanilla_configured_features::TALL_MANGROVE,
    );
    assert_feature(
        TreeGrower::Mangrove.get_configured_feature(&mut MaxRng, false),
        &vanilla_configured_features::MANGROVE,
    );
    assert!(
        TreeGrower::DarkOak
            .get_configured_feature(&mut ZeroRng, true)
            .is_none()
    );
    assert!(
        TreeGrower::PaleOak
            .get_configured_feature(&mut ZeroRng, true)
            .is_none()
    );
}

#[test]
fn mega_tree_selection_matches_vanilla() {
    init_vanilla_registry();

    assert_feature(
        TreeGrower::Spruce.get_configured_mega_feature(&mut ZeroRng),
        &vanilla_configured_features::MEGA_PINE,
    );
    assert_feature(
        TreeGrower::Spruce.get_configured_mega_feature(&mut MaxRng),
        &vanilla_configured_features::MEGA_SPRUCE,
    );
    assert_feature(
        TreeGrower::Jungle.get_configured_mega_feature(&mut ZeroRng),
        &vanilla_configured_features::MEGA_JUNGLE_TREE,
    );
    assert_feature(
        TreeGrower::DarkOak.get_configured_mega_feature(&mut ZeroRng),
        &vanilla_configured_features::DARK_OAK,
    );
    assert_feature(
        TreeGrower::PaleOak.get_configured_mega_feature(&mut ZeroRng),
        &vanilla_configured_features::PALE_OAK_BONEMEAL,
    );
    assert!(
        TreeGrower::Oak
            .get_configured_mega_feature(&mut ZeroRng)
            .is_none()
    );
}

#[test]
fn selection_consumes_the_same_random_values_as_vanilla() {
    init_vanilla_registry();

    let mut rng = CountingRng::default();
    TreeGrower::Birch.get_configured_feature(&mut rng, false);
    assert_eq!(
        rng.calls, 1,
        "getConfiguredFeature always rolls the secondary chance"
    );

    let mut rng = CountingRng::default();
    TreeGrower::Jungle.get_configured_mega_feature(&mut rng);
    assert_eq!(rng.calls, 0, "no secondary mega tree means no roll");

    let mut rng = CountingRng::default();
    TreeGrower::Spruce.get_configured_mega_feature(&mut rng);
    assert_eq!(rng.calls, 1);
}

#[test]
fn nearby_flowers_are_detected_within_the_vanilla_box() {
    init_vanilla_registry();
    let pos = BlockPos::ZERO;
    let flower = vanilla_blocks::DANDELION.default_state();

    assert!(TreeGrower::has_flowers(
        &TestLevel::default().with_block(pos.offset(2, 1, -2), flower),
        pos
    ));
    assert!(TreeGrower::has_flowers(
        &TestLevel::default().with_block(pos.offset(-2, -1, 2), flower),
        pos
    ));
    assert!(!TreeGrower::has_flowers(
        &TestLevel::default().with_block(pos.offset(3, 0, 0), flower),
        pos
    ));
    assert!(!TreeGrower::has_flowers(
        &TestLevel::default().with_block(pos.offset(0, 2, 0), flower),
        pos
    ));
}

#[test]
fn two_by_two_detection_requires_the_same_block_on_all_four_corners() {
    init_vanilla_registry();
    let pos = BlockPos::ZERO;
    let sapling = vanilla_blocks::DARK_OAK_SAPLING.default_state();
    let mut level = TestLevel::default();
    for corner in square_from(pos) {
        level = level.with_block(corner, sapling);
    }

    assert!(TreeGrower::is_two_by_two_sapling(
        sapling, &level, pos, 0, 0
    ));
    assert!(TreeGrower::is_two_by_two_sapling(
        sapling,
        &level,
        pos.east().south(),
        -1,
        -1
    ));
    assert!(!TreeGrower::is_two_by_two_sapling(
        sapling, &level, pos, -1, 0
    ));

    let mixed = level.with_block(pos.east(), vanilla_blocks::OAK_SAPLING.default_state());
    assert!(!TreeGrower::is_two_by_two_sapling(
        sapling, &mixed, pos, 0, 0
    ));
}

#[test]
fn lone_mega_only_sapling_does_not_grow() {
    init_vanilla_registry();
    init_behaviors();
    let (world, pos) = prepared_tree_world("tree_grower_lone_dark_oak");
    let state = vanilla_blocks::DARK_OAK_SAPLING.default_state();
    assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

    assert!(!TreeGrower::DarkOak.grow_tree(&world, pos, state, &mut ZeroRng));
    assert_eq!(world.get_block_state(pos), state);
}

#[test]
fn failed_mega_tree_restores_all_four_saplings() {
    init_vanilla_registry();
    init_behaviors();
    let (world, pos) = prepared_tree_world("tree_grower_failed_mega_restore");
    let state = vanilla_blocks::DARK_OAK_SAPLING.default_state();
    for sapling_pos in square_from(pos) {
        assert!(world.set_block(sapling_pos, state, UpdateFlags::UPDATE_NONE));
    }
    for x in pos.x() - 2..=pos.x() + 3 {
        for z in pos.z() - 2..=pos.z() + 3 {
            assert!(world.set_block(
                BlockPos::new(x, pos.y() + 1, z),
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_NONE,
            ));
        }
    }

    assert!(!TreeGrower::DarkOak.grow_tree(&world, pos, state, &mut ZeroRng));
    for sapling_pos in square_from(pos) {
        assert_eq!(world.get_block_state(sapling_pos), state);
    }
}

#[test]
fn mega_tree_grows_from_any_corner_of_the_square() {
    init_vanilla_registry();
    init_behaviors();
    let (world, pos) = prepared_tree_world("tree_grower_mega_from_far_corner");
    let state = vanilla_blocks::DARK_OAK_SAPLING.default_state();
    let square = square_from(pos);
    for sapling_pos in square {
        assert!(world.set_block(sapling_pos, state, UpdateFlags::UPDATE_NONE));
    }

    let far_corner = pos.east().south();
    assert!(grow_with_any_seed(
        TreeGrower::DarkOak,
        &world,
        far_corner,
        state
    ));
    assert!(has_log_near(&world, pos, &vanilla_blocks::DARK_OAK_LOG));
    assert!(
        square
            .iter()
            .all(|corner| world.get_block_state(*corner) != state)
    );
}

struct GrowerVariant {
    key: &'static str,
    sapling: BlockRef,
    grower: TreeGrower,
    log: BlockRef,
    two_by_two: bool,
}

const fn variant(
    key: &'static str,
    sapling: BlockRef,
    grower: TreeGrower,
    log: BlockRef,
    two_by_two: bool,
) -> GrowerVariant {
    GrowerVariant {
        key,
        sapling,
        grower,
        log,
        two_by_two,
    }
}

fn grower_variants() -> [GrowerVariant; 11] {
    use vanilla_blocks::*;
    [
        variant(
            "tree_grower_oak",
            &OAK_SAPLING,
            TreeGrower::Oak,
            &OAK_LOG,
            false,
        ),
        variant(
            "tree_grower_spruce",
            &SPRUCE_SAPLING,
            TreeGrower::Spruce,
            &SPRUCE_LOG,
            false,
        ),
        variant(
            "tree_grower_birch",
            &BIRCH_SAPLING,
            TreeGrower::Birch,
            &BIRCH_LOG,
            false,
        ),
        variant(
            "tree_grower_jungle",
            &JUNGLE_SAPLING,
            TreeGrower::Jungle,
            &JUNGLE_LOG,
            false,
        ),
        variant(
            "tree_grower_acacia",
            &ACACIA_SAPLING,
            TreeGrower::Acacia,
            &ACACIA_LOG,
            false,
        ),
        variant(
            "tree_grower_cherry",
            &CHERRY_SAPLING,
            TreeGrower::Cherry,
            &CHERRY_LOG,
            false,
        ),
        variant(
            "tree_grower_azalea",
            &AZALEA,
            TreeGrower::Azalea,
            &OAK_LOG,
            false,
        ),
        variant(
            "tree_grower_dark_oak",
            &DARK_OAK_SAPLING,
            TreeGrower::DarkOak,
            &DARK_OAK_LOG,
            true,
        ),
        variant(
            "tree_grower_pale_oak",
            &PALE_OAK_SAPLING,
            TreeGrower::PaleOak,
            &PALE_OAK_LOG,
            true,
        ),
        variant(
            "tree_grower_spruce_mega",
            &SPRUCE_SAPLING,
            TreeGrower::Spruce,
            &SPRUCE_LOG,
            true,
        ),
        variant(
            "tree_grower_jungle_mega",
            &JUNGLE_SAPLING,
            TreeGrower::Jungle,
            &JUNGLE_LOG,
            true,
        ),
    ]
}

#[test]
fn every_grower_places_its_own_tree() {
    init_vanilla_registry();
    init_behaviors();

    for GrowerVariant {
        key,
        sapling,
        grower,
        log,
        two_by_two,
    } in grower_variants()
    {
        let (world, pos) = prepared_tree_world(key);
        let state = sapling.default_state();
        let sapling_count = if two_by_two { 4 } else { 1 };
        for sapling_pos in square_from(pos).into_iter().take(sapling_count) {
            assert!(world.set_block(sapling_pos, state, UpdateFlags::UPDATE_NONE));
        }

        assert!(
            grow_with_any_seed(grower, &world, pos, state),
            "{key} should generate a tree"
        );
        assert!(
            has_log_near(&world, pos, log),
            "{key} should place its logs"
        );
        assert_ne!(
            world.get_block_state(pos),
            state,
            "{key} should consume the sapling"
        );
    }
}
