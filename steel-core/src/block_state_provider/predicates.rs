use super::BlockStateProviderEvaluator;
use crate::{
    behavior::BLOCK_BEHAVIORS, fluid::state::get_fluid_state_from_block, world::LevelReader,
};
use glam::IVec3;
use steel_registry::{
    Registry, blocks::block_state_ext::BlockStateExt as _, feature::BlockPredicate,
};
use steel_utils::BlockPos;

impl BlockStateProviderEvaluator {
    pub(crate) fn test_block_predicate(
        level: &dyn LevelReader,
        registry: &Registry,
        predicate: &BlockPredicate,
        origin: BlockPos,
    ) -> bool {
        match predicate {
            BlockPredicate::True => true,
            BlockPredicate::AllOf { predicates } => predicates
                .iter()
                .all(|predicate| Self::test_block_predicate(level, registry, predicate, origin)),
            BlockPredicate::AnyOf { predicates } => predicates
                .iter()
                .any(|predicate| Self::test_block_predicate(level, registry, predicate, origin)),
            BlockPredicate::Not { predicate } => {
                !Self::test_block_predicate(level, registry, predicate, origin)
            }
            BlockPredicate::MatchingBlockTag { tag, offset } => {
                let state = level.get_block_state(Self::offset(origin, offset));
                state.get_block().has_tag(tag)
            }
            BlockPredicate::MatchingBlocks { blocks, offset } => {
                let state = level.get_block_state(Self::offset(origin, offset));
                blocks.0.contains(&state.get_block())
            }
            BlockPredicate::MatchingFluids { fluids, offset } => {
                let state = level.get_block_state(Self::offset(origin, offset));
                let fluid_state = get_fluid_state_from_block(state);
                fluids.0.contains(&fluid_state.fluid_id)
            }
            BlockPredicate::Solid { offset } => level
                .get_block_state(Self::offset(origin, offset))
                .is_solid(),
            BlockPredicate::WouldSurvive { state, offset } => {
                let state = Self::block_state_from_data(registry, state);
                let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
                behavior.can_survive(state, level, Self::offset(origin, offset))
            }
            BlockPredicate::Replaceable { offset } => level
                .get_block_state(Self::offset(origin, offset))
                .is_replaceable(),
            BlockPredicate::HasSturdyFace { direction, offset } => {
                let position = Self::offset(origin, offset);
                level
                    .get_block_state(position)
                    .is_face_sturdy_at(position, *direction)
            }
            BlockPredicate::InsideWorldBounds { offset } => {
                let position = Self::offset(origin, offset);
                !level.is_outside_build_height(position.y())
            }
            BlockPredicate::HeightRange {
                min_inclusive,
                max_inclusive,
            } => {
                let min_y =
                    min_inclusive.resolve_y(level.min_y(), level.height(), level.sea_level());
                let max_y =
                    max_inclusive.resolve_y(level.min_y(), level.height(), level.sea_level());

                (min_y..=max_y).contains(&origin.y())
            }
            BlockPredicate::VolumeMatch { min, max, matches } => {
                for x in min.x..=max.x {
                    for y in min.y..=max.y {
                        for z in min.z..=max.z {
                            if !Self::test_block_predicate(
                                level,
                                registry,
                                matches,
                                Self::offset(origin, &IVec3::new(x, y, z)),
                            ) {
                                return false;
                            }
                        }
                    }
                }

                true
            }
        }
    }

    pub(crate) fn offset(origin: BlockPos, offset: &IVec3) -> BlockPos {
        BlockPos(origin.0 + offset)
    }
}
