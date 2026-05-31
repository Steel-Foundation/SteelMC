use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_blue_ice_feature(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        if origin.y() > region.sea_level() - 1 {
            return false;
        }

        if !Self::is_water(region.block_state(origin))
            && !Self::is_water(region.block_state(origin.below()))
        {
            return false;
        }

        let mut found_packed_ice = false;
        for direction in Self::VANILLA_DIRECTION_VALUES {
            if direction != Direction::Down
                && region.block_state(origin.relative(direction)).get_block()
                    == &vanilla_blocks::PACKED_ICE
            {
                found_packed_ice = true;
                break;
            }
        }

        if !found_packed_ice {
            return false;
        }

        let blue_ice = vanilla_blocks::BLUE_ICE.default_state();
        let _ = region.set_block_state(origin, blue_ice, UpdateFlags::UPDATE_CLIENTS);

        for _ in 0..200 {
            let y_offset = random.next_i32_bounded(5) - random.next_i32_bounded(6);
            let xz_diff = Self::blue_ice_xz_diff(y_offset);

            if xz_diff < 1 {
                continue;
            }

            let place_pos = origin.offset(
                random.next_i32_bounded(xz_diff) - random.next_i32_bounded(xz_diff),
                y_offset,
                random.next_i32_bounded(xz_diff) - random.next_i32_bounded(xz_diff),
            );
            let place_state = region.block_state(place_pos);
            if !place_state.is_air()
                && !Self::is_water(place_state)
                && place_state.get_block() != &vanilla_blocks::PACKED_ICE
                && place_state.get_block() != &vanilla_blocks::ICE
            {
                continue;
            }

            for direction in Self::VANILLA_DIRECTION_VALUES {
                if region
                    .block_state(place_pos.relative(direction))
                    .get_block()
                    == &vanilla_blocks::BLUE_ICE
                {
                    let _ =
                        region.set_block_state(place_pos, blue_ice, UpdateFlags::UPDATE_CLIENTS);
                    break;
                }
            }
        }

        true
    }

    fn is_water(state: BlockStateId) -> bool {
        state.get_block() == &vanilla_blocks::WATER
    }

    pub(in crate::worldgen::feature) const fn blue_ice_xz_diff(y_offset: i32) -> i32 {
        let mut xz_diff = 3;
        if y_offset < 2 {
            xz_diff += y_offset / 2;
        }
        xz_diff
    }
}
