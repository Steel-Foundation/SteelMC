use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_vines_feature(
        region: &mut WorldGenRegion<'_>,
        origin: BlockPos,
    ) -> bool {
        if !region.block_state(origin).is_air() {
            return false;
        }

        for direction in Self::VANILLA_DIRECTION_VALUES {
            if direction == Direction::Down {
                continue;
            }

            if Self::can_attach_to_multiface(region, origin, direction) {
                let _ = region.set_block_state(
                    origin,
                    Self::vine_state_for_face(direction),
                    UpdateFlags::UPDATE_CLIENTS,
                );
                return true;
            }
        }

        false
    }

    pub(in crate::worldgen::feature) fn vine_state_for_face(direction: Direction) -> BlockStateId {
        let vine = vanilla_blocks::VINE.default_state();
        match direction {
            Direction::Up => vine.set_value(&BlockStateProperties::UP, true),
            Direction::North => vine.set_value(&BlockStateProperties::NORTH, true),
            Direction::South => vine.set_value(&BlockStateProperties::SOUTH, true),
            Direction::West => vine.set_value(&BlockStateProperties::WEST, true),
            Direction::East => vine.set_value(&BlockStateProperties::EAST, true),
            Direction::Down => {
                panic!("vine has no face property for downward attachment")
            }
        }
    }
}
