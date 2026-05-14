use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::TreePlacement;

use crate::block_entity::entities::BeehiveBlockEntity;

const BEEHIVE_WORLDGEN_FACING: Direction = Direction::South;
const BEEHIVE_SPAWN_DIRECTIONS: [Direction; 3] =
    [Direction::East, Direction::South, Direction::West];

impl FeatureDecorationRunner {
    pub(super) fn place_tree_decorators(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) {
        for decorator in &config.decorators {
            match decorator {
                TreeDecorator::Beehive { probability } => {
                    Self::place_beehive_tree_decorator(
                        region,
                        registry,
                        random,
                        *probability,
                        placement,
                    );
                }
                TreeDecorator::AlterGround { .. }
                | TreeDecorator::Cocoa { .. }
                | TreeDecorator::CreakingHeart { .. }
                | TreeDecorator::LeaveVine { .. }
                | TreeDecorator::TrunkVine
                | TreeDecorator::AttachedToLeaves(_)
                | TreeDecorator::AttachedToLogs(_)
                | TreeDecorator::PlaceOnGround(_)
                | TreeDecorator::PaleMoss { .. } => {
                    panic!(
                        "tree decorator requires runtime support before minecraft:tree can be registered"
                    );
                }
            }
        }
    }

    fn place_beehive_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        probability: f32,
        placement: &mut TreePlacement,
    ) {
        let logs = Self::sorted_tree_positions(&placement.trunks);
        if logs.is_empty() || random.next_f32() >= probability {
            return;
        }

        let leaves = Self::sorted_tree_positions(&placement.foliage);
        let hive_y = if let Some(first_leaf) = leaves.first() {
            (first_leaf.y() - 1).max(logs[0].y() + 1)
        } else {
            let log_y = logs[0].y() + 1 + random.next_i32_bounded(3);
            let last_log_y = logs[logs.len() - 1].y();
            log_y.min(last_log_y)
        };

        let mut hive_placements = Vec::new();
        for log in logs.iter().copied().filter(|pos| pos.y() == hive_y) {
            for direction in BEEHIVE_SPAWN_DIRECTIONS {
                hive_placements.push(log.relative(direction));
            }
        }

        if hive_placements.is_empty() {
            return;
        }

        Self::shuffle_tree_positions(random, &mut hive_placements);
        let hive_pos = hive_placements.into_iter().find(|pos| {
            region.block_state(*pos).is_air()
                && region
                    .block_state(pos.relative(BEEHIVE_WORLDGEN_FACING))
                    .is_air()
        });
        let Some(hive_pos) = hive_pos else {
            return;
        };

        let hive_state = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::BEE_NEST)
            .set_value(
                &BlockStateProperties::HORIZONTAL_FACING,
                BEEHIVE_WORLDGEN_FACING,
            );
        placement.set_decoration(region, hive_pos, hive_state);

        let Some(block_entity) = region.block_entity(hive_pos) else {
            return;
        };
        let mut block_entity = block_entity.lock();
        let Some(beehive) = block_entity
            .as_any_mut()
            .downcast_mut::<BeehiveBlockEntity>()
        else {
            return;
        };

        let num_bees = 2 + random.next_i32_bounded(2);
        for _ in 0..num_bees {
            beehive.store_worldgen_bee(random.next_i32_bounded(599));
        }
    }

    fn sorted_tree_positions(positions: &FxHashSet<BlockPos>) -> Vec<BlockPos> {
        let mut positions = positions.iter().copied().collect::<Vec<_>>();
        positions.sort_by_key(BlockPos::y);
        positions
    }

    fn shuffle_tree_positions(random: &mut Xoroshiro, positions: &mut [BlockPos]) {
        for i in (1..positions.len()).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!("tree decorator shuffle length exceeds i32 range");
            };
            let j = random.next_i32_bounded(bound) as usize;
            positions.swap(i, j);
        }
    }
}
