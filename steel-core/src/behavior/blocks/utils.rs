use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks::{
    BARRIER, CARVED_PUMPKIN, JACK_O_LANTERN, MANGROVE_LEAVES, MELON, PUMPKIN,
};
use steel_utils::Direction;

pub fn is_excluded_for_connection(block: BlockRef) -> bool {
    block.has_tag(&BlockTag::LEAVES)
        || block == &BARRIER
        || block == &CARVED_PUMPKIN
        || block == &JACK_O_LANTERN
        || block == &MELON
        || block == &PUMPKIN
        || block.has_tag(&BlockTag::SHULKER_BOXES)
        || block == &MANGROVE_LEAVES
}

/// Vanilla `MultifaceBlock.getFaceProperty(faceDirection)`.
pub(crate) const fn multiface_face_property(direction: Direction) -> &'static BoolProperty {
    match direction {
        Direction::Up => &BlockStateProperties::UP,
        Direction::Down => &BlockStateProperties::DOWN,
        Direction::North => &BlockStateProperties::NORTH,
        Direction::South => &BlockStateProperties::SOUTH,
        Direction::East => &BlockStateProperties::EAST,
        Direction::West => &BlockStateProperties::WEST,
    }
}
