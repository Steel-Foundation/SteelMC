use super::*;

pub(super) const fn does_block_have_partial_collision(path_type: PathType) -> bool {
    matches!(
        path_type,
        PathType::Fence | PathType::DoorWoodClosed | PathType::DoorIronClosed
    )
}

pub struct WalkPathEvaluator;

impl WalkPathEvaluator {
    #[must_use]
    pub fn path_type(context: &mut PathfindingContext<'_>, x: i32, y: i32, z: i32) -> PathType {
        Self::path_type_static(context, BlockPos::new(x, y, z))
    }

    #[must_use]
    pub fn path_type_static(context: &mut PathfindingContext<'_>, pos: BlockPos) -> PathType {
        let x = pos.x();
        let y = pos.y();
        let z = pos.z();
        let block_path_type = context.get_path_type_from_state(x, y, z);
        if block_path_type != PathType::Open || y < context.level().min_y() + 1 {
            return block_path_type;
        }

        match context.get_path_type_from_state(x, y - 1, z) {
            PathType::Open | PathType::Water | PathType::Lava | PathType::Walkable => {
                PathType::Open
            }
            PathType::Fire => PathType::Fire,
            PathType::Damaging => PathType::Damaging,
            PathType::StickyHoney => PathType::StickyHoney,
            PathType::PowderSnow => PathType::OnTopOfPowderSnow,
            PathType::DamageCautious => PathType::DamageCautious,
            PathType::Trapdoor => PathType::OnTopOfTrapdoor,
            _ => Self::check_neighbour_blocks(context, x, y, z, PathType::Walkable),
        }
    }

    #[must_use]
    pub fn check_neighbour_blocks(
        context: &mut PathfindingContext<'_>,
        x: i32,
        y: i32,
        z: i32,
        block_path_type: PathType,
    ) -> PathType {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }

                    match context.get_path_type_from_state(x + dx, y + dy, z + dz) {
                        PathType::Damaging => return PathType::DamagingInNeighbor,
                        PathType::Fire | PathType::Lava => return PathType::FireInNeighbor,
                        PathType::Water => return PathType::WaterBorder,
                        PathType::DamageCautious => return PathType::DamageCautious,
                        _ => {}
                    }
                }
            }
        }

        block_path_type
    }

    #[must_use]
    pub fn path_type_from_state(level: &dyn LevelReader, pos: BlockPos) -> PathType {
        let block_state = level.get_block_state(pos);
        let block = block_state.get_block();
        if block_state.is_air() {
            return PathType::Open;
        }

        if block.has_tag(&BlockTag::TRAPDOORS)
            || block == &vanilla_blocks::LILY_PAD
            || block == &vanilla_blocks::BIG_DRIPLEAF
        {
            return PathType::Trapdoor;
        }

        if block == &vanilla_blocks::POWDER_SNOW {
            return PathType::PowderSnow;
        }

        if block == &vanilla_blocks::CACTUS || block == &vanilla_blocks::SWEET_BERRY_BUSH {
            return PathType::Damaging;
        }

        if block == &vanilla_blocks::HONEY_BLOCK {
            return PathType::StickyHoney;
        }

        if block == &vanilla_blocks::COCOA {
            return PathType::Cocoa;
        }

        if block == &vanilla_blocks::WITHER_ROSE || block.has_tag(&BlockTag::SPELEOTHEMS) {
            return PathType::DamageCautious;
        }

        let fluid_state = block_state.get_fluid_state();
        if fluid_state.is_lava() {
            return PathType::Lava;
        }

        if Self::is_burning_block(block_state) {
            return PathType::Fire;
        }

        if block.has_tag(&BlockTag::DOORS) {
            return if block_state
                .try_get_value(&BlockStateProperties::OPEN)
                .unwrap_or(false)
            {
                PathType::DoorOpen
            } else if block.has_tag(&BlockTag::MOB_INTERACTABLE_DOORS) {
                PathType::DoorWoodClosed
            } else {
                PathType::DoorIronClosed
            };
        }

        if block.has_tag(&BlockTag::RAILS) {
            return PathType::Rail;
        }

        if block.has_tag(&BlockTag::LEAVES) {
            return PathType::Leaves;
        }

        if block.has_tag(&BlockTag::FENCES)
            || block.has_tag(&BlockTag::WALLS)
            || block.has_tag(&BlockTag::FENCE_GATES)
                && !block_state
                    .try_get_value(&BlockStateProperties::OPEN)
                    .unwrap_or(false)
        {
            return PathType::Fence;
        }

        if !block_state.is_pathfindable(PathComputationType::Land) {
            return PathType::Blocked;
        }

        if fluid_state.is_water() {
            PathType::Water
        } else {
            PathType::Open
        }
    }

    #[must_use]
    pub fn is_burning_block(block_state: steel_utils::BlockStateId) -> bool {
        let block = block_state.get_block();
        block.has_tag(&BlockTag::FIRE)
            || block == &vanilla_blocks::LAVA
            || block == &vanilla_blocks::MAGMA_BLOCK
            || block == &vanilla_blocks::LAVA_CAULDRON
            || block.has_tag(&BlockTag::CAMPFIRES)
                && block_state
                    .try_get_value(&BlockStateProperties::LIT)
                    .unwrap_or(false)
    }
}
